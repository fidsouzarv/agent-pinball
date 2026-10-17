# 5. In-memory state first, SQLite history later

Status: Accepted

## Context

The panel's main job is the live board: which sessions exist now and how they
are doing. Every number can be rebuilt from the agents' own files on startup.
History (trends across restarts, past sessions) is useful but not needed for
the first release, and a database adds schema, migrations and a writer to
reason about.

## Decision

`pinball-store` starts as an in-memory `Board`: live sessions keyed by
`SessionId`, a fixed-size ring buffer of samples per session for sparklines,
and a cap on the number of sessions. On startup the collectors rebuild it from
the agent files. A SQLite history with a single writer task is added in a
later milestone (M6), fed from the same deltas, holding only sanitized values.

## Consequences

- No schema or migrations until history is actually built.
- Sparklines reset on restart until M6.
- Startup cost grows with the size of recent agent files; the collectors'
  read caps bound it.
- When history arrives it stores `Sanitized` values only, so the panel's data
  volume never holds raw agent text.
