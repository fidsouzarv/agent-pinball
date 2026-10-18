# agent-pinball engineering standards

agent-pinball is a pinball-themed panel that monitors Codex, Cursor and Claude coding agents, written in Rust (1.95, edition 2024).
This document sets the project's standards: toolchain, lint, tests, complexity, security and Docker. [AGENTS.md](AGENTS.md) is the short version for whoever is coding.

---

## 1. Toolchain and workspace

- `rust-toolchain.toml` pins the version (`channel = "1.95"`, `components = ["rustfmt", "clippy"]`, `profile = "minimal"`). rustup selects it automatically.
- A Cargo workspace with `resolver = "3"`. `[workspace.package]` centralizes `version`, `edition`, `rust-version`, `license` and `publish = false`.
- Every dependency is declared in `[workspace.dependencies]`, and each crate uses `dep = { workspace = true }`, so versions live in one place. Internal path dependencies also declare a `version`, because cargo-deny counts a path dependency without one as a wildcard.
- One crate per responsibility. The `cli` crate stays thin: it parses arguments, loads config once, calls typed library functions and renders output.
- A crate is added when it has real code. No empty crates or stubs.
- Profiles:
  - `release`: `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`.
  - `dev`: `debug = "line-tables-only"`, which shrinks test binaries and link time.
  - `dev.package."*"`: `opt-level = 1`, so dependencies compile optimized and tests run faster.
  - `dev.build-override`: `opt-level = 3`, so proc macros and build scripts run faster.
- The core is Rust-only: the server and the UI build with Cargo alone, with no Node or bundler. The UI is server-rendered, and a page may carry a small vanilla script.

Current crates: `pinball-core`, `pinball-setup`, `pinball-web` and `pinball-cli`. Target layout:

```
crates/
  pinball-core/       # domain types: Agent, AgentKind {Codex, Cursor, Claude}, Session, Status
  pinball-collectors/ # reads each agent's logs/state (one module per provider)
  pinball-store/      # persistence (SQLite, single writer)
  pinball-setup/      # `setup`: agent detection, hook entries, hook spool
  pinball-web/        # the panel (server-rendered, pinball theme)
  pinball-cli/        # thin binary
  pinball-test-support/
```

---

## 2. Lint

**Workspace lints** (root `Cargo.toml`). Every crate inherits them with `[lints] workspace = true`:

```toml
[workspace.lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
unwrap_used = "warn"
expect_used = "warn"
dbg_macro = "warn"
todo = "warn"
```

Test modules opt back in with `#[allow(clippy::unwrap_used, clippy::expect_used)]`.

**Warnings are errors in CI:**
- `RUSTFLAGS: "-D warnings"` in the workflow's global env.
- `cargo clippy --workspace --all-targets -- -D warnings`. `--all-targets` includes tests, so they are linted too.
- `cargo fmt --all -- --check`, with default rustfmt (no `rustfmt.toml`).
- `git diff --check` catches stray whitespace.

**Architecture rules written as lints** (a per-crate `clippy.toml` with `disallowed-methods`):
- `crates/pinball-web/clippy.toml` forbids `std::process::Command::new` and the writing functions of `std::fs`, because the panel only reads.
- Every entry has a `reason`, so the lint's error message names the right path.
- Clippy reads only the nearest `clippy.toml`, so a crate with its own file repeats the root complexity limits.

**Style:**
- A comment explains *why*, never restates *what*. Code comments and PR text are in English.
- Small, scoped changes: no opportunistic refactors, no speculative abstractions.
- No dead code and no half-built public API.
- `unwrap`, `expect` and `unreachable!` only in tests. Runtime paths use an explicit fallback.
- Typed boundaries: IDs, paths and kinds are parsed and normalized once, then reused.
- No global singletons. Dependencies are passed explicitly.

---

## 3. Tests

**Runner: cargo-nextest**, with aliases in `.cargo/config.toml`:

```toml
[alias]
t  = "nextest run"                          # everyday: skips the slow tier
tf = "nextest run --workspace -P full"      # gate: runs everything
```

**Tiers by name** (`.config/nextest.toml`):
- A test in a module whose name starts with `slow` or `stress` (`foo::slow::*`) is left out of `cargo t`.
- Budget: about 1s per test. The default profile flags any test over 5s as slow and kills it after 2 minutes.
- `fail-fast = false`: one run shows every failure.
- The `ci` profile: `default-filter = "all()"`, **`retries = 0`** (a retry hides a flaky test), a 60s slow-timeout and a 10-minute kill.
- Before moving a test to the slow tier, try to make it fast: an injectable timeout or deadline, a smaller fixture.

**Layout:**
- Unit tests live in `#[cfg(test)] mod tests` in the same file.
- Integration tests live in `crates/<crate>/tests/suite/*.rs`, declared in `tests/suite/mod.rs`, which `lib.rs` includes under `#[cfg(test)]`. **One test binary per crate**, because each binary costs a link and, on macOS, a malware scan.
- `rstest` for parameterized tests and `tempfile` for the filesystem.
- Doctests run separately with `cargo test --workspace --doc`, because `--all-targets` does not include them.

**Content rules:**
- Every bug fix and behavior change comes with a focused regression test.
- Filesystem tests use a temp dir or an injected root, never the real home directory.
- **Security-boundary tests are adversarial and mandatory.** They attempt the violation, assert it is refused, and include a legitimate control case. The test must *bite*: it fails with the guard removed.
- When a default changes, list every consumer and test each one under the new default.
- Third-party formats (each agent's files and logs) are modeled from the format's docs or source, with a link in the code and a test for each case.
- Each collector (Codex, Cursor, Claude) is tested against **real, anonymized fixtures** in `crates/pinball-collectors/fixtures/<provider>/`.
- Log parsers get malformed and truncated input in tests. They must never panic.
- The status computation (running, idle, stuck, error) is tested table-driven with `rstest`.
- Clock and filesystem are injected; no direct `SystemTime::now()` in logic.

**Pre-push hook** (`scripts/install-git-hooks.sh`): runs fmt, clippy and `cargo tf` before every push and only touches its own marked block in `.git/hooks/pre-push`. To skip it: `git push --no-verify`.

**CI:** tests on Linux and macOS on every PR, plus a separate `--release` build, because some failures only show up with LTO.

---

## 4. Cyclomatic quality / complexity

The limits are explicit and enforced by clippy:

```toml
# Cargo.toml (workspace)
[workspace.lints.clippy]
cognitive_complexity = "warn"
too_many_lines = "warn"
too_many_arguments = "warn"
type_complexity = "warn"
```

```toml
# clippy.toml (root)
cognitive-complexity-threshold = 15
too-many-lines-threshold = 80
too-many-arguments-threshold = 5
type-complexity-threshold = 250
```

With `-D warnings` in CI, crossing any of these limits fails the build.

Rules:
- An `#[allow(...)]` for complexity needs a comment saying *why*, scoped to the function, never the module. More than 5 arguments means a parameter struct or a builder is missing.
- Coverage: `cargo llvm-cov nextest` runs in CI as a report, with no gate at first. A 70% gate on `pinball-core` and `pinball-collectors` comes once the code settles.
- Optional once the core settles: `cargo mutants` on the collectors' parsers.

---

## 5. Security

**Code:**
- `unsafe_code = "forbid"` across the workspace.
- **Agent logs contain secrets and user code.** Every piece of text read from Codex, Cursor or Claude goes through a typed sanitizer (`Sanitized<T>`, built only by `sanitize()`) before it is stored or rendered. Nothing raw reaches the panel.
- The panel is **read-only** toward agents: it does not write their files, send them commands or kill their processes. `disallowed-methods` in the web crate enforces it, and changing that needs auth and confirmation designed first. The only exception is `agent-pinball setup`, run by the user in a terminal, which adds or removes agent-pinball's own hook entries after a diff and confirmation (ADR 0006).
- Agent paths (`~/.codex`, `~/.cursor`, `~/.claude`) come from config with an injectable root, and collectors only read them.
- Config is read once (`Config::from_lookup`) and passed down. Nothing else reads the environment.
- I/O is always bounded: the HTTP request head is capped at 8 KB (431 above that), and each connection has a 10s total deadline to send its request, on top of write timeouts.
- Templates escape by default, and the panel gets a strict `Content-Security-Policy` once it has pages.

**Dependencies:**
- `cargo deny check --all-features` with `deny.toml`:
  - License allowlist (MIT, Apache-2.0, BSD, ISC, MPL-2.0, Zlib, Unicode-3.0, CC0). GPL and AGPL are rejected.
  - `wildcards = "deny"`, `unknown-registry = "deny"`, `unknown-git = "deny"`, git only with a pinned `rev`.
  - An ignored advisory needs a comment justifying it.
- `cargo audit` in CI. An `--ignore` needs the advisory id and a written reason.
- `--locked` in CI and Docker builds.
- Before adding a dependency, check whether the project already has the capability.
- Monthly Dependabot for GitHub Actions and Cargo.

**CI supply chain:**
- **Every action is pinned to a commit SHA**, with the version in a comment (`actions/checkout@3d3c42e... # v7.0.1`).
- `permissions: contents: read` at the top of the workflow. Extra scopes only per job.
- `concurrency` cancels stale PR runs, but never runs on `main`.

**Secrets:**
- gitleaks on every push and PR, plus a weekly full-history scan (`secret-scan.yml`).
- `.gitleaks.toml` extends the default rules (`useDefault = true`). The allowlist accepts **only specific regexes** for obviously fake fixtures (the `FAKE` marker, as in `sk-ant-FAKE...`), with no path exclusions.

**Process:** `SECURITY.md` with a private channel (GitHub Security Advisory), a response SLA (7 days) and a patch SLA (30 days), and an explicit threat model of what is and is not in scope.

---

## 6. Docker and localhost

**Default network posture: loopback.**
- The default bind is `127.0.0.1:4242`, held in one constant (`DEFAULT_BIND` in `pinball-cli/src/config.rs`).
- A `Host` header allowlist (`AGENT_PINBALL_ALLOWED_HOSTS`, default `localhost,127.0.0.1,::1`) guards against DNS rebinding and runs before routing: a `Host` outside the list gets 403, and a request with no `Host` gets 400.
- **Fail-closed:** the server refuses to start on a non-loopback address outside a container. There is no authentication yet, so there is no override.
- Leaving loopback will require auth first, with TLS handled by a reverse proxy.

**Multi-stage Dockerfile** (`docker/Dockerfile`):
- `builder`: `rust:1.95-slim-bookworm`, `cargo build --locked --release --bin agent-pinball`, with BuildKit caches for the registry and target dir (`--mount=type=cache`).
- `runtime`: `debian:bookworm-slim` with only `ca-certificates`, `--no-install-recommends`, and `/var/lib/apt/lists/*` removed.
- **Non-root user** (`uid 1000`, `nologin` shell) owning `/data`.
- A `HEALTHCHECK` that uses the binary itself (`agent-pinball status`).
- `ENV AGENT_PINBALL_IN_CONTAINER=1`: inside the container the bind is `0.0.0.0` (the namespace requires it) and the guard warns instead of refusing. Real exposure is decided by the host's `-p`, which must stay on `127.0.0.1`.
- `.dockerignore` excludes `target/` and `.git/` and, **last**, the `.env` files, so no later negation can re-include them.

**docker-compose.yml:**
- `ports: "127.0.0.1:4242:4242"`, so it **publishes on the host's loopback only**.
- `user: "1000:1000"`, a named volume for data, `restart: unless-stopped`, a healthcheck.
- Agent directories (`~/.codex`, `~/.cursor`, `~/.claude`) are mounted **`:ro`**, each one added with its collector. The panel's own data lives in a separate named volume.
- Secrets live in `.env` (gitignored, `required: false`).

**CI:** the `docker-smoke` job builds the image and checks `--version`, that the user is not root, and that the healthcheck turns healthy.

---

## 7. Local gate before opening a PR

These are the same commands CI runs:

```bash
cargo fmt --all -- --check
git diff --check
cargo clippy --workspace --all-targets -- -D warnings
cargo tf
cargo test --workspace --doc
cargo deny check --all-features
cargo audit
```

## 8. Where each standard lives

| File | Contents |
|---|---|
| `rust-toolchain.toml` | pinned Rust version |
| `Cargo.toml` | workspace, lints, profiles |
| `clippy.toml` | complexity limits |
| `crates/pinball-web/clippy.toml` | `disallowed-methods` (read-only panel) + limits |
| `.cargo/config.toml` | `t` / `tf` aliases |
| `.config/nextest.toml` | `default` / `full` / `ci` profiles |
| `deny.toml` | dependency policy |
| `.gitleaks.toml` | secret scanning |
| `.github/workflows/ci.yml` | fmt, clippy, test, doc, coverage, deny, audit, gitleaks, release-build, docker-smoke |
| `.github/workflows/secret-scan.yml` | weekly full-history gitleaks |
| `.github/dependabot.yml` | Actions and Cargo updates |
| `scripts/install-git-hooks.sh` | pre-push with fmt, clippy and `cargo tf` |
| `docker/Dockerfile`, `docker/docker-compose.yml`, `.dockerignore` | section 6 |
| `SECURITY.md` | threat model |
| `AGENTS.md` + `CLAUDE.md` (`@AGENTS.md`) | rules for the agents coding the project |
