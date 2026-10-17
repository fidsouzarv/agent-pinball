//! The server over a real loopback socket.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;

use crate::{probe_health, serve};

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

#[test]
fn one_bad_connection_does_not_stop_the_server() {
    let addr = start();
    drop(TcpStream::connect(addr).expect("connect"));
    assert!(probe_health(addr).unwrap());
}
