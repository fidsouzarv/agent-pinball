//! The setup page: what `agent-pinball setup` found, drawn with the pinball
//! design system (docs/design/COMPONENTS.md). It only shows; writes happen in
//! the terminal (ADR 0006).
//!
//! The page and `/api/setup` share one view model, so the polling script
//! only copies values into the server-rendered markup and holds no logic.

use maud::{DOCTYPE, Markup, html};
use pinball_core::{AgentKind, AgentReport, HookRecord, HookState, SetupReport};
use serde::Serialize;

use crate::Response;

/// What the setup page shows, rebuilt on every request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupPage {
    /// Detection and hook state.
    pub report: SetupReport,
    /// The `setup --agent` brief, offered for copying.
    pub agent_brief: String,
}

/// Builds the page on demand. Supplied by the CLI, which owns detection; this
/// crate never inspects agent files itself.
pub type SetupProvider = Box<dyn Fn() -> SetupPage + Send + Sync>;

const PINBALL_CSS: &str = include_str!("../assets/pinball.css");
const SETUP_CSS: &str = include_str!("../assets/setup.css");
const SETUP_JS: &str = include_str!("../assets/setup.js");

pub(crate) fn asset(path: &str) -> Option<Response> {
    let (content_type, body) = match path {
        "/assets/pinball.css" => ("text/css; charset=utf-8", PINBALL_CSS),
        "/assets/setup.css" => ("text/css; charset=utf-8", SETUP_CSS),
        "/assets/setup.js" => ("text/javascript; charset=utf-8", SETUP_JS),
        _ => return None,
    };
    Some(Response::ok(content_type, body))
}

pub(crate) fn html(page: &SetupPage) -> Response {
    Response::ok(
        "text/html; charset=utf-8",
        render(&view(page), page).into_string(),
    )
}

pub(crate) fn json(page: &SetupPage) -> Response {
    let body = serde_json::to_string(&view(page)).unwrap_or_else(|_| "{}".to_owned());
    Response::ok("application/json", body)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PageView<'a> {
    ready: usize,
    found: usize,
    ticker: String,
    agents: Vec<AgentView>,
    agent_brief: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentView {
    kind: AgentKind,
    meta: &'static str,
    checks: [Check; 4],
    note: Option<String>,
    diff: Option<String>,
}

/// One insert light. `state` picks the lamp shape and colour in setup.css.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Check {
    id: &'static str,
    title: &'static str,
    state: &'static str,
    label: &'static str,
    detail: String,
    at_ms: Option<u64>,
}

impl Check {
    fn new(
        id: &'static str,
        title: &'static str,
        state: &'static str,
        label: &'static str,
    ) -> Self {
        Self {
            id,
            title,
            state,
            label,
            detail: String::new(),
            at_ms: None,
        }
    }

    fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }
}

const fn display_name(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::Claude => "Claude",
        AgentKind::Codex => "Codex",
        AgentKind::Cursor => "Cursor",
    }
}

fn view(page: &SetupPage) -> PageView<'_> {
    let agents = &page.report.agents;
    let found = agents.iter().filter(|a| a.root_found).count();
    let ready = agents
        .iter()
        .filter(|a| a.root_found && a.hooks == HookState::Installed)
        .count();
    let newest = agents
        .iter()
        .filter_map(|a| a.last_event.as_ref())
        .max_by_key(|e| e.at_ms);
    let ticker = match newest {
        Some(event) => ticker_line(event),
        None if found > 0 && ready == found => "ALL WIRED UP".to_owned(),
        None => "RUN SETUP IN THE TERMINAL".to_owned(),
    };
    PageView {
        ready,
        found,
        ticker,
        agents: agents.iter().map(agent_view).collect(),
        agent_brief: &page.agent_brief,
    }
}

/// The DMD is all capitals, project name included.
fn ticker_line(event: &HookRecord) -> String {
    let mut line = format!("{} {}", event.kind, event_label(event));
    if let Some(project) = &event.project {
        line.push(' ');
        line.push_str(project);
    }
    line.to_uppercase()
}

/// The event's name, or for a waiting event what it waits on.
fn event_label(event: &HookRecord) -> String {
    match event.reason {
        Some(reason) => format!("waiting {}", reason.as_str().replace('_', " ")),
        None => event.event.to_string(),
    }
}

fn agent_view(agent: &AgentReport) -> AgentView {
    let meta = match (&agent.hooks, agent.root_found) {
        (_, false) => "not installed",
        (HookState::Installed, true) => "ready",
        (HookState::Invalid { .. }, true) => "needs attention",
        (HookState::Missing | HookState::Outdated, true) => "run setup",
    };
    AgentView {
        kind: agent.kind,
        meta,
        checks: [
            binary_check(agent),
            root_check(agent),
            hooks_check(agent),
            event_check(agent),
        ],
        note: agent.note.clone(),
        diff: agent.pending_diff.clone(),
    }
}

fn binary_check(agent: &AgentReport) -> Check {
    match &agent.binary {
        Some(path) => Check::new("binary", "Binary", "ok", "found").detail(path),
        None => Check::new("binary", "Binary", "absent", "not on PATH"),
    }
}

fn root_check(agent: &AgentReport) -> Check {
    let (state, label) = if agent.root_found {
        ("ok", "found")
    } else {
        ("absent", "not found")
    };
    Check::new("root", "Data dir", state, label).detail(&agent.root)
}

fn hooks_check(agent: &AgentReport) -> Check {
    let (state, label) = match (&agent.hooks, agent.root_found) {
        (_, false) => ("absent", "skipped"),
        (HookState::Installed, true) => ("ok", "installed"),
        (HookState::Missing, true) => ("todo", "missing"),
        (HookState::Outdated, true) => ("todo", "outdated"),
        (HookState::Invalid { .. }, true) => ("error", "invalid"),
    };
    let detail = match &agent.hooks {
        HookState::Invalid { reason } => format!("{}: {reason}", agent.config_path),
        _ => agent.config_path.clone(),
    };
    Check::new("hooks", "Hooks", state, label).detail(detail)
}

fn event_check(agent: &AgentReport) -> Check {
    match &agent.last_event {
        Some(event) => {
            let mut detail = event_label(event);
            for part in [&event.project, &event.session_id].into_iter().flatten() {
                detail.push_str(" · ");
                detail.push_str(part);
            }
            let mut check = Check::new("event", "Last event", "ok", "received").detail(detail);
            check.at_ms = Some(event.at_ms);
            check
        }
        None if agent.hooks == HookState::Installed => {
            Check::new("event", "Last event", "waiting", "waiting").detail(format!(
                "start a {} session to see it here",
                display_name(agent.kind)
            ))
        }
        None => Check::new("event", "Last event", "absent", "none"),
    }
}

fn render(view: &PageView<'_>, page: &SetupPage) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" data-view="flat" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Agent Pinball setup" }
                link rel="stylesheet" href="/assets/pinball.css";
                link rel="stylesheet" href="/assets/setup.css";
                script src="/assets/setup.js" defer {}
            }
            body data-page="setup" {
                div.starfield aria-hidden="true" {}
                p #announcer.visually-hidden aria-live="polite" {}
                main.cabinet {
                    (backbox(view))
                    section.table aria-label="Agents" {
                        div.playfield.setup-playfield {
                            div.lanes { @for agent in &view.agents { (lane(agent)) } }
                            (apron(view, page))
                        }
                    }
                    (coin_door())
                }
            }
        }
    }
}

fn backbox(view: &PageView<'_>) -> Markup {
    html! {
        header.backbox {
            div.marquee {
                div.bulbs data-bulbs="30" aria-hidden="true" {}
                h1.title { span.glint { "Agent Pinball" } }
                p.tagline { "Setup · wire your agents to the table" }
                div.bulbs data-bulbs="30" aria-hidden="true" {}
            }
            div.dmd {
                div.dmd-screen {
                    div.dmd-main {
                        span.dmd-score.dmd-dots data-slot="ready" { (view.ready) "/" (view.found) }
                        small.dmd-dots.dmd-fine { "READY" }
                    }
                    div.dmd-ticker.dmd-dots.dmd-fine data-slot="ticker" { (view.ticker) }
                }
            }
        }
    }
}

fn lane(agent: &AgentView) -> Markup {
    let kind = agent.kind.as_str();
    let heading = format!("lane-{kind}");
    html! {
        section.lane data-kind=(kind) data-agent=(kind) aria-labelledby=(heading) {
            div.lane-head {
                h2.lane-name id=(heading) { (display_name(agent.kind)) }
                div.lane-meta data-slot="meta" { (agent.meta) }
                div.chevrons aria-hidden="true" {}
            }
            div.lane-body {
                article.setup-card data-kind=(kind) {
                    ul.checks {
                        @for check in &agent.checks { (check_row(check)) }
                    }
                    p.setup-note data-slot="note" hidden[agent.note.is_none()] {
                        (agent.note.as_deref().unwrap_or(""))
                    }
                    details.setup-diff data-slot="diff" hidden[agent.diff.is_none()] {
                        summary { "Change setup will make" }
                        pre { code { (agent.diff.as_deref().unwrap_or("")) } }
                    }
                }
            }
        }
    }
}

fn check_row(check: &Check) -> Markup {
    html! {
        li.check data-check-id=(check.id) {
            span.insert data-check=(check.state) {
                span.insert-lamp {}
                span.check-label { (check.label) }
            }
            span.check-title { (check.title) }
            span.check-detail { (check.detail) }
            time.check-time data-at-ms=[check.at_ms] {}
        }
    }
}

fn apron(view: &PageView<'_>, page: &SetupPage) -> Markup {
    let exe = &page.report.executable;
    html! {
        footer.apron {
            section.card {
                h2 { "How to finish" }
                ol.steps {
                    li { "Review each lane's change below its checks." }
                    li { "Answer " kbd { "y" } " in the terminal running " code { "agent-pinball setup --web" } "." }
                    li { "Start a session in each agent. Its last event lights up here." }
                    li { "Codex only: run " code { "/hooks" } " in Codex to trust the new hooks." }
                }
                ul.legend.check-legend {
                    li { span.insert data-check="ok" { span.insert-lamp {} } span { b { "Lit" } " means done." } }
                    li { span.insert data-check="todo" { span.insert-lamp {} } span { b { "Diamond" } " means setup will change it." } }
                    li { span.insert data-check="waiting" { span.insert-lamp {} } span { b { "Blinking" } " means waiting for the first event." } }
                    li { span.insert data-check="error" { span.insert-lamp {} } span { b { "X" } " means setup leaves it alone; see why." } }
                    li { span.insert data-check="absent" { span.insert-lamp {} } span { b { "Dim" } " means not on this machine." } }
                }
            }
            section.card {
                h2 { "Hand it to an agent" }
                p {
                    "Paste this brief into another coding agent, or run "
                    code { (exe) " setup --agent" }
                    ". It applies with " code { "setup --yes" } " only after you agree."
                }
                pre #agent-brief.agent-brief tabindex="0" data-slot="brief" { (view.agent_brief) }
                button.toggle.copy type="button" data-copy="agent-brief" { "Copy brief" }
            }
        }
    }
}

fn coin_door() -> Markup {
    html! {
        footer.coin-door {
            div.coin-slots aria-hidden="true" {
                span.coin-slot data-live="true" {}
                span.coin-slot data-live="true" {}
            }
            div.link-state data-live="true" role="status" { "LIVE" }
            div.toggles {
                a.toggle href="/setup" { "Re-check" }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use pinball_core::{HookEventKind, WaitingReason};

    fn agent(kind: AgentKind, hooks: HookState) -> AgentReport {
        AgentReport {
            kind,
            binary: None,
            root: format!("/h/.{kind}"),
            root_found: true,
            config_path: format!("/h/.{kind}/hooks.json"),
            hooks,
            pending_diff: None,
            last_event: None,
            note: None,
        }
    }

    pub(crate) fn page(agents: Vec<AgentReport>) -> SetupPage {
        SetupPage {
            report: SetupReport {
                executable: "/bin/agent-pinball".to_owned(),
                state_dir: "/s".to_owned(),
                agents,
            },
            agent_brief: "# brief".to_owned(),
        }
    }

    #[test]
    fn counts_ready_agents_and_names_the_newest_event() {
        let mut claude = agent(AgentKind::Claude, HookState::Installed);
        claude.last_event = Some(HookRecord {
            kind: AgentKind::Claude,
            event: HookEventKind::Stop,
            session_id: Some("s1".to_owned()),
            project: Some("api".to_owned()),
            at_ms: 9,
            reason: None,
        });
        let mut cursor = agent(AgentKind::Cursor, HookState::Missing);
        cursor.root_found = false;
        let page = page(vec![
            agent(AgentKind::Codex, HookState::Outdated),
            cursor,
            claude,
        ]);
        let view = view(&page);
        assert_eq!((view.ready, view.found), (1, 2));
        assert_eq!(view.ticker, "CLAUDE STOP API");
        assert_eq!(view.agents[0].checks[2].label, "outdated");
        assert_eq!(view.agents[1].checks[2].state, "absent");
        assert_eq!(view.agents[2].checks[3].detail, "stop · api · s1");
    }

    #[test]
    fn a_waiting_event_names_its_reason() {
        let mut claude = agent(AgentKind::Claude, HookState::Installed);
        claude.last_event = Some(HookRecord {
            kind: AgentKind::Claude,
            event: HookEventKind::AwaitingInput,
            session_id: None,
            project: Some("api".to_owned()),
            at_ms: 9,
            reason: Some(WaitingReason::IdlePrompt),
        });
        let page = page(vec![claude]);
        let view = view(&page);
        assert_eq!(view.ticker, "CLAUDE WAITING IDLE PROMPT API");
        assert_eq!(view.agents[0].checks[3].detail, "waiting idle prompt · api");
    }

    #[test]
    fn installed_without_events_is_waiting() {
        let page = page(vec![agent(AgentKind::Codex, HookState::Installed)]);
        let view = view(&page);
        assert_eq!(view.agents[0].checks[3].state, "waiting");
        assert_eq!(view.ticker, "ALL WIRED UP");
    }

    // Adversarial: paths, reasons and the brief come from the user's machine
    // and must render as text, never markup.
    #[test]
    fn report_text_is_escaped() {
        let mut codex = agent(
            AgentKind::Codex,
            HookState::Invalid {
                reason: "<img src=x onerror=alert(1)>".to_owned(),
            },
        );
        codex.root = "/h/<script>alert(1)</script>".to_owned();
        let mut page = page(vec![codex]);
        page.agent_brief = "</pre><script>steal()</script>".to_owned();
        let body = html(&page).body;
        assert!(!body.contains("<script>alert(1)"), "{body}");
        assert!(!body.contains("<script>steal"), "{body}");
        assert!(!body.contains("<img src=x"), "{body}");
        assert!(body.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    }

    #[test]
    fn page_loads_only_same_origin_assets() {
        let body = html(&page(vec![])).body;
        assert!(!body.contains("style=\""));
        assert!(!body.contains("<script>"), "no inline script under the CSP");
        for asset in [
            "/assets/pinball.css",
            "/assets/setup.css",
            "/assets/setup.js",
        ] {
            assert!(body.contains(asset), "{asset}");
            assert!(super::asset(asset).is_some());
        }
    }

    // The design system has one source; this copy must not drift from it.
    #[test]
    fn pinball_css_matches_the_design_prototype() {
        assert_eq!(
            PINBALL_CSS,
            include_str!("../../../docs/design/prototype/pinball.css")
        );
    }
}
