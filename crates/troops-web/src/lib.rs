//! HTTP surface of the agent-troops panel.
//!
//! Std-only while the panel serves nothing but `/healthz`; a framework comes in
//! with the first real page. Read-only by design: see this crate's
//! `clippy.toml`.

use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

/// Largest request head read before answering 431, so a client cannot make
/// the server buffer without bound.
const MAX_HEAD_BYTES: usize = 8 * 1024;
/// Most input discarded after refusing an oversized head.
const MAX_DRAIN_BYTES: u64 = 64 * 1024;
/// A slow or silent client is dropped after this long.
const IO_TIMEOUT: Duration = Duration::from_secs(5);

/// How a permitted bind address is exposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindPosture {
    /// Loopback: reachable only from this machine.
    Loopback,
    /// Non-loopback inside a container. Publishing a port requires binding the
    /// namespace's wildcard address, so reachability is decided by the host's
    /// `-p` spec, which the process cannot see.
    ContainerNamespace,
}

/// Why a bind address was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    /// A non-loopback address outside a container.
    NonLoopback(SocketAddr),
}

impl fmt::Display for BindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonLoopback(addr) => write!(
                f,
                "refusing to serve on non-loopback {addr}: the panel has no authentication yet"
            ),
        }
    }
}

impl std::error::Error for BindError {}

/// Fail-closed bind policy: loopback is always allowed, anything else only
/// inside a container.
pub fn check_bind(addr: SocketAddr, in_container: bool) -> Result<BindPosture, BindError> {
    if addr.ip().is_loopback() {
        Ok(BindPosture::Loopback)
    } else if in_container {
        Ok(BindPosture::ContainerNamespace)
    } else {
        Err(BindError::NonLoopback(addr))
    }
}

/// A response the panel can send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Response {
    /// HTTP status code.
    pub status: u16,
    /// Reason phrase for the status line.
    pub reason: &'static str,
    /// Plain-text body.
    pub body: &'static str,
}

const OK: Response = Response {
    status: 200,
    reason: "OK",
    body: "ok\n",
};
const BAD_REQUEST: Response = Response {
    status: 400,
    reason: "Bad Request",
    body: "bad request\n",
};
const FORBIDDEN: Response = Response {
    status: 403,
    reason: "Forbidden",
    body: "host not allowed\n",
};
const NOT_FOUND: Response = Response {
    status: 404,
    reason: "Not Found",
    body: "not found\n",
};
const METHOD_NOT_ALLOWED: Response = Response {
    status: 405,
    reason: "Method Not Allowed",
    body: "method not allowed\n",
};
const HEAD_TOO_LARGE: Response = Response {
    status: 431,
    reason: "Request Header Fields Too Large",
    body: "request head too large\n",
};

/// Routes one request head. Pure, so the Host guard is testable without a socket.
///
/// The `Host` allowlist runs before routing: it is the DNS-rebinding guard, and
/// a request without `Host` is refused rather than assumed local.
#[must_use]
pub fn route(head: &str, allowed_hosts: &[String]) -> Response {
    let mut lines = head.lines();
    let Some(request_line) = lines.next() else {
        return BAD_REQUEST;
    };
    let mut parts = request_line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return BAD_REQUEST;
    };
    let host = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.trim());
    let Some(host) = host else {
        return BAD_REQUEST;
    };
    if !host_allowed(host, allowed_hosts) {
        return FORBIDDEN;
    }
    match (method, target) {
        ("GET", "/healthz") => OK,
        (_, "/healthz") => METHOD_NOT_ALLOWED,
        _ => NOT_FOUND,
    }
}

fn host_allowed(host: &str, allowed_hosts: &[String]) -> bool {
    let name = strip_port(host);
    allowed_hosts
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(name))
}

/// `localhost:4242` -> `localhost`, `[::1]:4242` -> `::1`. A bare IPv6 address
/// has colons but no port, so it is returned unchanged.
fn strip_port(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        return rest.split_once(']').map_or(host, |(ip, _)| ip);
    }
    match host.rsplit_once(':') {
        Some((name, port)) if !name.contains(':') && port.bytes().all(|b| b.is_ascii_digit()) => {
            name
        }
        _ => host,
    }
}

/// Serves until the listener fails. One connection at a time: the panel has a
/// local viewer, and a sequential loop keeps resource use bounded.
pub fn serve(listener: &TcpListener, allowed_hosts: &[String]) -> io::Result<()> {
    for stream in listener.incoming() {
        // One bad connection must not stop the server.
        if let Err(err) = stream.and_then(|stream| handle(&stream, allowed_hosts)) {
            eprintln!("agent-troops: connection error: {err}");
        }
    }
    Ok(())
}

fn handle(stream: &TcpStream, allowed_hosts: &[String]) -> io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    match read_head(stream)? {
        Some(head) => write_response(stream, route(&head, allowed_hosts)),
        None => {
            write_response(stream, HEAD_TOO_LARGE)?;
            // Closing with unread input makes the kernel send RST, which can
            // discard the 431 before the client reads it. Signal EOF, then
            // discard a bounded amount of what the client is still sending.
            stream.shutdown(Shutdown::Write)?;
            io::copy(&mut stream.take(MAX_DRAIN_BYTES), &mut io::sink())?;
            Ok(())
        }
    }
}

/// Reads up to the blank line ending the head. `None` means it was too large.
fn read_head(stream: &TcpStream) -> io::Result<Option<String>> {
    let limit = u64::try_from(MAX_HEAD_BYTES).unwrap_or(u64::MAX) + 1;
    let mut reader = BufReader::new(stream.take(limit));
    let mut head = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        if head.len() + line.len() > MAX_HEAD_BYTES {
            return Ok(None);
        }
        let end_of_head = line == "\r\n" || line == "\n";
        head.push_str(&line);
        if end_of_head {
            break;
        }
    }
    Ok(Some(head))
}

fn write_response(mut stream: &TcpStream, response: Response) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {} {}\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Connection: close\r\n\r\n{}",
        response.status,
        response.reason,
        response.body.len(),
        response.body
    )?;
    stream.flush()
}

/// Probes `/healthz`, as the container healthcheck does. A wildcard bind is
/// reached through loopback.
pub fn probe_health(bind: SocketAddr) -> io::Result<bool> {
    let mut stream = TcpStream::connect_timeout(&probe_target(bind), IO_TIMEOUT)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
    let mut status_line = String::new();
    BufReader::new(&stream).read_line(&mut status_line)?;
    Ok(status_line.starts_with("HTTP/1.1 200 "))
}

fn probe_target(bind: SocketAddr) -> SocketAddr {
    let ip = match bind.ip() {
        IpAddr::V4(v4) if v4.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(v6) if v6.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    SocketAddr::new(ip, bind.port())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "../tests/suite/mod.rs"]
mod suite;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn defaults() -> Vec<String> {
        ["localhost", "127.0.0.1", "::1"]
            .map(str::to_owned)
            .to_vec()
    }

    #[test]
    fn healthz_on_allowed_host() {
        let head = "GET /healthz HTTP/1.1\r\nHost: localhost:4242\r\n\r\n";
        assert_eq!(route(head, &defaults()), OK);
    }

    #[test]
    fn bracketed_ipv6_host_is_allowed() {
        let head = "GET /healthz HTTP/1.1\r\nHost: [::1]:4242\r\n\r\n";
        assert_eq!(route(head, &defaults()), OK);
    }

    // Adversarial: DNS rebinding sends the attacker's name as Host.
    #[test]
    fn foreign_host_is_refused_before_routing() {
        let head = "GET /healthz HTTP/1.1\r\nHost: evil.example:4242\r\n\r\n";
        assert_eq!(route(head, &defaults()), FORBIDDEN);
        let head = "GET /nope HTTP/1.1\r\nHost: evil.example\r\n\r\n";
        assert_eq!(route(head, &defaults()), FORBIDDEN);
    }

    #[test]
    fn missing_host_is_a_bad_request() {
        assert_eq!(
            route("GET /healthz HTTP/1.1\r\n\r\n", &defaults()),
            BAD_REQUEST
        );
    }

    #[test]
    fn malformed_request_line_is_a_bad_request() {
        assert_eq!(route("", &defaults()), BAD_REQUEST);
        assert_eq!(
            route("GET\r\nHost: localhost\r\n\r\n", &defaults()),
            BAD_REQUEST
        );
    }

    #[test]
    fn write_methods_are_not_allowed() {
        let head = "POST /healthz HTTP/1.1\r\nHost: localhost\r\n\r\n";
        assert_eq!(route(head, &defaults()), METHOD_NOT_ALLOWED);
    }

    #[test]
    fn unknown_path_is_not_found() {
        let head = "GET /admin HTTP/1.1\r\nHost: localhost\r\n\r\n";
        assert_eq!(route(head, &defaults()), NOT_FOUND);
    }

    #[test]
    fn loopback_bind_is_allowed_everywhere() {
        let addr: SocketAddr = "127.0.0.1:4242".parse().unwrap();
        assert_eq!(check_bind(addr, false), Ok(BindPosture::Loopback));
        let addr: SocketAddr = "[::1]:4242".parse().unwrap();
        assert_eq!(check_bind(addr, false), Ok(BindPosture::Loopback));
    }

    // Adversarial: the panel has no auth, so a LAN bind must not start.
    #[test]
    fn wildcard_bind_is_refused_outside_a_container() {
        let addr: SocketAddr = "0.0.0.0:4242".parse().unwrap();
        assert_eq!(check_bind(addr, false), Err(BindError::NonLoopback(addr)));
        assert_eq!(check_bind(addr, true), Ok(BindPosture::ContainerNamespace));
    }

    #[test]
    fn wildcard_probe_goes_through_loopback() {
        let v4: SocketAddr = "0.0.0.0:4242".parse().unwrap();
        assert_eq!(probe_target(v4), "127.0.0.1:4242".parse().unwrap());
        let v6: SocketAddr = "[::]:4242".parse().unwrap();
        assert_eq!(probe_target(v6), "[::1]:4242".parse().unwrap());
    }
}
