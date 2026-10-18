# 7. Waiting status from awaiting-input hooks

Status: Accepted. Amends [ADR 0006](0006-setup-writes-agent-hooks-from-the-cli-only.md)
(one more hooked event per agent, same rules).

## Context

The most useful thing the panel can tell a user is that an agent is blocked
on them: a permission prompt, a question, or an idle prompt after a finished
turn. The status model had no such state. A session parked on a permission
prompt has an open turn and writes nothing, so it read as `running` and, after
`STUCK_AFTER`, as `stuck`.

The agents' files do not say when a prompt is pending. Codex lists approval
requests among the events it never persists to a rollout. Claude's
transcript shows the `tool_use` before the prompt and the `tool_result` after
it, but nothing in between. The signal only exists in hooks:

- Claude Code: `PermissionRequest` (before the permission dialog) and
  `Notification` with `notification_type` `permission_prompt`, `idle_prompt`,
  `elicitation_dialog`, `elicitation_url_dialog` or `agent_needs_input`.
- Codex: `PermissionRequest`. It has no `Notification` event.
- Cursor: no documented event.

Two of these hooks can steer the agent. A `PermissionRequest` hook that
prints an `allow` or `deny` decision answers the prompt itself. Payloads also
carry text that must not be stored: `message` and `title` on `Notification`,
`tool_input` (commands, file contents) on `PermissionRequest`.

## Decision

- **New hooked event.** `setup` installs one more agent-pinball entry per agent
  where an equivalent exists: `PermissionRequest` and `Notification` for Claude
  Code, `PermissionRequest` for Codex, nothing for Cursor. The rules of ADR 0006
  apply unchanged: own entries only, appended after the user's, shown as a
  diff, confirmed, backed up.
- **What the hook keeps.** The receiver records the event as `awaiting_input`
  with a `reason` enum: `approval`, `question` or `idle_prompt`. It reads the
  session id, the working directory's last component and `notification_type`.
  It never reads `message`, `title` or `tool_input`. Unknown types (for example
  `auth_success`) record nothing.
- **It never decides.** The receiver prints nothing and exits 0 on every event,
  so a `PermissionRequest` prompt is always shown to the user. A test asserts
  empty stdout.
- **Prompt-carrying sources stay out.** `UserPromptSubmit`,
  `beforeSubmitPrompt`, Claude's `ElicitationResult` and Codex's legacy
  `notify` program (whose payload has `input-messages`) are never hooked.
- **Status.** A new status `waiting` sits after `ended` and `error`, and before
  `stuck`, in the first-match table (SYSTEM_DESIGN §6). A session is waiting
  when its newest awaiting-input record is newer than any other activity:
  another hook event, or a transcript or rollout record. Collectors set facts
  (`awaiting { since, reason }`, `last_activity`) from record timestamps, and
  `compute_status` stays pure. Waiting clears on the next activity. That also
  covers a prompt the user denies by hand, which fires no hook but writes a
  `tool_result`.
- **Cursor.** Cursor has no hook, so waiting is not detected for it in v1. A
  heuristic over undocumented `state.vscdb` fields may follow behind a flag
  that is off by default, once it is checked against a real pending prompt.
- **Sound.** The browser plays a short chime synthesized with Web Audio when a
  session enters `waiting` for `approval` or `question`. Sound is off until the
  user turns it on in the coin door; that click is the user gesture the
  browser's autoplay policy needs. `idle_prompt` shows as waiting but is silent
  by default, because Claude sends it about a minute after every finished
  turn. Nothing is played server-side and no audio file ships, so the CSP gains
  no `media-src`.

## Consequences

- Users re-run `setup` to get the new entries. An install from before this ADR
  reports `outdated`.
- Codex requires the user to trust the new entry with `/hooks`, as before.
- Each permission prompt runs the hook binary once more. The receiver is
  bounded (64 KiB, 2 s) and returns at once.
- The event stream must stay connected while the tab is hidden, or a
  background tab could never chime. Only animations pause.
- `idle` stays reachable for agents without an idle notification, and for
  Claude in the minute between the end of a turn and `idle_prompt`.
