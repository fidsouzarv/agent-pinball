# 6. Setup writes agent hook config, from the CLI only

Status: Accepted. Narrows [ADR 0001](0001-monitor-only-read-only-panel.md)
for one command; the panel itself stays read-only.

## Context

Codex, Cursor and Claude Code all run user-configured hook commands on
session events (session start, end of turn, compaction, session end). A hook
that reports to agent-pinball gives exact turn boundaries without guessing
from file writes, and proves the agent is wired up. Installing one means
writing to each agent's own config file (`~/.claude/settings.json`,
`~/.codex/hooks.json`, `~/.cursor/hooks.json`), which ADR 0001 forbids.

The risk ADR 0001 guards against is a web bug turning into control over the
user's agents. That risk comes from the HTTP surface, not from a command the
user runs in their own terminal.

## Decision

`agent-pinball setup` may add, update and remove agent-pinball's own hook
entries in those three files, under these rules:

- Only the `pinball-setup` crate writes, and only the CLI's `setup` command
  calls it. `pinball-web` and the collectors stay read-only; the web crate's
  `disallowed-methods` are unchanged and the setup page has no write route.
- Only entries whose command is agent-pinball's `hook` subcommand are touched.
  Every other entry, key and ordering is preserved.
- Every write is shown first as a diff and needs confirmation: `y` on an
  interactive terminal, or an explicit `--yes`. Without a terminal and
  without `--yes`, nothing is written.
- Before writing, the original file is copied to
  `<file>.agent-pinball-bak-<timestamp>`; the write is atomic (temp file in
  the same directory, then rename) and keeps the file's permissions.
- A file that is not valid JSON is refused, never overwritten.
- agent-pinball never records trust for its own hooks (Codex keeps hook
  approval in `config.toml`); approving them stays the user's decision in the
  agent.
- `setup --remove` takes the entries out again.

The hook command (`agent-pinball hook <kind> <event>`) only appends a small,
sanitized record to agent-pinball's own state directory. It reads no prompt,
tool input or transcript path, always exits 0 and prints nothing, so it
cannot block or steer the agent.

## Consequences

- Invariant 4 now reads: the panel is read-only toward agents; only
  `setup`, run by the user, writes agent-pinball's hook entries.
- A malformed agent config is reported instead of repaired.
- Agents with hooks installed call the binary on every hooked event, so the
  installed path must stay valid; `setup` detects a moved binary as
  `outdated` and rewrites the entry.
- Any further write into agent files needs its own ADR.
