/*
 * agent-pinball design prototype: components, board controller, motion.
 *
 * Rules that keep this portable to pinball-web under a strict CSP:
 * - markup strings below are constants; agent-provided text (project, title,
 *   model) only ever goes through textContent, never innerHTML;
 * - runtime values go through style.setProperty on custom properties;
 * - the board only consumes { type: "board" | "delta" | "event" } messages,
 *   so mock-data.js can be swapped for an EventSource on /events.
 */
(function () {
  "use strict";

  const KINDS = ["claude", "codex", "cursor"];
  const KIND_LABEL = { claude: "Claude", codex: "Codex", cursor: "Cursor" };
  const STATUS_LABEL = { running: "Running", waiting: "Waiting", idle: "Idle", stuck: "Stuck", error: "Error", ended: "Ended" };
  const REASON_LABEL = { approval: "approval", question: "question", idle_prompt: "next prompt" };
  const REASON_FOR = { approval: "for approval", question: "for an answer", idle_prompt: "for a prompt" };
  const REASON_SENTENCE = {
    approval: "is waiting for your approval",
    question: "has a question for you",
    idle_prompt: "is waiting for your next prompt"
  };
  const EDITOR_LABEL = { vscode: "VS Code", cursor: "Cursor" };
  // The server builds every link; the client still refuses anything outside
  // these prefixes before it becomes an href.
  const LINK_PREFIXES = {
    editor: ["vscode://file/", "cursor://file/"],
    app: ["codex://threads/"]
  };
  const CHIME_GAP_MS = 2000;
  const REEL_DIGITS = 9;
  const TARGETS = 5;
  const RATE_WINDOW_MS = 60000;
  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

  /* ------------------------------------------------------------ helpers */

  const $ = (root, sel) => root.querySelector(sel);
  const $$ = (root, sel) => Array.from(root.querySelectorAll(sel));

  function fmt(n) {
    return Math.round(n).toLocaleString("en-US");
  }

  function compact(n) {
    if (n >= 1e6) return trimZero((n / 1e6).toFixed(n >= 1e7 ? 0 : 1)) + "M";
    if (n >= 1e3) return trimZero((n / 1e3).toFixed(n >= 1e5 ? 0 : 1)) + "k";
    return String(Math.round(n));
  }

  function trimZero(text) {
    return text.replace(/\.0$/, "");
  }

  function totalCoins(f) {
    return f ? f.input + f.output + f.cacheRead + f.cacheWrite : null;
  }

  function contextPct(s) {
    return s.context.limit ? Math.min(100, (s.context.used / s.context.limit) * 100) : null;
  }

  function level(pct) {
    if (pct === null) return "ok";
    if (pct >= 95) return "crit";
    if (pct >= 80) return "hot";
    if (pct >= 60) return "warm";
    return "ok";
  }

  function shortModel(model) {
    return model.replace(/^claude-/, "").replace(/-(\d+)-(\d+)$/, " $1.$2");
  }

  function ago(ms) {
    const s = Math.max(0, Math.round(ms / 1000));
    if (s < 60) return s + "s ago";
    const m = Math.round(s / 60);
    return m < 60 ? m + " min ago" : Math.round(m / 60) + " h ago";
  }

  function uptime(ms) {
    const m = Math.round(ms / 60000);
    return m < 60 ? m + " min" : Math.floor(m / 60) + " h " + (m % 60) + " min";
  }

  function statusText(s) {
    const reason = s.status === "waiting" && REASON_FOR[s.waitingReason];
    return STATUS_LABEL[s.status] + (reason ? " " + reason : "");
  }

  // Printable ASCII only, and one of the allowlisted prefixes.
  function safeHref(kind, href) {
    if (typeof href !== "string" || !/^[\x21-\x7e]+$/.test(href)) return null;
    return LINK_PREFIXES[kind].some((prefix) => href.startsWith(prefix)) ? href : null;
  }

  // Restart a CSS animation that is driven by a class.
  function retrigger(el, cls) {
    el.classList.remove(cls);
    void el.getBoundingClientRect();
    el.classList.add(cls);
  }

  function spawn(parent, cls, text) {
    const el = document.createElement("span");
    el.className = cls;
    el.textContent = text;
    el.addEventListener("animationend", () => el.remove(), { once: true });
    parent.append(el);
    return el;
  }

  /* ----------------------------------------------------- static markup */

  const DEFS =
    '<svg class="visually-hidden" aria-hidden="true" focusable="false"><defs>' +
    '<linearGradient id="chrome-h" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#ffffff"/><stop offset=".45" stop-color="#b9bccb"/><stop offset="1" stop-color="#3b3e4e"/></linearGradient>' +
    '<linearGradient id="chrome-v" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#3b3e4e"/><stop offset=".45" stop-color="#ffffff"/><stop offset="1" stop-color="#5a5d70"/></linearGradient>' +
    '<radialGradient id="ball" cx=".35" cy=".3" r=".8"><stop offset="0" stop-color="#ffffff"/><stop offset=".3" stop-color="#d4d7e2"/><stop offset=".8" stop-color="#4b4e5c"/><stop offset="1" stop-color="#1c1d24"/></radialGradient>' +
    '<radialGradient id="skirt" cx=".5" cy=".5" r=".5"><stop offset=".7" stop-color="#14121c"/><stop offset=".92" stop-color="#9aa0b4"/><stop offset="1" stop-color="#2a2a33"/></radialGradient>' +
    '<radialGradient id="cap-claude" cx=".4" cy=".3" r=".8"><stop offset="0" stop-color="#ffe7b0"/><stop offset=".45" stop-color="#ff6b3d"/><stop offset="1" stop-color="#7a1a08"/></radialGradient>' +
    '<radialGradient id="cap-codex" cx=".4" cy=".3" r=".8"><stop offset="0" stop-color="#eadcff"/><stop offset=".45" stop-color="#9b6bff"/><stop offset="1" stop-color="#2e1470"/></radialGradient>' +
    '<radialGradient id="cap-cursor" cx=".4" cy=".3" r=".8"><stop offset="0" stop-color="#dcfcff"/><stop offset=".45" stop-color="#2ad4f0"/><stop offset="1" stop-color="#0a2f80"/></radialGradient>' +
    '<radialGradient id="glow-claude"><stop offset="0" stop-color="#ff6b3d" stop-opacity=".55"/><stop offset="1" stop-color="#ff6b3d" stop-opacity="0"/></radialGradient>' +
    '<radialGradient id="glow-codex"><stop offset="0" stop-color="#9b6bff" stop-opacity=".55"/><stop offset="1" stop-color="#9b6bff" stop-opacity="0"/></radialGradient>' +
    '<radialGradient id="glow-cursor"><stop offset="0" stop-color="#2ad4f0" stop-opacity=".5"/><stop offset="1" stop-color="#2ad4f0" stop-opacity="0"/></radialGradient>' +
    '<linearGradient id="sling" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#ffd27a"/><stop offset="1" stop-color="#d0461a"/></linearGradient>' +
    '<linearGradient id="flipper" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#ffffff"/><stop offset=".6" stop-color="#f1ede2"/><stop offset="1" stop-color="#9a958a"/></linearGradient>' +
    '<linearGradient id="ramp-heat" gradientUnits="userSpaceOnUse" x1="30" y1="0" x2="570" y2="0"><stop offset="0" stop-color="#39ff88"/><stop offset=".55" stop-color="#39ff88"/><stop offset=".66" stop-color="#ffc800"/><stop offset=".84" stop-color="#ff7a1a"/><stop offset=".97" stop-color="#ff2e4d"/></linearGradient>' +
    "</defs></svg>";

  function artLayer() {
    let rays = "";
    for (let i = 0; i < 24; i += 1) {
      const a = (i / 24) * Math.PI * 2;
      const b = a + Math.PI / 48;
      rays +=
        '<polygon points="500,460 ' +
        (500 + Math.cos(a) * 900).toFixed(0) + "," + (460 + Math.sin(a) * 900).toFixed(0) + " " +
        (500 + Math.cos(b) * 900).toFixed(0) + "," + (460 + Math.sin(b) * 900).toFixed(0) + '"/>';
    }
    let stars = "";
    for (let i = 0; i < 70; i += 1) {
      // Deterministic scatter so the art does not reshuffle on every load.
      const x = (i * 137.5) % 1000;
      const y = (i * 263.1) % 1000;
      stars += '<circle cx="' + x.toFixed(0) + '" cy="' + y.toFixed(0) + '" r="' + (1 + (i % 3)) + '"/>';
    }
    return (
      '<svg viewBox="0 0 1000 1000" preserveAspectRatio="xMidYMid slice" aria-hidden="true">' +
      '<g class="art-rays" fill="#ffffff" fill-opacity=".025">' + rays + "</g>" +
      '<circle cx="170" cy="330" r="330" fill="url(#glow-claude)"/>' +
      '<circle cx="500" cy="300" r="330" fill="url(#glow-codex)"/>' +
      '<circle cx="830" cy="330" r="330" fill="url(#glow-cursor)"/>' +
      '<g fill="#ffffff" fill-opacity=".18">' + stars + "</g>" +
      '<g fill="none" stroke="#ffffff" stroke-opacity=".06" stroke-width="2">' +
      '<circle cx="500" cy="760" r="180"/><circle cx="500" cy="760" r="240"/><circle cx="500" cy="760" r="300"/></g>' +
      "</svg>"
    );
  }

  // The wireform runs above the lanes and down the side gutters, never over
  // a card: it is decoration and must not cost readability.
  const WIRE_LAYER =
    '<svg class="wire-top" viewBox="0 0 1000 60" preserveAspectRatio="none" aria-hidden="true">' +
    '<path class="wire" d="M996 60 C996 22 976 10 940 10 L60 10 C24 10 4 22 4 60"/>' +
    '<path class="wire" d="M988 60 C988 30 970 22 940 22 L60 22 C30 22 12 30 12 60"/>' +
    '<path class="wire-light" d="M992 60 C992 26 973 16 940 16 L60 16 C27 16 8 26 8 60"/>' +
    "</svg>" +
    '<span class="wire-side wire-side-l"></span><span class="wire-side wire-side-r"></span>';

  const LOWER =
    '<svg viewBox="0 0 1000 220" aria-hidden="true">' +
    '<path class="wire" d="M90 0 L90 110 L322 190"/>' +
    '<path class="wire" d="M910 0 L910 110 L678 190"/>' +
    '<circle class="inlane-light" cx="130" cy="58" r="9"/><circle class="inlane-light" cx="130" cy="92" r="9"/>' +
    '<circle class="inlane-light" cx="870" cy="58" r="9"/><circle class="inlane-light" cx="870" cy="92" r="9"/>' +
    '<polygon class="sling" points="190,0 190,96 300,128"/><line class="sling-rubber" x1="196" y1="4" x2="298" y2="122"/>' +
    '<polygon class="sling" points="810,0 810,96 700,128"/><line class="sling-rubber" x1="804" y1="4" x2="702" y2="122"/>' +
    '<path class="flipper flipper-l" d="M330 126 L460 134 A6 6 0 0 1 460 146 L330 154 A14 14 0 0 1 330 126 Z"/>' +
    '<path class="flipper flipper-r" d="M670 126 L540 134 A6 6 0 0 0 540 146 L670 154 A14 14 0 0 0 670 126 Z"/>' +
    '<circle cx="500" cy="196" r="12" fill="#3a2a10" stroke="#000" stroke-width="2"/>' +
    '<text x="500" y="216" text-anchor="middle" fill="#a49cb8" font-size="10" letter-spacing="3">AGENT SAVE</text>' +
    "</svg>";

  const SHOOTER =
    '<div class="shooter">' +
    '<span class="shooter-label">LAUNCH</span>' +
    '<span class="shot-ball"></span>' +
    '<button class="plunger" type="button" aria-label="Launch a demo session">' +
    '<svg viewBox="0 0 40 120" aria-hidden="true">' +
    '<polyline class="plunger-spring" points="20,22 8,30 32,38 8,46 32,54 8,62 32,70 8,78 32,86 8,94 20,100"/>' +
    '<g class="plunger-rod"><rect x="17" y="10" width="6" height="96" rx="2" fill="url(#chrome-v)"/>' +
    '<rect x="12" y="8" width="16" height="6" rx="2" fill="url(#chrome-v)"/>' +
    '<rect x="6" y="100" width="28" height="18" rx="6" fill="#d22" stroke="#000" stroke-width="2"/></g>' +
    "</svg></button></div>";

  // Sits after .bumper-hit and above it; never inside the button.
  const ACTIONS =
    '<div class="bumper-actions" hidden>' +
    '<a class="action" data-link="editor" hidden></a>' +
    '<button class="action" type="button" data-link="resume" hidden>Copy resume command</button>' +
    '<a class="action" data-link="app" hidden></a>' +
    "</div>";

  const INSERT = '<span class="insert"><span class="insert-lamp"></span><span class="insert-text"></span></span>';

  function reelsMarkup() {
    let html = '<span class="reels" role="img">';
    for (let i = 0; i < REEL_DIGITS; i += 1) {
      if (i > 0 && (REEL_DIGITS - i) % 3 === 0) html += '<span class="reel-sep">,</span>';
      html += '<span class="reel"><span class="reel-strip">';
      for (let d = 0; d < 10; d += 1) html += "<span>" + d + "</span>";
      html += "<span>-</span></span></span>";
    }
    return html + "</span>";
  }

  function targetsMarkup() {
    let html = '<span class="targets" role="img">';
    for (let i = 0; i < TARGETS; i += 1) html += '<span class="target"></span>';
    return html + "</span>";
  }

  const BUMPER =
    '<svg class="bumper-ring" viewBox="0 0 120 120" aria-hidden="true">' +
    '<circle class="ring-track" cx="60" cy="60" r="52"/>' +
    '<circle class="ring-fill" cx="60" cy="60" r="52" pathLength="100"/>' +
    '<circle class="ring-ticks" cx="60" cy="60" r="52" pathLength="100"/>' +
    '<circle class="bumper-skirt" cx="60" cy="60" r="45"/>' +
    '<circle class="bumper-cap" cx="60" cy="60" r="35"/>' +
    '<text class="cap-label" x="60" y="56"></text>' +
    '<text class="cap-sub" x="60" y="77">CONTEXT</text>' +
    '<g class="orbit"><circle class="ball" cx="60" cy="8" r="7"/></g>' +
    "</svg>" +
    '<h3 class="bumper-name"></h3>' +
    '<div class="bumper-sub"><span class="chip model"></span>' + INSERT + "</div>" +
    '<div class="ctx-line"><span class="ctx-of"></span><span class="tilt-warning">TILT WARNING</span><span class="your-shot"></span></div>' +
    '<div class="bumper-foot">' +
    '<div><div class="coins-label">Coins</div>' + reelsMarkup() + "</div>" +
    '<div class="foot-right">' + targetsMarkup() + '<span class="extra-balls" role="img"></span><span class="rate"></span></div>' +
    "</div>" +
    '<div class="waiting-glow" aria-hidden="true"></div>' +
    '<div class="overlay overlay-tilt" aria-hidden="true">TILT</div>' +
    '<div class="overlay overlay-over" aria-hidden="true">GAME OVER</div>' +
    '<button class="bumper-hit" type="button"></button>' +
    ACTIONS;

  const RAMP_D = "M30 150 C150 150 210 40 330 52 S520 140 570 30";

  const RAMP =
    '<svg class="ramp" viewBox="0 0 600 180" role="img">' +
    '<path class="ramp-track" d="' + RAMP_D + '"/>' +
    '<path class="ramp-fill" d="' + RAMP_D + '" pathLength="100"/>' +
    '<path class="ramp-cuts" d="' + RAMP_D + '" pathLength="100"/>' +
    '<path class="ramp-rail" transform="translate(0 -15)" d="' + RAMP_D + '"/>' +
    '<path class="ramp-rail" transform="translate(0 15)" d="' + RAMP_D + '"/>' +
    '<g class="ramp-marks"></g>' +
    '<circle class="ramp-ball" r="10" cx="30" cy="150"/>' +
    "</svg>";

  const DRAWER =
    '<dialog class="backglass" aria-labelledby="bg-title">' +
    '<header class="bg-head">' +
    '<div class="bg-kicker"></div>' +
    '<h2 class="bg-title" id="bg-title"></h2>' +
    '<p class="note bg-session"></p>' +
    '<button class="bg-close" type="button" aria-label="Close details">&times;</button>' +
    "</header>" +
    '<div class="bg-body">' +
    '<section class="bg-actions"><h3 class="section-label">Jump to the session</h3>' + ACTIONS +
    '<code class="resume-cmd" hidden></code></section>' +
    '<section><h3 class="section-label">Context ramp</h3>' + RAMP + '<p class="note bg-ctx"></p></section>' +
    '<section><h3 class="section-label">Context over time</h3>' +
    '<svg class="spark" viewBox="0 0 300 96" preserveAspectRatio="none" aria-hidden="true">' +
    '<line class="spark-limit" x1="0" x2="300" y1="6" y2="6"/>' +
    '<polygon class="spark-area"/><polyline class="spark-line"/><g class="spark-marks"></g></svg>' +
    '<p class="note">Drops are compactions (BALL SAVED). Dashed line is the model limit.</p></section>' +
    '<section><h3 class="section-label">Coins spent</h3>' + reelsMarkup() +
    '<div class="breakdown"></div><p class="note bg-coins-note"></p></section>' +
    '<dl class="bg-grid">' +
    ["model", "status", "turns", "compactions", "subagents", "pid", "uptime", "activity"]
      .map((k) => '<div class="stat"><dt>' + k + '</dt><dd data-stat="' + k + '"></dd></div>')
      .join("") +
    "</dl></div></dialog>";

  /* -------------------------------------------------------- components */

  function injectDefs() {
    document.body.insertAdjacentHTML("afterbegin", DEFS);
  }

  function fillSlots(root) {
    const slots = { art: artLayer(), wire: WIRE_LAYER, lower: LOWER, shooter: SHOOTER };
    $$(root, "[data-slot]").forEach((el) => {
      el.innerHTML = slots[el.dataset.slot] || "";
    });
    $$(root, "[data-bulbs]").forEach((el) => {
      for (let i = 0; i < Number(el.dataset.bulbs); i += 1) {
        const b = document.createElement("span");
        b.className = "bulb";
        b.style.setProperty("--i", i);
        el.append(b);
      }
    });
    $$(root, ".chevrons").forEach((el) => {
      for (let i = 0; i < 3; i += 1) {
        const c = document.createElement("span");
        c.className = "chevron";
        c.style.setProperty("--i", 2 - i);
        el.append(c);
      }
    });
  }

  function setReels(el, value) {
    const strips = $$(el, ".reel-strip");
    if (value === null) {
      el.dataset.unavailable = "";
      strips.forEach((s) => s.style.setProperty("--d", 10));
      el.setAttribute("aria-label", "Coins not recorded by this client");
      return;
    }
    delete el.dataset.unavailable;
    const digits = String(Math.min(Math.round(value), 10 ** REEL_DIGITS - 1)).padStart(REEL_DIGITS, "0");
    strips.forEach((s, i) => s.style.setProperty("--d", digits[i]));
    el.setAttribute("aria-label", fmt(value) + " coins");
  }

  function setInsert(el, status) {
    el.dataset.status = status;
    $(el, ".insert-text").textContent = STATUS_LABEL[status];
  }

  async function copyText(text) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch (_) {
      return false;
    }
  }

  // Fills an action strip from session.links. Missing or refused links hide
  // their control; the strip hides when nothing is left.
  function setActions(el, s, onCopyFailed) {
    const links = s.links || {};
    const editor = links.editor && EDITOR_LABEL[links.editor.app] ? links.editor : null;
    const editorHref = editor && safeHref("editor", editor.href);
    const appHref = links.app && safeHref("app", links.app.href);
    const command = links.resume && typeof links.resume.command === "string" ? links.resume.command : null;

    const editorEl = $(el, '[data-link="editor"]');
    editorEl.hidden = !editorHref;
    if (editorHref) {
      editorEl.href = editorHref;
      editorEl.textContent = "Open in " + EDITOR_LABEL[editor.app];
      editorEl.setAttribute("aria-label", "Open " + s.project + " in " + EDITOR_LABEL[editor.app]);
    } else {
      editorEl.removeAttribute("href");
    }

    const appEl = $(el, '[data-link="app"]');
    appEl.hidden = !appHref;
    if (appHref) {
      appEl.href = appHref;
      appEl.textContent = typeof links.app.label === "string" ? links.app.label : "Open app";
      appEl.setAttribute("aria-label", appEl.textContent + ": " + s.project);
    } else {
      appEl.removeAttribute("href");
    }

    const resumeEl = $(el, '[data-link="resume"]');
    resumeEl.hidden = command === null;
    resumeEl.setAttribute("aria-label", "Copy resume command for " + s.project);
    if (!resumeEl.dataset.wired) {
      resumeEl.dataset.wired = "";
      resumeEl.addEventListener("click", async () => {
        const ok = await copyText(resumeEl.dataset.command);
        resumeEl.textContent = ok ? "Copied" : "Copy failed";
        if (!ok && onCopyFailed) onCopyFailed();
        window.setTimeout(() => {
          resumeEl.textContent = "Copy resume command";
        }, 2000);
      });
    }
    if (command !== null) resumeEl.dataset.command = command;

    el.hidden = !editorHref && !appHref && command === null;
  }

  // A bank of five drop targets: one falls per finished turn, then resets.
  function setTargets(el, turns) {
    const down = turns % TARGETS;
    const prev = el.dataset.down === undefined ? down : Number(el.dataset.down);
    el.dataset.down = down;
    el.setAttribute("aria-label", turns + " turns");
    const targets = $$(el, ".target");
    if (down === 0 && prev === TARGETS - 1) {
      targets.forEach((t) => t.classList.add("down"));
      window.setTimeout(() => targets.forEach((t) => t.classList.remove("down")), 650);
      return;
    }
    targets.forEach((t, i) => t.classList.toggle("down", i < down));
  }

  function setExtraBalls(el, subagents) {
    el.replaceChildren(
      ...subagents.map((a) => {
        const b = document.createElement("span");
        b.className = "mini-ball";
        b.dataset.status = a.status;
        return b;
      })
    );
    const live = subagents.filter((a) => a.status !== "ended").length;
    el.setAttribute("aria-label", subagents.length ? subagents.length + " subagents, " + live + " active" : "No subagents");
  }

  function setRamp(svg, pct) {
    const p = pct === null ? 0 : pct;
    svg.style.setProperty("--ctx", p.toFixed(1));
    svg.setAttribute("aria-label", pct === null ? "Context limit unknown" : "Context " + Math.round(p) + " percent of limit");
    const path = $(svg, ".ramp-track");
    const len = path.getTotalLength();
    const ball = $(svg, ".ramp-ball");
    const pt = path.getPointAtLength((len * p) / 100);
    ball.setAttribute("cx", pt.x.toFixed(1));
    ball.setAttribute("cy", pt.y.toFixed(1));
    const marks = $(svg, ".ramp-marks");
    if (!marks.childElementCount) {
      [[60, "60%"], [80, "80% TILT WARNING"], [95, "95%"]].forEach(([at, label]) => {
        const m = path.getPointAtLength((len * at) / 100);
        marks.insertAdjacentHTML(
          "beforeend",
          '<line class="ramp-mark-line" x1="' + m.x.toFixed(0) + '" x2="' + m.x.toFixed(0) +
            '" y1="' + (m.y - 26).toFixed(0) + '" y2="' + (m.y + 26).toFixed(0) + '"/>' +
            '<text class="ramp-mark" text-anchor="middle" x="' + m.x.toFixed(0) + '" y="' + (m.y - 32).toFixed(0) + '">' + label + "</text>"
        );
      });
    }
  }

  function createBumper(s) {
    const el = document.createElement("article");
    el.className = "bumper";
    el.dataset.id = s.id;
    el.innerHTML = BUMPER;
    updateBumper(el, s);
    return el;
  }

  function updateBumper(el, s) {
    const pct = contextPct(s);
    el.dataset.kind = s.kind;
    el.dataset.status = s.status;
    el.dataset.level = level(pct);
    el.style.setProperty("--ctx", pct === null ? 0 : pct.toFixed(1));
    $(el, ".cap-label").textContent = pct === null ? "??" : Math.round(pct) + "%";
    $(el, ".bumper-name").textContent = s.project;
    $(el, ".model").textContent = shortModel(s.model);
    $(el, ".ctx-of").textContent = compact(s.context.used) + " / " + (s.context.limit ? compact(s.context.limit) : "??");
    const reason = REASON_LABEL[s.waitingReason];
    $(el, ".your-shot").textContent = "YOUR SHOT" + (reason ? " / " + reason : "");
    setInsert($(el, ".insert"), s.status);
    setReels($(el, ".reels"), totalCoins(s.coins));
    setTargets($(el, ".targets"), s.turns);
    setExtraBalls($(el, ".extra-balls"), s.subagents);
    $(el, ".bumper-hit").setAttribute(
      "aria-label",
      s.project + ", " + KIND_LABEL[s.kind] + ", " + statusText(s) + ", context " +
        (pct === null ? "unknown" : Math.round(pct) + " percent") + ". Open details"
    );
    setActions($(el, ".bumper-actions"), s, () => el.dispatchEvent(new CustomEvent("copyfailed")));
  }

  function setRate(el, s, rate) {
    const label = $(el, ".rate");
    if (s.coins === null) {
      label.textContent = "coins n/a";
      return;
    }
    label.textContent = rate === null ? "-- /min" : compact(rate) + "/min";
    // The ball rolls faster the faster this session burns coins.
    const spin = rate === null ? 2.4 : Math.max(0.45, Math.min(3, 240000 / Math.max(rate, 1)));
    el.style.setProperty("--spin", spin.toFixed(2) + "s");
  }

  function hit(el, amount) {
    retrigger(el, "hit");
    spawn(el, "score-pop", "+" + compact(amount));
  }

  function banner(el, text) {
    spawn(el, "banner", text);
  }

  /* --------------------------------------------------------------- DMD */

  function createDmd(root) {
    if (!root) return { setScore() {}, say() {}, takeover() {} };
    const score = $(root, ".dmd-score");
    const ticker = $(root, ".dmd-ticker");
    const take = $(root, ".dmd-takeover");
    const screen = $(root, ".dmd-screen");
    const queue = [];
    // The dot mask lets the score show through, so hide it during a takeover.
    take.addEventListener("animationend", () => screen.classList.remove("taking"));
    let shown = 0;
    let target = 0;
    let raf = 0;
    let busy = null;

    function frame() {
      shown += (target - shown) * 0.12;
      if (Math.abs(target - shown) < 1) shown = target;
      score.textContent = fmt(shown);
      raf = shown === target ? 0 : requestAnimationFrame(frame);
    }

    function pump() {
      if (busy || !queue.length) return;
      ticker.textContent = queue.shift();
      retrigger(ticker, "wipe");
      busy = window.setTimeout(() => {
        busy = null;
        pump();
      }, 1100);
    }

    return {
      setScore(value) {
        target = value;
        if (reducedMotion.matches) {
          shown = value;
          score.textContent = fmt(value);
        } else if (!raf) {
          raf = requestAnimationFrame(frame);
        }
      },
      say(text) {
        queue.push(text.toUpperCase());
        if (queue.length > 4) queue.shift();
        pump();
      },
      takeover(text) {
        take.textContent = text;
        screen.classList.add("taking");
        retrigger(take, "show");
      }
    };
  }

  /* -------------------------------------------- flippers, slings, plunger */

  function createMachine(root) {
    const flippers = [$(root, ".flipper-l"), $(root, ".flipper-r")].filter(Boolean);
    const slings = $$(root, ".sling");
    const lights = $$(root, ".inlane-light");
    const plunger = $(root, ".plunger");
    const ball = $(root, ".shot-ball");
    const shooter = $(root, ".shooter");
    let side = 0;

    return {
      flick() {
        if (!flippers.length) return;
        const f = flippers[side % flippers.length];
        side += 1;
        f.classList.add("up");
        window.setTimeout(() => f.classList.remove("up"), 150);
      },
      kick() {
        if (slings.length) retrigger(slings[Math.floor(Math.random() * slings.length)], "kick");
      },
      light() {
        lights.forEach((l) => l.classList.toggle("on", Math.random() < 0.5));
      },
      fire() {
        if (!plunger) return;
        plunger.classList.remove("release");
        plunger.classList.add("pulling");
        window.setTimeout(() => {
          plunger.classList.remove("pulling");
          plunger.classList.add("release");
          if (reducedMotion.matches || !ball.animate) return;
          const rise = shooter.clientHeight - 130;
          ball.animate(
            [
              { opacity: 1, transform: "translateY(0)" },
              { opacity: 1, transform: "translateY(" + -rise + "px)", offset: 0.85 },
              { opacity: 0, transform: "translateY(" + -(rise + 24) + "px)" }
            ],
            { duration: 700, easing: "cubic-bezier(.2,.8,.3,1)" }
          );
        }, 450);
      },
      onPlunge(fn) {
        if (plunger) plunger.addEventListener("click", fn);
      }
    };
  }

  /* -------------------------------------------------------------- sound */

  /*
   * The waiting chime: two short oscillator tones, so there is no audio file
   * and no CSP media-src. Browsers only start audio after a user gesture, so
   * the coin-door toggle (or the first click or key press on the page, when
   * sound was left on) creates and resumes the one AudioContext.
   */
  function createSound(button) {
    const Ctx = window.AudioContext || window.webkitAudioContext;
    let ctx = null;
    let last = 0;
    let wanted = remember("pinball.sound") === "on";

    const running = () => ctx !== null && ctx.state === "running";

    function render() {
      if (!button) return;
      button.setAttribute("aria-pressed", String(wanted));
      button.dataset.armed = String(!wanted || running());
      button.textContent = wanted && !running() ? "Arm sound" : "Sound";
    }

    function arm() {
      if (!Ctx) return;
      if (!ctx) ctx = new Ctx();
      if (ctx.state !== "running") ctx.resume().then(render, render);
      render();
    }

    function tone(freq, at) {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "triangle";
      osc.frequency.value = freq;
      gain.gain.setValueAtTime(0.0001, at);
      gain.gain.exponentialRampToValueAtTime(0.2, at + 0.015);
      gain.gain.exponentialRampToValueAtTime(0.0001, at + 0.18);
      osc.connect(gain).connect(ctx.destination);
      osc.start(at);
      osc.stop(at + 0.2);
    }

    if (button) {
      button.addEventListener("click", () => {
        // Stored on but not yet armed: this click arms it, it does not turn it off.
        wanted = !(wanted && running());
        remember("pinball.sound", wanted ? "on" : "off");
        if (wanted) arm();
        render();
      });
    }
    const armOnGesture = () => {
      if (wanted && !running()) arm();
    };
    document.addEventListener("pointerdown", armOnGesture);
    document.addEventListener("keydown", armOnGesture);
    render();

    return {
      // Returns whether a chime actually played.
      chime() {
        const now = Date.now();
        if (!wanted || !running() || now - last < CHIME_GAP_MS) return false;
        last = now;
        const t = ctx.currentTime;
        tone(880, t);
        tone(1318.5, t + 0.15);
        return true;
      }
    };
  }

  /* ---------------------------------------------------- live announcer */

  // Screen readers hear status changes, not every token burst.
  function createAnnouncer(el) {
    let last = 0;
    let timer = 0;
    return (text) => {
      if (!el) return;
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        el.textContent = text;
        last = Date.now();
      }, Math.max(0, 4000 - (Date.now() - last)));
    };
  }

  /* --------------------------------------------------- backglass drawer */

  function createDrawer() {
    document.body.insertAdjacentHTML("beforeend", DRAWER);
    const dlg = $(document, ".backglass");
    let openId = null;

    $(dlg, ".bg-close").addEventListener("click", () => dlg.close());
    dlg.addEventListener("click", (e) => {
      if (e.target === dlg) dlg.close();
    });
    dlg.addEventListener("close", () => {
      openId = null;
    });

    function stat(key, value) {
      $(dlg, '[data-stat="' + key + '"]').textContent = value;
    }

    function drawSpark(s) {
      const svg = $(dlg, ".spark");
      const values = s.history.length > 1 ? s.history : [s.context.used, s.context.used];
      const max = s.context.limit || Math.max(...values) * 1.15;
      const pts = values.map((v, i) => [(i / (values.length - 1)) * 300, 96 - (v / max) * 90]);
      const line = pts.map((p) => p[0].toFixed(1) + "," + p[1].toFixed(1)).join(" ");
      $(svg, ".spark-line").setAttribute("points", line);
      $(svg, ".spark-area").setAttribute("points", line + " 300,96 0,96");
      $(svg, ".spark-limit").style.setProperty("display", s.context.limit ? "inline" : "none");
      const marks = values
        .map((v, i) => (i > 0 && v < values[i - 1] * 0.6 ? pts[i][0] : null))
        .filter((x) => x !== null)
        .map((x) => '<line class="spark-save" x1="' + x.toFixed(1) + '" x2="' + x.toFixed(1) + '" y1="0" y2="96"/>');
      $(svg, ".spark-marks").innerHTML = marks.join("");
    }

    function drawBreakdown(s) {
      const box = $(dlg, ".breakdown");
      const note = $(dlg, ".bg-coins-note");
      box.replaceChildren();
      if (!s.coins) {
        note.textContent = "Cursor does not record token spend locally, so coins stay dark. Context is still measured.";
        return;
      }
      note.textContent = "Every turn re-sends the whole context, so coins grow much faster than context.";
      const total = totalCoins(s.coins) || 1;
      [["Input", s.coins.input], ["Cache read", s.coins.cacheRead], ["Cache write", s.coins.cacheWrite], ["Output", s.coins.output]]
        .forEach(([name, value]) => {
          const row = document.createElement("div");
          row.className = "bar";
          row.innerHTML = '<span></span><span class="bar-track"><span class="bar-fill"></span></span><span></span>';
          row.children[0].textContent = name;
          row.children[2].textContent = fmt(value);
          $(row, ".bar-fill").style.setProperty("--w", ((value / total) * 100).toFixed(1));
          box.append(row);
        });
    }

    function render(s) {
      const pct = contextPct(s);
      dlg.dataset.kind = s.kind;
      dlg.dataset.status = s.status;
      const cmd = $(dlg, ".resume-cmd");
      const strip = $(dlg, ".bumper-actions");
      // The command is shown here in full so it can be selected by hand
      // when the clipboard is refused.
      setActions(strip, s, () => getSelection().selectAllChildren(cmd));
      cmd.textContent = s.links && s.links.resume ? s.links.resume.command : "";
      cmd.hidden = !cmd.textContent;
      $(dlg, ".bg-actions").hidden = strip.hidden;
      $(dlg, ".bg-kicker").textContent = KIND_LABEL[s.kind] + " / " + s.model;
      $(dlg, ".bg-title").textContent = s.project;
      $(dlg, ".bg-session").textContent = s.title;
      setRamp($(dlg, ".ramp"), pct);
      $(dlg, ".bg-ctx").textContent =
        fmt(s.context.used) + " of " + (s.context.limit ? fmt(s.context.limit) : "an unknown limit") + " tokens in the window" +
        (pct === null ? "." : " (" + Math.round(pct) + "%).");
      drawSpark(s);
      setReels($(dlg, ".reels"), totalCoins(s.coins));
      drawBreakdown(s);
      stat("model", s.model);
      stat("status", statusText(s));
      stat("turns", s.turns);
      stat("compactions", s.compactions);
      stat("subagents", s.subagents.length);
      stat("pid", s.pid === null ? "--" : s.pid);
      stat("uptime", uptime(Date.now() - s.startedAt));
      stat("activity", ago(Date.now() - s.lastActivityAt));
    }

    return {
      open(s, selectCommand) {
        openId = s.id;
        if (!dlg.open) dlg.showModal();
        render(s);
        if (selectCommand) getSelection().selectAllChildren($(dlg, ".resume-cmd"));
      },
      update(s) {
        if (openId === s.id) render(s);
      }
    };
  }

  /* --------------------------------------------------------------- board */

  function createBoard(root, options) {
    const opts = options || {};
    const items = new Map();
    const lanes = {};
    KINDS.forEach((k) => {
      lanes[k] = $(root, '[data-lane="' + k + '"]');
    });
    const dmd = createDmd($(root, ".dmd"));
    const machine = createMachine(root);
    const announce = createAnnouncer(document.getElementById("announcer"));
    const drawer = createDrawer();
    const entering = new Set();
    const sound = opts.sound || { chime: () => false };
    const baseTitle = document.title;
    let totalsQueued = false;

    function rate(item) {
      const total = totalCoins(item.s.coins);
      if (total === null) return null;
      const now = Date.now();
      item.samples.push([now, total]);
      while (item.samples.length > 2 && now - item.samples[0][0] > RATE_WINDOW_MS) item.samples.shift();
      const [t0, v0] = item.samples[0];
      return now - t0 < 4000 ? null : ((total - v0) * 60000) / (now - t0);
    }

    function upsert(s, delay) {
      let item = items.get(s.id);
      if (!item) {
        const el = createBumper(s);
        $(el, ".bumper-hit").addEventListener("click", () => drawer.open(items.get(s.id).s));
        el.addEventListener("copyfailed", () => drawer.open(items.get(s.id).s, true));
        item = { s, el, samples: [] };
        items.set(s.id, item);
        if (delay !== null) {
          el.style.setProperty("--delay", delay + "s");
          el.classList.add("entering");
          el.addEventListener("animationend", () => el.classList.remove("entering"), { once: true });
        }
        $(lanes[s.kind], ".lane-body").append(el);
        sortLane(s.kind);
      } else {
        const before = level(contextPct(item.s));
        const wasWaiting = item.s.status === "waiting";
        item.s = s;
        if (wasWaiting !== (s.status === "waiting")) sortLane(s.kind);
        updateBumper(item.el, s);
        const after = level(contextPct(s));
        if ((before === "ok" || before === "warm") && (after === "hot" || after === "crit")) {
          banner(item.el, "TILT WARNING");
          dmd.say(s.project + " context " + Math.round(contextPct(s)) + "%");
          announce(s.project + " context above 80 percent");
        }
      }
      setRate(item.el, s, rate(item));
      drawer.update(s);
      queueTotals();
    }

    function remove(id) {
      const item = items.get(id);
      if (!item) return;
      items.delete(id);
      item.el.remove();
      queueTotals();
    }

    // Waiting cards go first in their lane; everything else keeps its order.
    // The DOM is reordered (not CSS order) so tab order matches what is seen.
    function sortLane(kind) {
      const body = $(lanes[kind], ".lane-body");
      const cards = Array.from(body.children);
      const sorted = cards
        .filter((c) => c.dataset.status === "waiting")
        .concat(cards.filter((c) => c.dataset.status !== "waiting"));
      if (sorted.every((c, i) => c === cards[i])) return;
      const focused = document.activeElement;
      body.append(...sorted);
      if (focused && body.contains(focused) && document.activeElement !== focused) {
        focused.focus({ preventScroll: true });
      }
    }

    function queueTotals() {
      // Not batched into a frame: hidden tabs get no frames, and the title
      // is what a hidden tab shows.
      let waiting = 0;
      items.forEach((i) => {
        if (i.s.status === "waiting") waiting += 1;
      });
      document.title = (waiting ? "(" + waiting + ") WAITING / " : "") + baseTitle;
      if (totalsQueued) return;
      totalsQueued = true;
      requestAnimationFrame(() => {
        totalsQueued = false;
        renderTotals();
      });
    }

    function renderTotals() {
      const all = Array.from(items.values()).map((i) => i.s);
      let grand = 0;
      const rows = [];
      KINDS.forEach((k) => {
        const list = all.filter((s) => s.kind === k);
        const live = list.filter((s) => s.status !== "ended");
        const coins = list.map((s) => totalCoins(s.coins)).filter((v) => v !== null);
        const sum = coins.reduce((a, b) => a + b, 0);
        const pcts = live.map(contextPct).filter((p) => p !== null);
        const avg = pcts.length ? pcts.reduce((a, b) => a + b, 0) / pcts.length : null;
        grand += sum;
        const lane = lanes[k];
        $(lane, ".lane-meta").textContent = live.length + (live.length === 1 ? " ball" : " balls") + " in play";
        $(lane, ".lane-empty").hidden = list.length > 0;
        const player = $(root, '.player[data-kind="' + k + '"] .player-score');
        if (player) player.textContent = coins.length ? fmt(sum) : "N/A";
        rows.push([k, live.length, coins.length ? fmt(sum) : "n/a", avg === null ? "--" : Math.round(avg) + "%"]);
      });
      dmd.setScore(grand);
      const balls = all.filter((s) => s.status !== "ended").length;
      const ballsEl = $(root, ".balls-count");
      if (ballsEl) ballsEl.textContent = balls;
      const body = $(root, ".totals tbody");
      if (body) {
        body.replaceChildren(
          ...rows.map(([k, live, sum, avg]) => {
            const tr = document.createElement("tr");
            tr.dataset.kind = k;
            tr.innerHTML = '<td><span class="dot"></span><span></span></td><td></td><td></td><td></td>';
            tr.children[0].lastChild.textContent = KIND_LABEL[k];
            tr.children[1].textContent = live;
            tr.children[2].textContent = sum;
            tr.children[3].textContent = avg;
            return tr;
          })
        );
      }
    }

    function onEvent(e) {
      const item = items.get(e.sessionId);
      const name = item ? item.s.project : "";
      machine.flick();
      switch (e.kind) {
        case "tokens":
          if (!item) break;
          hit(item.el, e.amount);
          machine.kick();
          dmd.say(KIND_LABEL[item.s.kind] + " " + name + " +" + fmt(e.amount));
          break;
        case "turn_end":
          machine.light();
          dmd.say(name + " turn " + e.turns);
          break;
        case "compacted":
          if (item) banner(item.el, "BALL SAVED");
          dmd.takeover("BALL SAVED");
          dmd.say(name + " " + compact(e.preTokens) + " > " + compact(e.postTokens));
          announce(name + " compacted its context");
          break;
        case "launched":
          entering.add(e.sessionId);
          machine.fire();
          dmd.takeover(items.size >= 2 ? "MULTIBALL" : "BALL LAUNCHED");
          announce("New session started");
          break;
        case "status":
          onStatus(item, e.status);
          break;
        case "removed":
          remove(e.sessionId);
          break;
      }
    }

    function onStatus(item, status) {
      if (!item) return;
      const name = item.s.project;
      if (status === "error") dmd.takeover("TILT");
      if (status === "ended") dmd.takeover("GAME OVER");
      if (status === "stuck") dmd.say("ball stuck: " + name);
      if (status === "running") dmd.say(name + " back in play");
      if (status === "waiting") {
        onWaiting(item);
        return;
      }
      announce(name + " is " + STATUS_LABEL[status].toLowerCase());
    }

    // A status event is a transition, never the first snapshot, so this is
    // the only place the chime can fire.
    function onWaiting(item) {
      const s = item.s;
      const reason = REASON_SENTENCE[s.waitingReason] ? s.waitingReason : null;
      dmd.takeover("YOUR SHOT");
      dmd.say(s.project + " needs you");
      announce(s.project + " " + (reason ? REASON_SENTENCE[reason] : "is waiting for you"));
      if (reason !== "idle_prompt" || opts.chimeOnIdle()) sound.chime();
    }

    return {
      apply(msg) {
        if (msg.type === "board") {
          // Full snapshot (first connect or reconnect): reconcile, no fanfare
          // for sessions already on the table.
          const seen = new Set();
          msg.sessions.forEach((s, i) => {
            seen.add(s.id);
            upsert(s, items.has(s.id) ? null : i * 0.08);
          });
          Array.from(items.keys()).filter((id) => !seen.has(id)).forEach(remove);
        } else if (msg.type === "delta") {
          const isNew = entering.delete(msg.session.id);
          upsert(msg.session, isNew ? 0.55 : null);
        } else if (msg.type === "event") {
          onEvent(msg);
        }
      },
      dmd,
      machine
    };
  }

  /* ------------------------------------------------------------ motion */

  function startParallax() {
    const rootStyle = document.documentElement.style;
    let scrollQueued = false;
    window.addEventListener(
      "scroll",
      () => {
        if (scrollQueued) return;
        scrollQueued = true;
        requestAnimationFrame(() => {
          scrollQueued = false;
          rootStyle.setProperty("--scroll", Math.round(window.scrollY));
        });
      },
      { passive: true }
    );

    if (reducedMotion.matches || !window.matchMedia("(pointer: fine)").matches) return;
    let tx = 0;
    let ty = 0;
    let x = 0;
    let y = 0;
    let raf = 0;

    function frame() {
      x += (tx - x) * 0.08;
      y += (ty - y) * 0.08;
      rootStyle.setProperty("--px", x.toFixed(3));
      rootStyle.setProperty("--py", y.toFixed(3));
      raf = Math.abs(tx - x) > 0.002 || Math.abs(ty - y) > 0.002 ? requestAnimationFrame(frame) : 0;
    }

    window.addEventListener(
      "pointermove",
      (e) => {
        tx = (e.clientX / window.innerWidth) * 2 - 1;
        ty = (e.clientY / window.innerHeight) * 2 - 1;
        if (!raf) raf = requestAnimationFrame(frame);
      },
      { passive: true }
    );
  }

  // Tilting the playfield back leaves an empty band above it; pull the table
  // up by exactly that much so the backbox sits on the playfield.
  function fitTilt() {
    const table = $(document, ".table");
    const field = $(document, ".playfield");
    if (!table || !field) return;
    const measure = () => {
      table.style.setProperty("--lift", "0px");
      const lift = field.getBoundingClientRect().top - table.getBoundingClientRect().top;
      table.style.setProperty("--lift", Math.max(0, Math.round(lift)) + "px");
    };
    new ResizeObserver(measure).observe(field);
    field.addEventListener("transitionend", measure);
  }

  function remember(key, value) {
    try {
      if (value === undefined) return window.localStorage.getItem(key);
      window.localStorage.setItem(key, value);
    } catch (_) {
      // Storage can be blocked; the toggle simply will not persist.
    }
    return null;
  }

  /* -------------------------------------------------------------- pages */

  function bootTable() {
    const html = document.documentElement;
    let chimeOnIdle = remember("pinball.sound.idle") === "on";
    const sound = createSound($(document, '[data-toggle="sound"]'));
    const board = createBoard(document, { sound, chimeOnIdle: () => chimeOnIdle });
    const sim = window.PinballMock.createSimulator((msg) => board.apply(msg));
    const state = { paused: false, live: true };
    fitTilt();

    function sync() {
      // The stream stays connected while the tab is hidden, so a background
      // tab can still chime; the browser already pauses the animations.
      const shouldRun = !state.paused && state.live;
      if (shouldRun && !sim.running()) sim.start();
      if (!shouldRun && sim.running()) sim.stop();
      $$(document, "[data-live]").forEach((el) => {
        el.dataset.live = String(state.live);
      });
      $(document, ".link-state").textContent = state.live ? "LIVE" : "INSERT COIN";
    }

    function bindToggle(name, onChange) {
      const btn = $(document, '[data-toggle="' + name + '"]');
      btn.addEventListener("click", () => {
        const pressed = btn.getAttribute("aria-pressed") !== "true";
        btn.setAttribute("aria-pressed", String(pressed));
        onChange(pressed);
      });
      return btn;
    }

    const flatBtn = bindToggle("flat", (on) => {
      html.dataset.view = on ? "flat" : "tilt";
      remember("pinball.view", html.dataset.view);
    });
    if (remember("pinball.view") === "flat") {
      html.dataset.view = "flat";
      flatBtn.setAttribute("aria-pressed", "true");
    }
    const idleBtn = bindToggle("idle-chime", (on) => {
      chimeOnIdle = on;
      remember("pinball.sound.idle", on ? "on" : "off");
    });
    idleBtn.setAttribute("aria-pressed", String(chimeOnIdle));
    $(document, '[data-demo-wait]').addEventListener("click", () => {
      if (state.live) sim.wait();
    });
    bindToggle("pause", (on) => {
      state.paused = on;
      sync();
    });
    bindToggle("unplug", (on) => {
      state.live = !on;
      if (!state.live) board.dmd.say("connection lost");
      sync();
    });

    board.machine.onPlunge(() => {
      if (state.live) sim.launch();
    });
    document.addEventListener("keydown", (e) => {
      if (e.key === "n" && !e.metaKey && !e.ctrlKey && e.target === document.body) {
        retrigger($(document, ".table"), "nudge");
      }
    });
    sync();
  }

  function bootCatalog() {
    const base = window.PinballMock.sessions;
    const byId = (id) => JSON.parse(JSON.stringify(base.find((s) => s.id === id)));
    const drawer = createDrawer();
    const demos = new Map();

    function place(slot, s) {
      const el = createBumper(s);
      $(el, ".bumper-hit").addEventListener("click", () => drawer.open(s));
      el.addEventListener("copyfailed", () => drawer.open(s, true));
      setRate(el, s, s.coins ? 180000 : null);
      $(document, '[data-demo="' + slot + '"]').append(el);
      demos.set(slot, { el, s });
    }

    const statuses = {
      running: byId("claude-7f3a"),
      waiting: byId("claude-3c4d"),
      idle: byId("codex-4d18"),
      stuck: byId("claude-9e02"),
      error: byId("codex-b7c1"),
      ended: byId("cursor-5a1e")
    };
    Object.keys(statuses).forEach((k) => place("status-" + k, statuses[k]));

    [["ok", 0.32], ["warm", 0.68], ["hot", 0.86], ["crit", 0.97]].forEach(([lv, frac]) => {
      const s = byId("codex-0a55");
      s.status = "running";
      s.context.used = Math.round(s.context.limit * frac);
      s.project = "level " + lv;
      place("level-" + lv, s);
    });

    const cursor = byId("cursor-c0de");
    place("kind-cursor", cursor);

    const question = byId("codex-0a55");
    question.status = "waiting";
    question.waitingReason = "question";
    question.waitingSince = Date.now() - 12000;
    place("action-all", question);
    const resumeOnly = byId("claude-21bc");
    resumeOnly.links.editor = null;
    place("action-resume", resumeOnly);
    // Hostile hrefs the client must refuse even if the server sent them.
    const refused = byId("codex-4d18");
    refused.project = "links refused";
    refused.links = {
      editor: { app: "vscode", href: "javascript:alert(1)//vscode://file/x" },
      resume: null,
      app: { label: "Open in Codex app", href: "https://example.invalid/codex://threads/x" }
    };
    place("action-refused", refused);
    const unknown = byId("cursor-5a1e");
    unknown.status = "running";
    unknown.project = "limit unknown";
    place("kind-unknown", unknown);

    $$(document, "[data-action]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const demo = demos.get(btn.dataset.for);
        const act = btn.dataset.action;
        if (act === "hit") hit(demo.el, 18204);
        if (act === "saved") banner(demo.el, "BALL SAVED");
        if (act === "warning") banner(demo.el, "TILT WARNING");
        if (act === "turn") {
          demo.s.turns += 1;
          updateBumper(demo.el, demo.s);
        }
      });
    });

    // Standalone pieces.
    $$(document, "[data-demo-insert]").forEach((el) => {
      el.innerHTML = INSERT;
      setInsert($(el, ".insert"), el.dataset.demoInsert);
    });

    const reels = $(document, "[data-demo-reels]");
    reels.innerHTML = reelsMarkup();
    let reelValue = 4410550;
    setReels($(reels, ".reels"), reelValue);
    $(document, '[data-action-reels="add"]').addEventListener("click", () => {
      reelValue += Math.round(20000 + Math.random() * 400000);
      setReels($(reels, ".reels"), reelValue);
    });
    const darkReels = $(document, "[data-demo-reels-off]");
    darkReels.innerHTML = reelsMarkup();
    setReels($(darkReels, ".reels"), null);

    $$(document, "[data-demo-targets]").forEach((el) => {
      el.innerHTML = targetsMarkup();
      setTargets($(el, ".targets"), Number(el.dataset.demoTargets));
    });

    const ramp = $(document, "[data-demo-ramp]");
    ramp.innerHTML = RAMP;
    const slider = $(document, "[data-ramp-input]");
    const setFromSlider = () => setRamp($(ramp, ".ramp"), Number(slider.value));
    slider.addEventListener("input", setFromSlider);
    setFromSlider();

    const dmd = createDmd($(document, ".dmd"));
    dmd.setScore(12408331);
    dmd.say("Claude agent-pinball +18,204");
    $(document, '[data-action-dmd="say"]').addEventListener("click", () => dmd.say("Codex api-gateway turn 45"));
    $(document, '[data-action-dmd="takeover"]').addEventListener("click", () => dmd.takeover("BALL SAVED"));
    $(document, '[data-action-dmd="your-shot"]').addEventListener("click", () => dmd.takeover("YOUR SHOT"));

    const sound = createSound($(document, '[data-toggle="sound"]'));
    const chimeBtn = $(document, "[data-action-chime]");
    chimeBtn.addEventListener("click", () => {
      chimeBtn.textContent = sound.chime() ? "Chimed" : "Silent: arm sound, or wait 2 s";
      window.setTimeout(() => {
        chimeBtn.textContent = "Play chime";
      }, 1500);
    });

    const machine = createMachine(document);
    machine.onPlunge(() => machine.fire());
    $(document, '[data-action-machine="flip"]').addEventListener("click", () => {
      machine.flick();
      machine.kick();
      machine.light();
    });

    $(document, '[data-action-drawer]').addEventListener("click", () => drawer.open(byId("claude-21bc")));
    $(document, '[data-action-drawer-waiting]').addEventListener("click", () => drawer.open(byId("claude-3c4d")));

    $$(document, "[data-swatch]").forEach((el) => {
      el.style.setProperty("--sw", "var(" + el.dataset.swatch + ")");
      const label = document.createElement("code");
      label.textContent = el.dataset.swatch;
      el.append(label);
    });
  }

  document.addEventListener("DOMContentLoaded", () => {
    injectDefs();
    fillSlots(document);
    startParallax();
    if (document.body.dataset.page === "table") bootTable();
    if (document.body.dataset.page === "catalog") bootCatalog();
  });
})();
