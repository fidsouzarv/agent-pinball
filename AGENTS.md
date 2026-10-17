# AGENTS.md — agent-pinball contributor guide

agent-pinball is a pinball-themed panel that monitors Codex, Cursor and
Claude coding agents. Rust 1.95, edition 2024, pinned in `rust-toolchain.toml`.

The full standards are in [STANDARDS.md](STANDARDS.md). This file is the
short version an agent needs while working.

## Layout

```
crates/pinball-core  domain types (AgentKind, ...)
crates/pinball-web   HTTP surface; read-only by design (see its clippy.toml)
crates/pinball-cli   the `agent-pinball` binary: parse args, load config, call libraries
```

Planned crates (`pinball-collectors`, `pinball-store`, `pinball-test-support`)
are added when they have real code. No empty crates or stubs.

## Commands

```bash
# Everyday loop (cargo install cargo-nextest --locked)
cargo t                         # skips slow*/stress* modules
cargo t -p pinball-web          # one crate

# Before saying a change is ready (CI runs the same)
cargo fmt --all -- --check
git diff --check
cargo clippy --workspace --all-targets -- -D warnings
cargo tf                        # every test, slow tier included
cargo test --workspace --doc
cargo deny check                # if installed
cargo audit                     # if installed
```

Install the pre-push hook once per clone: `scripts/install-git-hooks.sh`.

## Code style

- Match the surrounding file. Comments explain *why*, never restate *what*.
- Small, scoped changes. No speculative abstractions, no dead code, no public
  surface without a caller.
- No `unwrap`/`expect`/`unreachable!` in runtime paths (lint-enforced); tests
  opt in with `#[allow(clippy::unwrap_used, clippy::expect_used)]`.
- `unsafe` is forbidden.
- Complexity limits are in `clippy.toml` (cognitive 15, 80 lines, 5 args).
  An `allow` is scoped to one function and carries a comment saying why.
- Parse once at the boundary into typed values (`AgentKind`, `SocketAddr`).
- Config is read once (`Config::from_lookup`) and passed down. Nothing else
  reads the environment; injectable lookups keep tests off the real env.
- No global singletons.

## Invariants

1. **Loopback by default, fail-closed.** `serve` refuses a non-loopback bind
   outside a container. Do not weaken this before authentication exists.
2. **Host allowlist before routing** on every request (DNS-rebinding guard).
3. **Bounded I/O.** Every read has a size cap and a timeout.
4. **The panel is read-only toward agents.** It never writes their files,
   spawns or kills them. `crates/pinball-web/clippy.toml` enforces it.
5. **Agent log text is untrusted.** It contains secrets; it goes through a
   sanitizer before being stored or rendered.
6. **Collectors read from injected roots**, never a hard-coded home directory.

## Tests

- Every bug fix and behavior change gets a focused regression test.
- Integration tests live in `crates/<crate>/tests/suite/` and are declared in
  its `mod.rs`, which `lib.rs` includes under `#[cfg(test)]`: one test binary
  per crate.
- A test that needs more than ~1s goes in a `slow`/`stress` module, after you
  try to make it fast.
- **Security-boundary tests are adversarial.** Attempt the violation, assert
  refusal, keep a legitimate control case. The test must fail with the guard
  removed.
- Collectors are tested against anonymized fixtures of each agent's real
  files, linked to the source of the format. Malformed input must not panic.
- Filesystem tests use `tempfile` or injected roots, never the real home.
- Fake secrets in fixtures use the `FAKE` marker (`sk-ant-FAKE...`) so
  gitleaks accepts them (`.gitleaks.toml`).

## Dependencies

Check the workspace does not already cover the need before adding a crate.
Declare versions in `[workspace.dependencies]` only. `cargo deny` rejects
copyleft licenses, unknown registries and wildcard versions.

## Docker

`docker/Dockerfile` (multi-stage, non-root, healthcheck) and
`docker/docker-compose.yml` (published on `127.0.0.1` only, agent directories
mounted `:ro`). See STANDARDS.md §6.
