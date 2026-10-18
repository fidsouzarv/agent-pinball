# Component catalog

The agent-pinball dashboard is a pinball table. Each agent session is a ball
in play; the data it shows (context used, coins spent, status) is drawn with
the parts of a real machine. This document is the spec. The working reference
is the prototype:

- [`prototype/index.html`](prototype/index.html): the full table, driven by a
  simulated event stream.
- [`prototype/catalog.html`](prototype/catalog.html): every component in every
  state, with buttons to trigger each animation.

Opening the HTML file directly works in a normal browser. Some embedded
browser panes snapshot local files without running scripts; for those, serve
the folder from the repo root:

```bash
python3 -m http.server 4290 --bind 127.0.0.1 --directory docs/design/prototype
```

The visual language takes cues from three reference tables: the warm
red/yellow energy of a Pokémon playfield, the cosmic violet/orange of Rush,
and the cyan/blue portals of Portal. Each one becomes a client colour.

## Metaphor map

| Domain | On the table |
|---|---|
| All agents on this machine | The cabinet: backbox on top, playfield below |
| A session | A ball in play, drawn as a pop bumper card |
| Several live sessions | Multiball |
| Subagent | Extra ball, shown next to its parent |
| Coins (tokens spent) | Score: EM reels per session, DMD total in the backbox |
| Context used | The bumper ring and the context ramp filling toward TILT |
| Context at 80% or more | TILT WARNING insert |
| Compaction | BALL SAVED |
| New session | Plunger launch, card drops onto the table |
| Token burst | Pop-bumper hit with a score popup |
| Finished turn | A drop target falls |
| Waiting for the user (approval, question, idle prompt) | Ball held in the kickout hole: YOUR SHOT |
| Idle | Ball resting in the saucer |
| Stuck | Ball trapped, rattling |
| Error | TILT |
| Ended | GAME OVER |

Coins are raw tokens: `input + output + cache read + cache write`. No price
table. Every turn re-sends the whole context, so coins per turn are roughly
the context size, and total coins grow much faster than context does.

## Design tokens

Defined on `:root` in `pinball.css`. The page is always dark: it is a cabinet.

| Group | Tokens | Use |
|---|---|---|
| Cabinet | `--cab-void`, `--cab-black`, `--cab-panel`, `--cab-panel-2`, `--cab-line`, `--ink`, `--ink-dim`, `--ink-faint` | Surfaces and text |
| Chrome | `--chrome-hi`, `--chrome-mid`, `--chrome-lo` | Rails, screws, wireforms |
| DMD | `--dmd-on`, `--dmd-hot`, `--dmd-off`, `--dmd-bg` | Dot-matrix display, marquee bulbs |
| Claude | `--claude` `#ff6b3d`, `--claude-2` `#ffc23d` | Lane, caps, targets, banners |
| Codex | `--codex` `#9b6bff`, `--codex-2` `#ff7a1a` | Same |
| Cursor | `--cursor` `#2ad4f0`, `--cursor-2` `#2f6bff` | Same |
| Status | `--st-running`, `--st-waiting`, `--st-idle`, `--st-stuck`, `--st-error`, `--st-ended` | Insert lights, the waiting frame |
| Context level | `--lv-warm` (60%), `--lv-hot` (80%), `--lv-crit` (95%) | Ring and ramp colour |

Type: `--font-display` (Impact-style condensed, for names and banners),
`--font-body` (system sans), `--font-dmd` (monospace under a dot mask). No web
fonts: the dashboard must work offline and under a strict CSP.

Components read the client colour from two local properties, `--p` and
`--p2`, which `[data-kind]` sets. Context colour comes from `--lv`, set by
`[data-level]`.

## Components

Each entry lists its markup hook, the data it binds, its states and its
motion. "Reduced motion" is what remains under `prefers-reduced-motion`.

### 1. Backbox

`header.backbox`. Marquee with chasing bulbs (`[data-bulbs]` filled by JS),
chrome title, the DMD, and one score window per client (`.player`) plus balls
in play.

- **Data:** total coins, coins per client (`N/A` when a client records none),
  count of sessions not ended.
- **Motion:** bulbs chase, sunburst rotates slowly and moves against the
  pointer (parallax).
- **Reduced motion:** static bulbs and art.

### 2. Dot-matrix display (DMD)

`.dmd > .dmd-screen`. Real text under a radial-gradient mask, so it reads as
lit dots and stays selectable and readable by assistive tech. `.dmd-fine` uses
a 3px pitch for small text; 4px dots cannot draw small glyphs.

- **Score:** total coins, eased count-up.
- **Ticker:** routine events (`CLAUDE agent-pinball +18,204`,
  `api-gateway turn 45`), queued, at most 4 waiting, one every 1.1s, wipe-in.
- **Takeover:** full screen for big moments: BALL SAVED, MULTIBALL,
  BALL LAUNCHED, TILT, GAME OVER. The score is hidden while it plays because
  the dot mask would let it show through.
- **Accessibility:** not a live region. Announcements go through
  `#announcer` (see Accessibility).

### 3. Playfield

`section.table > .playfield`. Tilted back with `perspective` and
`rotateX(var(--tilt))`, pivoting on the bottom edge so the far end recedes.
`fitTilt()` measures the empty band this leaves and pulls the table up via
`--lift`.

Parallax layers, each moved by `--px`/`--py` times its `--depth`:

| Layer | Depth | Content |
|---|---|---|
| `.layer-art` | -8 | Sunburst, client glows, stars, rings |
| lanes | 0 | Everything clickable stays still under the cursor |
| `.layer-wire` | 4 | Wireform along the top and side gutters, never over a card |
| `.layer-glass` | 26 | Glass glare |

- **Tilt:** 12° desktop, 8° under 980px, flat under 640px. "Flat view" in the
  coin door turns it off and is remembered per browser.
- **Reduced motion:** no tilt, no parallax.

### 4. Lane

`section.lane[data-kind]`. One per client: arched plastic header with the
client name, "N balls in play", and three chevron inserts that chase upward.
Holds that client's bumpers in `.lane-body`; `.lane-empty` shows
"No balls in play".

- **Layout:** three columns on desktop, stacked under 980px.

### 5. Agent bumper (session card)

`article.bumper[data-kind][data-status][data-level]`, built by
`createBumper(session)` and refreshed by `updateBumper(el, session)`. The
whole card is one control through the stretched `.bumper-hit` button, whose
`aria-label` carries project, client, status and context.

| Part | Binds |
|---|---|
| Ring (`.ring-fill`, `pathLength=100`) | `--ctx` = context used / limit, in % |
| Cap label | Context %, or `??` when the limit is unknown |
| Ball (`.orbit .ball`) | Status; orbit speed `--spin` follows coins/min |
| Name | Project directory name (sanitized) |
| Model chip | Short model name |
| Insert | Status |
| YOUR SHOT | Shown while waiting, with the reason |
| `.ctx-of` | `used / limit` in compact units |
| TILT WARNING | Shown at `data-level` hot or crit |
| Score reels | Total coins |
| Drop targets | Turns |
| Extra balls | Subagents and their status |
| Rate | Coins per minute over the last 60s |
| Action strip | `links` (see 16) |

**States**

| `data-status` | Look |
|---|---|
| `running` | Ball orbits the ring; insert is a blinking triangle |
| `waiting` | Ball held at the top of the ring (kickout hole); star insert; magenta frame and YOUR SHOT / reason, all static; the glow behind the ring pulses only when motion is allowed |
| `idle` | Ball rests at the bottom (saucer); round insert |
| `stuck` | Ball rattles in place; blinking diamond insert |
| `error` | Card shakes, striped TILT overlay; X insert |
| `ended` | Greyscale, GAME OVER stamp; dim square insert |

| `data-level` | Context | Ring |
|---|---|---|
| `ok` | under 60% | client colour |
| `warm` | 60 to 80% | amber |
| `hot` | 80 to 95% | orange, TILT WARNING |
| `crit` | 95% and up | red, TILT WARNING |

**Moments**

| Event | Animation |
|---|---|
| `tokens` | Cap pops, ring flashes, `+12.4k` floats up |
| `turn_end` | Next drop target falls; bank resets after five |
| `compacted` | BALL SAVED banner |
| crossing 80% | TILT WARNING banner, once per crossing |
| `launched` | Card drops in after the plunger fires (`--delay: .55s`) |
| `status` into `waiting` | DMD takeover YOUR SHOT, `#announcer` sentence by reason, the chime (see 17), `document.title` prefix `(N) WAITING / ` |

**Order:** waiting cards sort first in their lane; the rest keep their
order. The DOM is reordered, not CSS `order`, so tab order matches what is
on screen. The server sorts the first render the same way.

The chime only follows a `status` event, which is a transition, so the
first `board` snapshot and every reconnect snapshot are silent. The title
prefix is updated outside `requestAnimationFrame`, since hidden tabs get no
frames and the title is what a hidden tab shows.

**Missing data:** Cursor records no token spend locally, so its reels show
dashes, the rate reads "coins n/a" and the backglass explains why. A session
with no known limit shows `??` and an empty ring.

### 6. Score reels

`.reels > .reel > .reel-strip`. Nine electro-mechanical digit wheels with
comma separators. Each strip holds 0 to 9 plus a dash; `--d` picks the digit
and a spring transition rolls it. `role="img"` with the formatted value as
its label. `[data-unavailable]` shows dashes on dark wheels.

### 7. Insert lights

`.insert[data-status]`. Lamp plus text label. Shape carries the meaning as
well as colour, so status never depends on colour alone: triangle (running),
star (waiting), circle (idle), diamond (stuck), X (error), square (ended).

### 8. Drop targets

`.targets > .target` x5. One falls per finished turn (`turns % 5`); after the
fifth the bank drops fully and resets. Label: "N turns".

### 9. Extra balls

`.extra-balls > .mini-ball[data-status]`. One chrome ball per subagent
(Claude sidechains, Codex spawned threads, Cursor subagent composers).
Running ones bob and glow; ended ones fade.

### 10. Context ramp

`svg.ramp`, in the backglass. A wireform ramp drawn on one path: black track,
heat-gradient fill (`stroke-dasharray: var(--ctx) 100`), segment cuts, two
chrome rails, and a ball placed with `getPointAtLength` at the current level.
Marks at 60%, 80% (TILT WARNING) and 95%.

### 11. Plunger and shooter lane

`.shooter` with `button.plunger`. On `launched` the spring compresses, the rod
pulls, then a ball shoots up the lane (Web Animations API). In the prototype,
clicking the plunger starts a demo session; in the product it is decoration.

### 12. Flippers, slingshots, inlane lights

The lower playfield SVG. Flippers alternate a flick on every event (a
heartbeat for the stream), slingshots kick on token bursts, inlane lights
shuffle on finished turns. Purely decorative and `aria-hidden`.

### 13. Apron

`footer.apron`. Two cards on a red apron:

- **How to read the table:** the legend for ring, reels, inserts, targets,
  extra balls, TILT WARNING and BALL SAVED.
- **Table totals:** per client, balls in play, coins and average context.

### 14. Backglass detail

`dialog.backglass[data-kind]`, opened from any bumper with `showModal()`, so
focus is trapped, Escape closes it and focus returns to the card. Updates live
while open.

- Header: client and model, project name, session title (sanitized).
- Context ramp and the exact `used of limit` sentence.
- Context over time (sparkline from the session's samples); compactions are
  marked where the line drops by 40% or more; dashed line at the limit.
- Coins reels and a breakdown bar per category (input, cache read, cache
  write, output).
- Jump to the session: the action strip (16) plus the full resume command
  in `code.resume-cmd`. Hidden when the session has no links.
- Facts: model, status (with the waiting reason), turns, compactions,
  subagents, PID, uptime, last activity.

### 15. Coin door

`footer.coin-door`. Link state of the event stream: LIVE (green) or
INSERT COIN (blinking, coin slots flash) while disconnected and reconnecting.
Holds the view toggles. The prototype adds "Pause demo", "Drop link" and
"Ask for input" (forces a waiting transition) to exercise those states.

- **Sound** (`[data-toggle="sound"]`, `aria-pressed`): see 17.
- **Chime on idle** (`[data-toggle="idle-chime"]`, `aria-pressed`): opts in
  to chiming for `idle_prompt`, remembered per browser.

### 16. Bumper actions

`.bumper-actions`, in every bumper and in the backglass ("Jump to the
session"). Navigation and clipboard only; the panel stays read-only.

| Control | Binds | Text |
|---|---|---|
| `a[data-link="editor"]` | `links.editor.href` | "Open in VS Code" or "Open in Cursor" |
| `button[data-link="resume"]` | `links.resume.command` | "Copy resume command" |
| `a[data-link="app"]` | `links.app.href` | `links.app.label` |

- **Layering:** a sibling of the stretched `.bumper-hit`, after it in the
  DOM and above it (`z-index: 3`). Never nested inside the button, so each
  control has its own role, name and focus stop.
- **Allowlist:** the server builds every link, and the client still refuses
  any `href` that is not printable ASCII or does not start with
  `vscode://file/`, `cursor://file/` (editor) or `codex://threads/` (app).
  A null or refused link hides its control; the strip hides when nothing is
  left.
- **Copy:** `navigator.clipboard.writeText`. Button reads "Copied" for 2s.
  When the clipboard is refused it reads "Copy failed" and the backglass
  opens with the command selected in `code.resume-cmd`, ready for Ctrl+C.
- **Names:** each control's `aria-label` includes the project, since every
  card has the same visible text.
- **Waiting:** controls take the `--st-waiting` border.

### 17. Sound

The waiting chime is synthesized with Web Audio: two triangle-wave tones
(880 Hz then 1318.5 Hz), under 400 ms in total. No audio file, so no CSP
`media-src`.

- **Gesture:** browsers only start audio after a user gesture. The coin-door
  "Sound" toggle creates and resumes the page's one `AudioContext` in its
  click handler.
- **Remembered:** on/off goes through `remember("pinball.sound")`. When it
  was left on, the door shows ARM SOUND (`data-armed="false"`, blinking)
  until the next click or key press anywhere on the page arms it. Clicking
  ARM SOUND arms it; it does not turn sound off.
- **When:** only on a `status` event into `waiting`, never on a snapshot, at
  most one chime every 2s. `idle_prompt` is silent unless "Chime on idle"
  is on; `approval` and `question` always chime.
- **Hidden tabs:** the stream stays connected, so a background tab still
  chimes.

## Data contract

The board consumes exactly three message shapes. The prototype's
`mock-data.js` emits them; the product will receive them from `GET /events`
(server-sent events), so `pinball.js` swaps the simulator for an
`EventSource` and nothing else changes.

```jsonc
// On connect and on every reconnect: full snapshot, the board reconciles.
{ "type": "board", "sessions": [Session] }

// Whenever a session changes.
{ "type": "delta", "session": Session }

// Moments worth animating. kind is one of:
// tokens {amount} | turn_end {turns} | compacted {preTokens, postTokens}
// | launched | status {status} | removed
// status {status: "waiting"} is the sound trigger; there is no extra kind.
{ "type": "event", "kind": "tokens", "sessionId": "claude-7f3a", "amount": 18204 }
```

```jsonc
// Session
{
  "id": "claude-7f3a",
  "kind": "claude",                 // claude | codex | cursor
  "project": "agent-pinball",       // sanitized, directory name only
  "title": "system design ...",     // sanitized, truncated
  "model": "claude-opus-5-5",
  "status": "running",              // running | waiting | idle | stuck | error | ended
  "waitingSince": null,             // ms epoch while waiting, else null
  "waitingReason": null,            // approval | question | idle_prompt, or null
  "context": { "used": 412000, "limit": 1000000 },   // limit may be null
  "coins": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 }, // or null
  "turns": 38,
  "compactions": 0,
  "subagents": [{ "id": "...", "status": "running" }],
  "pid": 48122,                     // null when no live process is known
  "startedAt": 1792270000000,
  "lastActivityAt": 1792275000000,
  "history": [18000, 52000, 412000], // context samples for the sparkline
  "links": {                        // built by the server; each one may be null
    "editor": { "app": "vscode", "href": "vscode://file/Users/me/src/agent-pinball" },
    "resume": { "command": "cd '/Users/me/src/agent-pinball' && claude --resume <id>" },
    "app": { "label": "Open in Codex app", "href": "codex://threads/<id>" }
  }
}
```

`waitingReason` is an enum only; the panel never receives the prompt or
message text behind it. `links.editor.app` is `vscode` or `cursor`. Paths
in `links` are the only place the full project directory appears.

## Motion budget

- Animate `transform` and `opacity` only, plus `stroke-dasharray` on SVG
  gauges.
- One `requestAnimationFrame` loop each for parallax and the DMD count-up,
  and both stop when they settle.
- The event stream stays connected while the tab is hidden, so a background
  tab can still chime and update its title; only the animations pause (the
  browser stops frames and the rAF loops idle).
- `prefers-reduced-motion: reduce` collapses every animation and transition
  to an instant change, removes tilt and parallax, and keeps every state
  readable: status still shows by insert shape and label, context by the
  ring, coins by the reels.

## Accessibility

- Each bumper is one button with a full sentence label.
- Status uses shape, text and colour together.
- `#announcer` (`aria-live="polite"`) speaks status changes, compactions,
  TILT WARNING and new sessions, at most one message every 4 seconds. Token
  bursts are never announced. Entering waiting says why: "is waiting for
  your approval", "has a question for you", "is waiting for your next
  prompt".
- Sound is never the only signal: every chime comes with the announcer
  message, the DMD takeover, the title prefix and the waiting frame.
- DMD and score windows are real text; decorative SVG is `aria-hidden`.
- The backglass is a native modal dialog.
- Works at 375px wide with no horizontal scroll.

## Porting to pinball-web

The prototype is written to move into `crates/pinball-web` with no rewrite:

- Classic scripts, no inline `<script>`, no `style=""`, no `on*` attributes:
  everything works under
  `default-src 'self'; script-src 'self'; style-src 'self'`.
- Runtime values go through `element.style.setProperty('--x', ...)` (CSSOM is
  allowed under that CSP).
- Agent-provided text only ever goes through `textContent`. `innerHTML` is
  used only for constant markup and numbers.
- The server renders the first `board` into the HTML so the page is useful
  before the script runs, then the script takes over from `/events`.
- `mock-data.js` is dropped; the simulator's message shapes are the contract.
