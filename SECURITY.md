# Security Policy

## Reporting a vulnerability

Do **not** open a public issue. Use a GitHub private security advisory on this
repository. You will get a response within 7 days; a confirmed issue gets a
patch within 30 days and credit in the changelog unless you prefer otherwise.

## Threat model

agent-pinball is a **single-user workstation panel**. It reads the local state
of Codex, Cursor and Claude agents and shows it. It has no authentication yet.

### In scope

- **Network exposure.** The server binds `127.0.0.1:4242` by default and
  refuses to start on a non-loopback address. Inside a container a wildcard
  bind is allowed (`AGENT_PINBALL_IN_CONTAINER=1`) because publishing a port
  requires it; reachability is then decided by the host's `-p` spec, which
  must be `-p 127.0.0.1:4242:4242`.
- **DNS rebinding.** Every request's `Host` header is checked against
  `AGENT_PINBALL_ALLOWED_HOSTS` (default `localhost,127.0.0.1,::1`) before
  routing; anything else gets 403, and a request without `Host` gets 400.
- **Resource exhaustion.** Request heads are capped at 8 KB (431 above that),
  and reads and writes time out after 5 seconds.
- **Secrets in agent logs.** Agent transcripts routinely contain API keys and
  private code. Text read from them must pass the sanitizer boundary before it
  is stored or rendered (STANDARDS.md §5).
- **Agent integrity.** The panel never writes to agent directories, spawns or
  kills agents. `crates/pinball-web/clippy.toml` enforces this, and the Docker
  setup mounts agent directories read-only.

### Out of scope

- Other local users or processes with access to your account: they can read
  the same agent files the panel reads.
- Exposing the panel beyond loopback. That needs authentication and TLS, which
  do not exist yet.

## Supply chain

- `unsafe_code` is forbidden workspace-wide.
- `cargo deny` (licenses, sources, advisories) and `cargo audit` run in CI.
- Every GitHub Action is pinned to a commit SHA.
- gitleaks scans every push and PR, plus the full history weekly.
