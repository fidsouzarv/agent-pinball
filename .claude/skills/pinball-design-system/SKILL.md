---
name: pinball-design-system
description: Build or change any agent-pinball web page (crates/pinball-web) with the project's pinball design system - tokens, components, CSP-safe markup and the browser QA loop. Use when adding a page, a component, a style or a script to pinball-web, porting from docs/design/prototype, or reviewing UI changes. Not for Rust-only changes with no rendered output.
---

# agent-pinball design system

The design system already exists. Do not invent colours, fonts or components.

- Spec: `docs/design/COMPONENTS.md` (metaphor map, tokens, every component, states, motion, accessibility).
- Source of truth for styles: `docs/design/prototype/pinball.css`. A byte-identical copy ships at `crates/pinball-web/assets/pinball.css`; the test `pinball_css_matches_the_design_prototype` fails if they drift. Change the prototype first, then copy.
- Live references: open `docs/design/prototype/index.html` (full table) and `catalog.html` (every component in every state).

Read the parts of COMPONENTS.md you touch before writing markup.

## Rules

1. **Tokens only.** Colours, fonts, radii and easing come from `:root` in pinball.css (`--cab-*`, `--ink*`, `--dmd-*`, `--claude`/`--codex`/`--cursor` and their `-2`, `--st-*`, `--lv-*`, `--font-display`/`--font-body`/`--font-dmd`). No hex values in page CSS except inside new tokens added to the prototype.
2. **Client colour through `[data-kind]`.** Components read `--p`/`--p2`; set `data-kind="claude|codex|cursor"` on the element instead of colouring it.
3. **Status never by colour alone.** Use `.insert` lamps (shape + label). The panel uses `data-status` (running, idle, stuck, error, ended); the setup page uses `data-check` (ok, todo, waiting, error, absent) defined in `assets/setup.css`.
4. **Strict CSP** (`default-src 'none'; script-src 'self'; style-src 'self'`, sent on every response): no inline `<script>`, no `style=""`, no `on*` attributes. Runtime values go through `el.style.setProperty('--x', v)`. Classic scripts with `defer`.
5. **Untrusted text through `textContent`** in JS, and through maud's escaping on the server (never `PreEscaped` for report or agent data). `innerHTML` only for constant markup.
6. **Server-rendered first.** The page must read correctly without JS; scripts only fill decoration (`[data-bulbs]`, `.chevrons`) and copy server values into `data-slot` elements. Keep view logic in Rust (one view model for HTML and JSON), not in JS.
7. **Read-only.** Every route is `GET`; never add a form or a write endpoint (ADR 0001, ADR 0006).
8. **Accessibility and motion:** works at 375 px with no horizontal scroll; `prefers-reduced-motion` keeps every state readable; status changes go to `#announcer` (polite, at most one per 4 s).

## Where things go

| What | Where |
|---|---|
| Page markup | `crates/pinball-web/src/<page>_page.rs` (maud `html!`) |
| Page-only layout | `crates/pinball-web/assets/<page>.css`, built from tokens and existing components |
| Page script | `crates/pinball-web/assets/<page>.js`, embedded with `include_str!` and served from `/assets/` |
| Routes | `resource()` in `crates/pinball-web/src/lib.rs`, after the Host allowlist |

Reuse the existing structure: `header.backbox` (marquee + `.dmd`), `section.table > .playfield > .lanes > section.lane[data-kind]`, `footer.apron > .card`, `footer.coin-door` (LIVE / INSERT COIN for the data link).

## Verify

1. `cargo t -p pinball-web` (escaping, CSP and Host tests must stay adversarial).
2. Run the page against a fake home, never the real one, for example:

   ```bash
   H=$(mktemp -d); mkdir -p $H/.claude $H/.codex $H/.cursor
   env -i HOME=$H PATH=/usr/bin:/bin target/debug/agent-pinball setup --web --bind 127.0.0.1:4243
   ```

3. Browser QA with `/fullstack-pr-qa` on `http://127.0.0.1:4243/setup`: desktop and 375 px, reduced motion, live update after the terminal step, response headers (CSP, `nosniff`), foreign `Host` gets 403.
