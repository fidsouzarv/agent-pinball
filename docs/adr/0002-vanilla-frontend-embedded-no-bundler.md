# 2. Vanilla frontend embedded in the binary, no bundler

Status: Accepted

## Context

The panel is one page with a pinball playfield, gauges and a detail view.
STANDARDS.md §1 requires the server and UI to build with Cargo alone, with no
Node toolchain. A JS framework would add a second dependency tree to audit, a
build step to CI and Docker, and inline-script habits that fight a strict CSP.

## Decision

The UI is server-rendered HTML with escaped-by-default templates, plus
hand-written CSS, SVG and a small amount of vanilla JavaScript for live
updates. Assets are embedded with `include_str!`/`include_bytes!`, served from
`/assets/*` under content-hashed names with immutable caching. No inline
scripts or styles: dynamic values are set through CSSOM `setProperty`, so the
CSP can be `script-src 'self'; style-src 'self'`.

## Consequences

- One binary, one `cargo build`; nothing to install for the frontend.
- The page works without JavaScript, minus live updates.
- No component framework: state handling in JS stays deliberately small,
  driven by server events.
- Animations and layout are limited to what CSS and SVG do well, which suits
  a pinball theme.
