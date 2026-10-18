# 8. Action links and absolute project paths on the loopback panel

Status: Accepted. Keeps [ADR 0001](0001-monitor-only-read-only-panel.md):
the panel still sends nothing to agents.

## Context

When a session is waiting for the user ([ADR 0007](0007-waiting-status-from-awaiting-input-hooks.md)),
the user has to get to it. ADR 0001 already says the answer is the agent's own
UI. The panel can shorten that trip without acting on the agent:

- **Editor:** `vscode://file/<abs path>` or `cursor://file/<abs path>` opens the
  project folder.
- **Resume command:** `claude --resume <id>` or `codex resume <id>`, run from
  the project directory. Claude scopes resume to the directory, so the command
  needs the path too.
- **Desktop app:** `codex://threads/<id>` opens a Codex app thread. Claude's
  desktop app has no URL for an existing Code session, and Cursor has no deep
  link to a composer.

The editor link and the resume command both need the absolute working
directory. Until now the panel showed only its last component (SYSTEM_DESIGN
§9), because the full path discloses the username and directory layout.

## Decision

- **Links are navigation and clipboard only.** A link is an `<a href>` the
  browser hands to the OS. The resume command is copied to the clipboard for
  the user to run. No route accepts input and nothing is executed by the panel,
  so ADR 0001 stands unchanged.
- **The server builds every link.** Each session on the wire carries
  `links { editor | null, resume | null, app | null }`. The client never builds
  a URL or command from parts. It re-checks every `href` against a fixed scheme
  allowlist (`vscode://file/`, `cursor://file/`, `codex://threads/`) and drops
  anything else.
- **One typed carrier for the path.** `ProjectDir` in `pinball-core` is built
  only by a parser. The parser accepts an absolute path of at most 1024 bytes,
  with no `..` component and no control or bidi characters. It does not check
  that the path exists, since in files-only mode it is a host path. A rejected
  path makes `editor` and `resume` null; `project` still shows the sanitized
  directory name.
- **Encoding.** In `href`s the path is percent-encoded segment by segment. In
  commands it is shell-quoted with the same `quote()` that `setup` uses for
  hook commands. The session id must match the id rule the hook spool already
  enforces.
- **Where the path appears.** Only inside `links.*`. There is no separate path
  field, and argv is still never read.
- **Editor choice.** Cursor sessions link to `cursor://`. Claude and Codex
  sessions use `AGENT_PINBALL_EDITOR` (`vscode`, `cursor` or `none`, default
  `vscode`).

## Consequences

- The absolute project path is readable by anything that can read the panel.
  Today that is the local user only: a loopback bind, the Host allowlist and
  no CORS headers. Any change that lets the panel leave loopback has to revisit
  this ADR, together with the authentication it would need anyway.
- Screen shares of the panel can show paths inside link targets. The board
  itself shows only directory names.
- `codex://threads/<id>` is documented for app threads. Whether CLI threads
  open is not, so the link is labelled "Open in Codex app" and the resume
  command stays available.
- A new link kind needs a new scheme in the allowlist on both server and
  client, plus a test.
