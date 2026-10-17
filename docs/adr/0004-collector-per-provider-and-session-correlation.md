# 4. One collector per provider, sessions correlated to processes without argv

Status: Accepted

## Context

Codex, Cursor and Claude store sessions in unrelated, undocumented formats:
JSONL rollouts, JSONL transcripts plus pid files, and a multi-GB SQLite
database in WAL mode. They also count tokens differently (Codex
`input_tokens` includes cached tokens, Claude's does not; Cursor keeps no
per-message counts locally). Linking a session to a live process is needed to
tell "idle" from "ended", but process command lines can hold secrets
(`cursor-agent --api-key`).

## Decision

- `pinball-collectors` has one module per provider (`claude`, `codex`,
  `cursor`) behind a common output, `SessionDelta`, and a shared `discover`
  module for the process scan.
- Each collector normalizes into `pinball-core` types (`TokenUsage` with
  cache traffic split out, `ContextGauge` with an optional limit) so nothing
  downstream knows provider quirks.
- Roots are injected from config. Files are tailed from stored offsets with
  line and pass size caps; malformed input is skipped, never a panic.
- Correlation uses only executable name, working directory and start time.
  Claude links through its pid file (checked against process start time);
  Codex and Cursor match by working directory and most recent write. argv is
  never read, stored or rendered.

## Consequences

- A format change in one agent breaks one module, with fixtures pinning each
  known shape.
- Codex and Cursor correlation is heuristic: two sessions in the same
  directory may be attributed to the wrong host process. Status still comes
  from the session's own records, so the error is limited to liveness.
- Cursor coins are shown as unavailable until a local source exists.
- In Docker there is no process table; the collectors run on files alone.
