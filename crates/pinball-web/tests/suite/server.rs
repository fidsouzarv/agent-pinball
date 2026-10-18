//! The server: routing through the router, then over a real loopback socket.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::conn::{Limits, MAX_HEAD_BYTES, serve_with};
use crate::{Site, app, probe_health};

const FAST: Limits = Limits {
    deadline: Duration::from_secs(10),
    write_timeout: Duration::from_secs(5),
    max_connections: 64,
};

fn defaults() -> Site {
    Site::new(
        ["localhost", "127.0.0.1", "::1"]
            .map(str::to_owned)
            .to_vec(),
    )
}

/// Sends one request through the router, without a socket. `hosts` become
/// `Host` headers, so none or several can be sent.
pub(super) async fn call(
    site: Site,
    method: &str,
    target: &str,
    hosts: &[&str],
) -> axum::response::Response {
    let mut request = Request::builder().method(method).uri(target);
    for host in hosts {
        request = request.header("host", *host);
    }
    let request = request.body(Body::empty()).unwrap();
    app(Arc::new(site)).oneshot(request).await.unwrap()
}

pub(super) async fn body(response: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn healthz_on_allowed_host() {
    let response = call(defaults(), "GET", "/healthz", &["localhost:4242"]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body(response).await, "ok\n");
    let response = call(defaults(), "GET", "/healthz?probe=1", &["localhost"]).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn bracketed_ipv6_host_is_allowed() {
    let response = call(defaults(), "GET", "/healthz", &["[::1]:4242"]).await;
    assert_eq!(response.status(), StatusCode::OK);
}

// Adversarial: DNS rebinding sends the attacker's name as Host.
#[tokio::test]
async fn foreign_host_is_refused_before_routing() {
    for path in ["/healthz", "/nope"] {
        let response = call(defaults(), "GET", path, &["evil.example:4242"]).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
        assert_eq!(body(response).await, "host not allowed\n");
    }
    let response = call(defaults(), "POST", "/healthz", &["evil.example"]).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

// Adversarial: an absolute-form target carries its own authority, which must
// not slip past the allowlist behind a local Host header.
#[tokio::test]
async fn foreign_authority_in_the_target_is_refused() {
    let target = "http://evil.example/healthz";
    let response = call(defaults(), "GET", target, &["localhost"]).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let target = "http://localhost:4242/healthz";
    let response = call(defaults(), "GET", target, &["localhost"]).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn missing_or_repeated_host_is_a_bad_request() {
    let response = call(defaults(), "GET", "/healthz", &[]).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = call(
        defaults(),
        "GET",
        "/healthz",
        &["localhost", "evil.example"],
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn write_methods_are_not_allowed() {
    for method in ["POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"] {
        let response = call(defaults(), method, "/healthz", &["localhost"]).await;
        assert_eq!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{method}"
        );
    }
}

#[tokio::test]
async fn unknown_path_is_not_found() {
    for method in ["GET", "POST"] {
        let response = call(defaults(), method, "/admin", &["localhost"]).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method}");
    }
    let response = call(defaults(), "GET", "/assets/nope.js", &["localhost"]).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// The headers hold on refusals too, not only on pages.
#[tokio::test]
async fn every_response_carries_the_security_headers() {
    let cases = [
        ("GET", "/healthz", &["localhost"][..]),
        ("GET", "/healthz", &["evil.example"][..]),
        ("GET", "/healthz", &[][..]),
        ("POST", "/healthz", &["localhost"][..]),
        ("GET", "/nope", &["localhost"][..]),
    ];
    for (method, path, hosts) in cases {
        let response = call(defaults(), method, path, hosts).await;
        let headers = response.headers();
        let csp = headers["content-security-policy"].to_str().unwrap();
        assert!(
            csp.starts_with("default-src 'none'"),
            "{method} {path} {hosts:?}"
        );
        assert!(csp.ends_with("frame-ancestors 'none'"));
        assert_eq!(headers["x-content-type-options"], "nosniff");
        assert_eq!(headers["referrer-policy"], "no-referrer");
        assert_eq!(headers["cache-control"], "no-cache");
    }
}

/// Starts a server on an ephemeral loopback port. The thread is left running;
/// it ends with the test process.
pub(super) fn start_with(site: Site, limits: Limits) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    listener.set_nonblocking(true).expect("nonblocking");
    thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener)?;
            serve_with(listener, site, limits).await
        })
    });
    addr
}

fn start() -> SocketAddr {
    start_with(
        Site::new(vec!["localhost".to_owned(), "127.0.0.1".to_owned()]),
        FAST,
    )
}

pub(super) fn send(addr: SocketAddr, raw: &[u8]) -> String {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream.write_all(raw).expect("write");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read");
    response
}

#[test]
fn healthz_answers_ok_and_probe_agrees() {
    let addr = start();
    let response = send(addr, b"GET /healthz HTTP/1.1\r\nHost: localhost\r\n\r\n");
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
    assert!(response.contains("X-Content-Type-Options: nosniff"));
    assert!(response.contains("Connection: close"));
    assert!(probe_health(addr).unwrap());
}

// Adversarial: a rebinding page reaches the socket with its own Host header.
#[test]
fn foreign_host_gets_403_over_the_wire() {
    let addr = start();
    let response = send(
        addr,
        b"GET /healthz HTTP/1.1\r\nHost: attacker.example\r\n\r\n",
    );
    assert!(response.starts_with("HTTP/1.1 403 "), "{response}");
}

#[test]
fn malformed_request_line_is_a_bad_request() {
    let addr = start();
    let response = send(addr, b"GET\r\nHost: localhost\r\n\r\n");
    assert!(response.starts_with("HTTP/1.1 400 "), "{response}");
}

// Adversarial: a huge head must be refused, not buffered.
#[test]
fn oversized_head_gets_431() {
    let addr = start();
    let mut raw = b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nX-Pad: ".to_vec();
    raw.extend(std::iter::repeat_n(b'a', 16 * 1024));
    raw.extend_from_slice(b"\r\n\r\n");
    let response = send(addr, &raw);
    assert!(response.starts_with("HTTP/1.1 431 "), "{response}");
    assert!(
        response.contains("x-content-type-options: nosniff"),
        "{response}"
    );
}

// Adversarial: the blank line can arrive in the same read that crosses the cap.
#[test]
fn head_one_byte_over_the_cap_gets_431() {
    let addr = start();
    let mut raw = b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nX-Pad: ".to_vec();
    raw.resize(MAX_HEAD_BYTES - 3, b'a');
    raw.extend_from_slice(b"\r\n\r\n");
    let response = send(addr, &raw);
    assert!(response.starts_with("HTTP/1.1 431 "), "{response}");
}

// Adversarial: a head that never ends is refused once it passes the cap, not
// read until the deadline.
#[test]
fn endless_head_is_refused_at_the_cap() {
    let limits = Limits {
        deadline: Duration::from_secs(2),
        ..FAST
    };
    let addr = start_with(Site::new(vec!["localhost".to_owned()]), limits);
    let mut stream = TcpStream::connect(addr).expect("connect");
    let mut raw = b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nX-Pad: ".to_vec();
    raw.resize(MAX_HEAD_BYTES * 2, b'a');
    stream.write_all(&raw).expect("write");
    let started = Instant::now();
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    assert!(response.starts_with("HTTP/1.1 431 "), "{response}");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "refused after {:?}",
        started.elapsed()
    );
}

// The cap is on the head, so a head just under it is still served.
#[test]
fn head_at_the_cap_is_served() {
    let addr = start();
    let mut raw = b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nX-Pad: ".to_vec();
    raw.resize(MAX_HEAD_BYTES - 4, b'a');
    raw.extend_from_slice(b"\r\n\r\n");
    let response = send(addr, &raw);
    assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
}

// Adversarial: a client trickling its head one byte at a time, each well
// inside any per-read timeout, is cut at the connection deadline. The control
// client is served meanwhile.
#[test]
fn trickling_client_is_cut_at_the_connection_deadline() {
    let limits = Limits {
        deadline: Duration::from_millis(300),
        ..FAST
    };
    let addr = start_with(Site::new(vec!["localhost".to_owned()]), limits);

    let mut slow = TcpStream::connect(addr).expect("connect slow client");
    let mut reader = slow.try_clone().expect("clone slow client");
    thread::spawn(move || {
        for byte in b"GET /healthz HTTP/1.1\r\nX-Slow: ".iter().cycle().take(60) {
            if slow.write_all(&[*byte]).is_err() {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
    });

    assert!(probe_health(addr).unwrap());
    let started = Instant::now();
    // The slow client alone would keep its connection for 3s (60 bytes x 50ms).
    // Bounds the test itself when the deadline is missing.
    reader
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout");
    let mut rest = Vec::new();
    let _ = reader.read_to_end(&mut rest);
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "slow client held its connection for {:?}",
        started.elapsed()
    );
    assert!(rest.is_empty(), "no response to an unfinished head");
}

// Adversarial: idle clients beyond the cap wait in the backlog; they cannot
// make the server open more connections than the cap.
#[test]
fn connections_over_the_cap_wait_for_a_free_slot() {
    let limits = Limits {
        deadline: Duration::from_millis(400),
        max_connections: 1,
        ..FAST
    };
    let addr = start_with(Site::new(vec!["localhost".to_owned()]), limits);
    let idle = TcpStream::connect(addr).expect("connect idle client");
    thread::sleep(Duration::from_millis(50));

    let started = Instant::now();
    assert!(probe_health(addr).unwrap());
    assert!(
        started.elapsed() >= Duration::from_millis(250),
        "probe served beside the idle client after {:?}",
        started.elapsed()
    );
    drop(idle);
}

#[test]
fn one_bad_connection_does_not_stop_the_server() {
    let addr = start();
    drop(TcpStream::connect(addr).expect("connect"));
    assert!(probe_health(addr).unwrap());
}
