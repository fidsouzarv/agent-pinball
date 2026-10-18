//! HTTP surface of the agent-pinball panel.
//!
//! axum routes the requests; [`serve`] owns each connection, so the head cap,
//! the connection deadline and the write timeout hold before a request reaches
//! axum (`conn.rs`). Read-only by design: see this crate's `clippy.toml`.
//! Every route is `GET`.

mod conn;
mod setup_page;

use std::borrow::Cow;
use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{Request, State};
use axum::handler::Handler;
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, HOST, HeaderName, REFERRER_POLICY,
    X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::IntoResponse;
use axum::routing::{MethodRouter, get};

pub use conn::serve;

/// Connect and probe timeout, and how long one write may stall.
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

pub use setup_page::{SetupPage, SetupProvider};

/// Sent on every response, refusals included. No inline script or style is
/// ever needed, so the policy can stay this strict (docs/SYSTEM_DESIGN.md §8).
const SECURITY_HEADERS: [(HeaderName, &str); 4] = [
    (
        CONTENT_SECURITY_POLICY,
        "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; \
         connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
    ),
    (X_CONTENT_TYPE_OPTIONS, "nosniff"),
    (REFERRER_POLICY, "no-referrer"),
    (CACHE_CONTROL, "no-cache"),
];

const TEXT: &str = "text/plain; charset=utf-8";

/// A response the panel can send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Response {
    status: StatusCode,
    content_type: &'static str,
    body: Cow<'static, str>,
}

impl Response {
    const fn text(status: StatusCode, body: &'static str) -> Self {
        Self {
            status,
            content_type: TEXT,
            body: Cow::Borrowed(body),
        }
    }

    pub(crate) fn ok(content_type: &'static str, body: impl Into<Cow<'static, str>>) -> Self {
        Self {
            status: StatusCode::OK,
            content_type,
            body: body.into(),
        }
    }
}

impl IntoResponse for Response {
    fn into_response(self) -> axum::response::Response {
        (self.status, [(CONTENT_TYPE, self.content_type)], self.body).into_response()
    }
}

const OK: Response = Response::text(StatusCode::OK, "ok\n");
const BAD_REQUEST: Response = Response::text(StatusCode::BAD_REQUEST, "bad request\n");
const FORBIDDEN: Response = Response::text(StatusCode::FORBIDDEN, "host not allowed\n");
const NOT_FOUND: Response = Response::text(StatusCode::NOT_FOUND, "not found\n");
const METHOD_NOT_ALLOWED: Response =
    Response::text(StatusCode::METHOD_NOT_ALLOWED, "method not allowed\n");
const HEAD_TOO_LARGE: Response = Response::text(
    StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
    "request head too large\n",
);
const INTERNAL_ERROR: Response =
    Response::text(StatusCode::INTERNAL_SERVER_ERROR, "internal error\n");

/// What the server answers for: the Host allowlist and the pages it has.
pub struct Site {
    /// Accepted `Host` header values, without port.
    pub allowed_hosts: Vec<String>,
    /// Serves `/setup` and `/api/setup` when present.
    pub setup: Option<SetupProvider>,
}

impl Site {
    /// A site with only `/healthz`.
    #[must_use]
    pub fn new(allowed_hosts: Vec<String>) -> Self {
        Self {
            allowed_hosts,
            setup: None,
        }
    }
}

/// The panel's routes behind the `Host` guard.
///
/// The guard is the last layer added, so it wraps every route and the
/// fallback: it is the DNS-rebinding guard, and it runs before routing.
pub(crate) fn app(site: Arc<Site>) -> Router {
    let mut router = Router::new().route("/healthz", get_only(async || OK));
    for (path, content_type, body) in setup_page::ASSETS {
        router = router.route(
            path,
            get_only(async move || Response::ok(content_type, body)),
        );
    }
    if site.setup.is_some() {
        let html = get_only(async |State(site)| with_setup(site, setup_page::html).await);
        let json = get_only(async |State(site)| with_setup(site, setup_page::json).await);
        router = router
            .route("/", html.clone())
            .route("/setup", html)
            .route("/api/setup", json);
    }
    router
        .fallback(async || NOT_FOUND)
        .with_state(Arc::clone(&site))
        .layer(middleware::from_fn_with_state(site, guard))
}

/// `GET` only. axum also answers `HEAD` from a `GET` route, so it is refused
/// explicitly along with every other method.
fn get_only<H, T>(handler: H) -> MethodRouter<Arc<Site>>
where
    H: Handler<T, Arc<Site>>,
    T: 'static,
{
    get(handler)
        .head(async || METHOD_NOT_ALLOWED)
        .fallback(async || METHOD_NOT_ALLOWED)
}

/// Builds the setup page off the async workers: the provider reads agent
/// config files.
async fn with_setup(site: Arc<Site>, render: fn(&SetupPage) -> Response) -> Response {
    let page = tokio::task::spawn_blocking(move || site.setup.as_ref().map(|page| render(&page())));
    match page.await {
        Ok(Some(response)) => response,
        Ok(None) => NOT_FOUND,
        Err(_) => INTERNAL_ERROR,
    }
}

/// Refuses a foreign or missing `Host` before routing, and stamps the security
/// headers on every response, refusals included.
async fn guard(
    State(site): State<Arc<Site>>,
    request: Request,
    next: Next,
) -> axum::response::Response {
    let mut response = match refusal(&request, &site.allowed_hosts) {
        Some(refused) => refused.into_response(),
        None => next.run(request).await,
    };
    let headers = response.headers_mut();
    for (name, value) in SECURITY_HEADERS {
        headers.insert(name, HeaderValue::from_static(value));
    }
    response
}

/// A request without exactly one readable `Host` is refused rather than
/// assumed local. An absolute-form target names its own authority, which must
/// pass the allowlist too.
fn refusal(request: &Request, allowed_hosts: &[String]) -> Option<Response> {
    let mut hosts = request.headers().get_all(HOST).iter();
    let (Some(host), None) = (hosts.next(), hosts.next()) else {
        return Some(BAD_REQUEST);
    };
    let Ok(host) = host.to_str() else {
        return Some(BAD_REQUEST);
    };
    let authority = request
        .uri()
        .authority()
        .map(|authority| authority.as_str());
    let allowed = |name: &str| host_allowed(name, allowed_hosts);
    if allowed(host) && authority.is_none_or(allowed) {
        None
    } else {
        Some(FORBIDDEN)
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
