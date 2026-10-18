//! The setup page over a real loopback socket.

use std::net::{SocketAddr, TcpListener};
use std::thread;

use pinball_core::{AgentKind, AgentReport, HookState, SetupReport};

use super::server::send;
use crate::{SetupPage, Site, route, serve};

fn page() -> SetupPage {
    SetupPage {
        report: SetupReport {
            executable: "/bin/agent-pinball".to_owned(),
            state_dir: "/s".to_owned(),
            agents: vec![AgentReport {
                kind: AgentKind::Claude,
                binary: Some("/bin/claude".to_owned()),
                root: "/h/.claude".to_owned(),
                root_found: true,
                config_path: "/h/.claude/settings.json".to_owned(),
                hooks: HookState::Missing,
                pending_diff: Some("+hooks\n".to_owned()),
                last_event: None,
                note: None,
            }],
        },
        agent_brief: "# agent-pinball setup".to_owned(),
    }
}

fn setup_site() -> Site {
    Site {
        allowed_hosts: vec!["localhost".to_owned()],
        setup: Some(Box::new(page)),
    }
}

fn start() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    let site = setup_site();
    thread::spawn(move || serve(&listener, &site));
    addr
}

fn get(addr: SocketAddr, path: &str, host: &str) -> String {
    send(
        addr,
        format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes(),
    )
}

#[test]
fn setup_page_renders_with_a_strict_policy() {
    let addr = start();
    let response = get(addr, "/setup", "localhost");
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
    assert!(response.contains("Content-Type: text/html; charset=utf-8"));
    assert!(response.contains("Content-Security-Policy: default-src 'none'; script-src 'self'"));
    assert!(response.contains("frame-ancestors 'none'"));
    assert!(response.contains("Referrer-Policy: no-referrer"));
    assert!(response.contains(r#"data-agent="claude""#));
}

#[test]
fn api_and_assets_are_served() {
    let addr = start();
    let api = get(addr, "/api/setup", "localhost");
    assert!(api.contains("Content-Type: application/json"), "{api}");
    assert!(api.contains(r#""ready":0"#));
    assert!(api.contains(r##""agentBrief":"# agent-pinball setup""##));
    let css = get(addr, "/assets/pinball.css", "localhost");
    assert!(css.contains("Content-Type: text/css") && css.contains("--cab-void"));
    let js = get(addr, "/assets/setup.js", "localhost");
    assert!(js.contains("Content-Type: text/javascript"));
}

// Adversarial: a rebinding page must not read the setup report.
#[test]
fn foreign_host_cannot_read_setup() {
    let addr = start();
    for path in ["/setup", "/api/setup", "/assets/setup.js"] {
        let response = get(addr, path, "attacker.example");
        assert!(response.starts_with("HTTP/1.1 403 "), "{path}: {response}");
        assert!(!response.contains("settings.json"));
    }
}

// Adversarial: the page shows; it never accepts writes (ADR 0006).
#[test]
fn setup_routes_refuse_every_other_method() {
    let site = setup_site();
    for method in ["POST", "PUT", "DELETE", "PATCH"] {
        for path in ["/setup", "/api/setup"] {
            let head = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n\r\n");
            assert_eq!(route(&head, &site).status, 405, "{method} {path}");
        }
    }
    let get = "GET /api/setup HTTP/1.1\r\nHost: localhost\r\n\r\n";
    assert_eq!(route(get, &site).status, 200);
}

#[test]
fn setup_is_absent_when_not_configured() {
    let site = Site::new(vec!["localhost".to_owned()]);
    let head = "GET /setup HTTP/1.1\r\nHost: localhost\r\n\r\n";
    assert_eq!(route(head, &site).status, 404);
    let head = "GET /healthz HTTP/1.1\r\nHost: localhost\r\n\r\n";
    assert_eq!(route(head, &site).status, 200);
}
