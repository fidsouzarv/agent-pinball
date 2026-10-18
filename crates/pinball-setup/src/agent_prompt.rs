//! `setup --agent`: the report as Markdown another coding agent can act on.

use std::fmt::Write;

use pinball_core::{AgentReport, HookState, SetupReport};

/// Markdown brief: status table, the exact diffs, the commands to run and
/// the rules to follow, then the report as JSON.
#[must_use]
pub fn agent_prompt(report: &SetupReport) -> String {
    let exe = format!("'{}'", report.executable.replace('\'', r"'\''"));
    let mut out = String::from(
        "# agent-pinball setup\n\n\
         agent-pinball is a local, read-only panel for Codex, Cursor and Claude \
         sessions. Setup adds hook entries to each installed agent's config so \
         the agent reports session start, end of turn, compaction and session \
         end to agent-pinball. The hooks only append ids and timestamps to \
         agent-pinball's own state directory; they never read prompts.\n\n\
         ## Status\n\n\
         | Agent | Binary | Config | Hooks | Last event |\n\
         |---|---|---|---|---|\n",
    );
    for agent in &report.agents {
        let _ = writeln!(
            out,
            "| {} | {} | `{}`{} | {} | {} |",
            agent.kind,
            agent
                .binary
                .as_deref()
                .map_or("not on PATH".to_owned(), |b| format!("`{b}`")),
            agent.config_path,
            if agent.root_found {
                ""
            } else {
                " (agent not installed)"
            },
            hook_label(&agent.hooks),
            agent
                .last_event
                .as_ref()
                .map_or("none".to_owned(), |e| format!(
                    "{} at {} ms",
                    e.event, e.at_ms
                )),
        );
    }
    write_changes(&mut out, &report.agents);
    let _ = write!(
        out,
        "\n## Steps\n\n\
         1. Show the user the changes above and get a clear yes. Do not run \
         step 2 without it.\n\
         2. Apply: `{exe} setup --yes` (one agent: add `--only claude`, \
         `--only codex` or `--only cursor`).\n\
         3. Check: `{exe} setup --agent` again; every installed agent should \
         read `installed`.\n\
         4. Codex: ask the user to open Codex and run `/hooks` to trust the new \
         hooks; Codex skips untrusted hooks.\n\
         5. Undo at any time: `{exe} setup --remove --yes`.\n\n\
         ## Rules\n\n\
         - Change agent configs only through `{exe} setup`. Do not edit \
         `settings.json` or `hooks.json` by hand.\n\
         - A config listed as `invalid` is left alone. Tell the user why and \
         let them fix it.\n\
         - Every write keeps a `.agent-pinball-bak-<ms>` copy next to the file.\n"
    );
    let json = serde_json::to_string_pretty(report).unwrap_or_default();
    let _ = write!(out, "\n## Report (JSON)\n\n```json\n{json}\n```\n");
    out
}

fn write_changes(out: &mut String, agents: &[AgentReport]) {
    out.push_str("\n## Changes `setup --yes` would make\n\n");
    let mut any = false;
    for agent in agents {
        if let Some(diff) = &agent.pending_diff {
            any = true;
            let _ = write!(out, "### {}\n\n```diff\n{diff}```\n\n", agent.kind);
        }
    }
    if !any {
        out.push_str("None: every installed agent is up to date.\n");
    }
}

fn hook_label(state: &HookState) -> String {
    match state {
        HookState::Missing => "missing".to_owned(),
        HookState::Installed => "installed".to_owned(),
        HookState::Outdated => "outdated".to_owned(),
        HookState::Invalid { reason } => format!("invalid: {reason}"),
    }
}
