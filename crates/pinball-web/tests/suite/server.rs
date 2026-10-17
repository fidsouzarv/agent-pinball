//! The server over a real loopback socket.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use crate::{probe_health, serve, serve_with_deadline};

/// Starts a server on an ephemeral loopback port. The thread is left running;
/// it ends with the test process.
fn start() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    let allowed = vec!["localhost".to_owned(), "127.0.0.1".to_owned()];
    thread::spawn(move || serve(&listener, &allowed));
    addr
}

fn send(addr: SocketAddr, raw: &[u8]) -> String {
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

// Adversarial: an endless head must be cut off, not buffered.
#[test]
fn oversized_head_gets_431() {
    let addr = start();
    let mut raw = b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nX-Pad: ".to_vec();
    raw.extend(std::iter::repeat_n(b'a', 16 * 1024));
    raw.extend_from_slice(b"\r\n\r\n");
    let response = send(addr, &raw);
    assert!(response.starts_with("HTTP/1.1 431 "), "{response}");
}

// Adversarial: a client trickling its head one byte at a time, each well
// inside any per-read timeout, must not hold the sequential loop past the
// connection deadline. The control client queued behind it must be served.
#[test]
fn trickling_client_is_cut_at_the_connection_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    let allowed = vec!["localhost".to_owned()];
    thread::spawn(move || serve_with_deadline(&listener, &allowed, Duration::from_millis(300)));

    let mut slow = TcpStream::connect(addr).expect("connect slow client");
    thread::spawn(move || {
        for byte in b"GET /healthz HTTP/1.1\r\nX-Slow: ".iter().cycle().take(60) {
            if slow.write_all(&[*byte]).is_err() {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
    });
    // Let the server pick up the slow client first.
    thread::sleep(Duration::from_millis(50));

    let started = Instant::now();
    assert!(probe_health(addr).unwrap());
    // The slow client alone would hold the loop for 3s (60 bytes x 50ms).
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "control client waited {:?}",
        started.elapsed()
    );
}

#[test]
fn one_bad_connection_does_not_stop_the_server() {
    let addr = start();
    drop(TcpStream::connect(addr).expect("connect"));
    assert!(probe_health(addr).unwrap());
}
