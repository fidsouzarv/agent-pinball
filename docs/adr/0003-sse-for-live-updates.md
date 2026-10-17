# 3. Server-Sent Events for live updates

Status: Accepted

## Context

The board changes every second or so while agents run. The browser only needs
to receive; it never sends anything back (ADR 0001). Options were polling
`/api/board`, WebSockets, or Server-Sent Events.

## Decision

`GET /events` streams Server-Sent Events: a full `board` snapshot on connect,
`delta` when one session changes, and `event` for compaction, errors and
endings. The board publishes on a bounded `tokio::sync::broadcast` channel; a
client that lags gets a fresh `board` snapshot instead of a growing queue.
`EventSource` reconnects on its own, and every reconnect starts with a
snapshot, so no replay log is needed.

## Consequences

- One-way, plain HTTP: the `Host` allowlist, CSP `connect-src 'self'` and
  existing timeouts apply unchanged. No upgrade handshake to secure.
- Memory per client is bounded by the channel size.
- Long-lived connections need the server to be concurrent, one reason
  `pinball-web` moves to axum + tokio.
- If the panel ever needs to send commands, SSE is not enough, and that is
  intended (ADR 0001).
