//! `agent-pinball` binary: parse arguments, load config once, call the library.

use std::net::{SocketAddr, TcpListener};
use std::process::ExitCode;

use pinball_cli::{Command, Config, USAGE, parse_args};
use pinball_web::BindPosture;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("agent-pinball: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let command = parse_args(std::env::args().skip(1)).map_err(|err| err.to_string())?;
    let config =
        Config::from_lookup(|key| std::env::var(key).ok()).map_err(|err| err.to_string())?;
    match command {
        Command::Help => print!("{USAGE}"),
        Command::Version => println!("agent-pinball {}", env!("CARGO_PKG_VERSION")),
        Command::Status => status(config.bind)?,
        Command::Serve { bind } => serve(bind.unwrap_or(config.bind), &config)?,
    }
    Ok(())
}

fn status(bind: SocketAddr) -> Result<(), String> {
    match pinball_web::probe_health(bind) {
        Ok(true) => {
            println!("ok");
            Ok(())
        }
        Ok(false) => Err(format!("{bind} answered but is not healthy")),
        Err(err) => Err(format!("cannot reach {bind}: {err}")),
    }
}

fn serve(bind: SocketAddr, config: &Config) -> Result<(), String> {
    let posture =
        pinball_web::check_bind(bind, config.in_container).map_err(|err| err.to_string())?;
    if posture == BindPosture::ContainerNamespace {
        eprintln!(
            "agent-pinball: binding {bind} inside a container; publish it on loopback only \
             (-p 127.0.0.1:{port}:{port})",
            port = bind.port()
        );
    }
    let listener = TcpListener::bind(bind).map_err(|err| format!("cannot bind {bind}: {err}"))?;
    eprintln!("agent-pinball: serving on http://{bind}");
    pinball_web::serve(&listener, &config.allowed_hosts).map_err(|err| err.to_string())
}
