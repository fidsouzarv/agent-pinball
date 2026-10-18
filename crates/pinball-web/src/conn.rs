//! One connection, from accept to close.
//!
//! The request head is read here, under the size cap and the connection
//! deadline, before hyper parses it. hyper would answer an oversized head
//! itself, but without the security headers and closing over unread input,
//! which can reset the connection before the client reads the 431.

use std::fmt::Write as _;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::Router;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::time::{Instant, Sleep, timeout_at};

use crate::{HEAD_TOO_LARGE, IO_TIMEOUT, SECURITY_HEADERS, Site};

/// Largest request head read before answering 431, so a client cannot make
/// the server buffer without bound.
pub(crate) const MAX_HEAD_BYTES: usize = 8 * 1024;
/// Most input discarded after refusing an oversized head.
const MAX_DRAIN_BYTES: u64 = 64 * 1024;

/// How long a connection may take and how many may be open at once.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    /// Total time a connection may spend sending its request head, however it
    /// trickles.
    pub(crate) deadline: Duration,
    /// How long one write may stall on a client that does not read.
    pub(crate) write_timeout: Duration,
    /// Connections served at once. The panel has a local viewer; the cap keeps
    /// stalled clients from growing memory without bound.
    pub(crate) max_connections: usize,
}

const LIMITS: Limits = Limits {
    deadline: Duration::from_secs(10),
    write_timeout: IO_TIMEOUT,
    max_connections: 64,
};

/// Serves until the listener fails. One bad connection never stops the server.
pub async fn serve(listener: TcpListener, site: Site) -> io::Result<()> {
    serve_with(listener, site, LIMITS).await
}

/// `serve` with injectable limits, so tests need not wait seconds.
pub(crate) async fn serve_with(
    listener: TcpListener,
    site: Site,
    limits: Limits,
) -> io::Result<()> {
    let app = crate::app(Arc::new(site));
    let permits = Arc::new(Semaphore::new(limits.max_connections));
    loop {
        // A permit is taken before accepting, so clients over the cap wait in
        // the kernel backlog rather than in memory.
        let permit = Arc::clone(&permits)
            .acquire_owned()
            .await
            .map_err(io::Error::other)?;
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(err) => {
                eprintln!("agent-pinball: accept error: {err}");
                continue;
            }
        };
        let app = app.clone();
        tokio::spawn(async move {
            if let Err(err) = connection(stream, app, limits).await {
                eprintln!("agent-pinball: connection error: {err}");
            }
            drop(permit);
        });
    }
}

async fn connection<S>(mut stream: S, app: Router, limits: Limits) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let until = Instant::now() + limits.deadline;
    let head = timeout_at(until, read_head(&mut stream))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "connection deadline exceeded"))??;
    let Some(head) = head else {
        let conn = Conn::new(Vec::new(), stream, limits.write_timeout);
        return refuse_oversized(conn, until).await;
    };
    let conn = Conn::new(head, stream, limits.write_timeout);
    http1::Builder::new()
        // One request per connection: the deadline above covers its only head.
        .keep_alive(false)
        .title_case_headers(true)
        .serve_connection(TokioIo::new(conn), TowerToHyperService::new(app))
        .await
        .map_err(io::Error::other)
}

/// Reads up to the blank line ending the head. `None` means it was too large.
/// Bytes read past the head are kept for hyper.
async fn read_head(stream: &mut (impl AsyncRead + Unpin)) -> io::Result<Option<Vec<u8>>> {
    let mut head = Vec::with_capacity(1024);
    let mut chunk = [0; 1024];
    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(Some(head));
        }
        head.extend_from_slice(chunk.get(..read).unwrap_or_default());
        if let Some(end) = head_end(&head) {
            return Ok((end <= MAX_HEAD_BYTES).then_some(head));
        }
        if head.len() > MAX_HEAD_BYTES {
            return Ok(None);
        }
    }
}

/// Length of the head including its blank line, accepting bare `\n` endings.
fn head_end(buf: &[u8]) -> Option<usize> {
    let crlf = buf.windows(3).position(|w| w == b"\n\r\n").map(|i| i + 3);
    let lf = buf.windows(2).position(|w| w == b"\n\n").map(|i| i + 2);
    crlf.into_iter().chain(lf).min()
}

async fn refuse_oversized<S>(mut conn: Conn<S>, until: Instant) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    conn.write_all(too_large().as_bytes()).await?;
    // Closing with unread input makes the kernel send RST, which can discard
    // the 431 before the client reads it. Signal EOF, then discard a bounded
    // amount of what the client is still sending, still under the deadline.
    conn.shutdown().await?;
    let (mut rest, mut sink) = (conn.take(MAX_DRAIN_BYTES), tokio::io::sink());
    match timeout_at(until, tokio::io::copy(&mut rest, &mut sink)).await {
        Ok(Err(err)) => Err(err),
        Ok(Ok(_)) | Err(_) => Ok(()),
    }
}

/// The 431, written by hand because hyper never sees this connection.
fn too_large() -> String {
    let response = HEAD_TOO_LARGE;
    let mut out = format!(
        "HTTP/1.1 431 Request Header Fields Too Large\r\n\
         Content-Type: {}\r\nContent-Length: {}\r\n",
        response.content_type,
        response.body.len()
    );
    for (name, value) in SECURITY_HEADERS {
        let _ = write!(out, "{name}: {value}\r\n");
    }
    out.push_str("Connection: close\r\n\r\n");
    out.push_str(&response.body);
    out
}

/// The connection as hyper sees it: the head already read, then the socket,
/// with a timeout on any write that stalls.
struct Conn<S> {
    head: Vec<u8>,
    read: usize,
    stream: S,
    write_timeout: Duration,
    stall: Option<Pin<Box<Sleep>>>,
}

impl<S> Conn<S> {
    fn new(head: Vec<u8>, stream: S, write_timeout: Duration) -> Self {
        Self {
            head,
            read: 0,
            stream,
            write_timeout,
            stall: None,
        }
    }

    /// A pending write starts the stall timer; any progress clears it.
    fn stalled<T>(
        &mut self,
        cx: &mut Context<'_>,
        poll: Poll<io::Result<T>>,
    ) -> Poll<io::Result<T>> {
        if poll.is_ready() {
            self.stall = None;
            return poll;
        }
        let timeout = self.write_timeout;
        let stall = self
            .stall
            .get_or_insert_with(|| Box::pin(tokio::time::sleep(timeout)));
        match stall.as_mut().poll(cx) {
            Poll::Ready(()) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "write stalled",
            ))),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for Conn<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let rest = this.head.get(this.read..).unwrap_or_default();
        if rest.is_empty() {
            return Pin::new(&mut this.stream).poll_read(cx, buf);
        }
        let n = rest.len().min(buf.remaining());
        buf.put_slice(rest.get(..n).unwrap_or_default());
        this.read += n;
        Poll::Ready(Ok(()))
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Conn<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let poll = Pin::new(&mut this.stream).poll_write(cx, buf);
        this.stalled(cx, poll)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let poll = Pin::new(&mut this.stream).poll_flush(cx);
        this.stalled(cx, poll)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let poll = Pin::new(&mut this.stream).poll_shutdown(cx);
        this.stalled(cx, poll)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn head_ends_at_the_first_blank_line() {
        assert_eq!(head_end(b"GET / HTTP/1.1\r\nHost: a\r\n\r\nrest"), Some(27));
        assert_eq!(head_end(b"GET / HTTP/1.1\nHost: a\n\nrest"), Some(24));
        assert_eq!(head_end(b"GET / HTTP/1.1\r\nHost: a\r\n"), None);
    }

    #[tokio::test]
    async fn bytes_past_the_head_reach_hyper() {
        let (mut client, server) = tokio::io::duplex(64);
        client.write_all(b"tail").await.unwrap();
        drop(client);
        let mut conn = Conn::new(b"head ".to_vec(), server, IO_TIMEOUT);
        let mut all = String::new();
        conn.read_to_string(&mut all).await.unwrap();
        assert_eq!(all, "head tail");
    }

    // Adversarial: a client that never reads must not hold a write forever.
    #[tokio::test]
    async fn a_stalled_write_times_out() {
        let (_client, server) = tokio::io::duplex(16);
        let mut conn = Conn::new(Vec::new(), server, Duration::from_millis(50));
        let write = conn.write_all(&[0; 64]);
        let err = tokio::time::timeout(Duration::from_secs(2), write)
            .await
            .expect("the write timeout fired first")
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    }
}
