# 1. Monitor only: the panel is read-only toward agents

Status: Accepted

## Context

agent-pinball watches Codex, Cursor and Claude sessions. Being able to stop a
stuck agent or answer a waiting one from the panel is tempting. But the panel
has no authentication, any local page could try to reach it, and agent
directories hold credentials and private code. A panel that can write to an
agent's files or signal its process turns any web bug into control over the
user's coding agents.

## Decision

The panel only observes. It never writes to agent files, never spawns, signals
or kills agent processes, and never sends them input. The HTTP surface is
`GET` only. SQLite databases are opened read-only. The rule is enforced by
`disallowed-methods` in `crates/pinball-web/clippy.toml`, extended to every
crate that touches agent data, and by `:ro` mounts in Docker.

## Consequences

- A whole class of vulnerabilities (CSRF, command injection, file tampering)
  has nothing to act on.
- Users still switch to the agent's own UI to act on what they see.
- Adding any control feature needs authentication and confirmation designed
  first and a new ADR superseding this one, not a lint `allow`.
