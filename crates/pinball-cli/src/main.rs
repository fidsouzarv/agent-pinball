//! `agent-pinball` binary: parse arguments, load config once, call the library.

use std::io::{self, IsTerminal};
use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::process::ExitCode;
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use pinball_cli::{Command, Config, SetupArgs, Terminal, converse, parse_args, setup_env, usage};
use pinball_core::{AgentKind, HookEventKind};
use pinball_setup::{SetupEnv, agent_prompt, report, spool};
use pinball_web::{BindPosture, SetupPage, Site};

fn main() -> ExitCode {
    let command = match parse_args(std::env::args()) {
        Ok(command) => command,
        Err(err) => err.exit(),
    };
    match run(command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("agent-pinball: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> Result<(), String> {
    // Config is read inside each arm that needs it: `hook` runs inside agents
    // and must not fail on an unrelated bad variable.
    let config = || Config::from_lookup(|key| std::env::var(key).ok()).map_err(|e| e.to_string());
    match command {
        Command::Help => print!("{}", usage()),
        Command::Hook {
            state_dir,
            kind,
            event,
        } => hook(&state_dir, kind, event),
        Command::Status => status(config()?.bind)?,
        Command::Serve { bind } => {
            let config = config()?;
            serve(bind.unwrap_or(config.bind), &config)?;
        }
        Command::Setup(args) => setup(&args, &config()?)?,
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// Records one agent hook event. Never fails and prints nothing on stdout:
/// agents read a hook's stdout and exit code (ADR 0006).
fn hook(state_dir: &Path, kind: AgentKind, event: HookEventKind) {
    let payload = spool::read_payload(io::stdin(), spool::PAYLOAD_DEADLINE);
    let record = spool::record(kind, event, &payload, now_ms());
    if let Err(err) = spool::append(state_dir, &record) {
        eprintln!("agent-pinball: cannot record hook event: {err}");
    }
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

/// Binds after the fail-closed loopback check.
fn listen(bind: SocketAddr, config: &Config) -> Result<TcpListener, String> {
    let posture =
        pinball_web::check_bind(bind, config.in_container).map_err(|err| err.to_string())?;
    if posture == BindPosture::ContainerNamespace {
        eprintln!(
            "agent-pinball: binding {bind} inside a container; publish it on loopback only \
             (-p 127.0.0.1:{port}:{port})",
            port = bind.port()
        );
    }
    TcpListener::bind(bind).map_err(|err| format!("cannot bind {bind}: {err}"))
}

fn serve(bind: SocketAddr, config: &Config) -> Result<(), String> {
    let listener = listen(bind, config)?;
    eprintln!("agent-pinball: serving on http://{bind}");
    let site = Site::new(config.allowed_hosts.clone());
    pinball_web::serve(&listener, &site).map_err(|err| err.to_string())
}

fn setup(args: &SetupArgs, config: &Config) -> Result<(), String> {
    let executable = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|err| format!("cannot locate this binary: {err}"))?;
    let env = setup_env(config, executable).map_err(|err| err.to_string())?;
    if args.agent {
        print!("{}", agent_prompt(&report(&env)));
        return Ok(());
    }
    let page = if args.web {
        let bind = args.bind.unwrap_or(config.bind);
        Some((bind, setup_page(bind, config, env.clone())?))
    } else {
        None
    };
    let stdin = io::stdin();
    let mut term = Terminal {
        interactive: stdin.is_terminal(),
        input: stdin.lock(),
        output: io::stdout(),
    };
    let result = converse(args, &env, &mut term, now_ms());
    let Some((bind, server)) = page else {
        return result;
    };
    // The page outlives the terminal step: without a terminal, setup can be
    // applied from another shell with `setup --yes` and the page follows.
    if let Err(message) = result {
        eprintln!("agent-pinball: {message}");
    }
    eprintln!("agent-pinball: setup page stays at http://{bind}/setup; Ctrl-C to quit");
    let _ = server.join();
    Ok(())
}

/// Serves the setup page on its own thread while the terminal asks.
fn setup_page(
    bind: SocketAddr,
    config: &Config,
    env: SetupEnv,
) -> Result<JoinHandle<io::Result<()>>, String> {
    let listener = listen(bind, config)?;
    let site = Site {
        allowed_hosts: config.allowed_hosts.clone(),
        setup: Some(Box::new(move || {
            let report = report(&env);
            SetupPage {
                agent_brief: agent_prompt(&report),
                report,
            }
        })),
    };
    eprintln!("agent-pinball: setup page on http://{bind}/setup");
    Ok(thread::spawn(move || pinball_web::serve(&listener, &site)))
}
