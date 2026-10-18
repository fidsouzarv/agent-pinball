/*
 * Setup page: fills the decorative bulbs and chevrons, then polls
 * /api/setup and copies the server's view into the page. All values go
 * through textContent, dataset or CSSOM, so the strict CSP holds and agent
 * or machine text never becomes markup. The page reads fine without it.
 */
(function () {
  "use strict";

  const POLL_MS = 2000;
  const ANNOUNCE_GAP_MS = 4000;
  const $ = (root, sel) => root.querySelector(sel);
  const $$ = (root, sel) => Array.from(root.querySelectorAll(sel));

  function fillDecor() {
    $$(document, "[data-bulbs]").forEach((el) => {
      for (let i = 0; i < Number(el.dataset.bulbs); i += 1) {
        const b = document.createElement("span");
        b.className = "bulb";
        b.style.setProperty("--i", i);
        el.append(b);
      }
    });
    $$(document, ".chevrons").forEach((el) => {
      for (let i = 0; i < 3; i += 1) {
        const c = document.createElement("span");
        c.className = "chevron";
        c.style.setProperty("--i", 2 - i);
        el.append(c);
      }
    });
  }

  function ago(atMs) {
    const s = Math.max(0, Math.round((Date.now() - atMs) / 1000));
    if (s < 60) return s + "s ago";
    if (s < 3600) return Math.round(s / 60) + "m ago";
    return Math.round(s / 3600) + "h ago";
  }

  function createAnnouncer(el) {
    let last = 0;
    return (text) => {
      const now = Date.now();
      if (!el || now - last < ANNOUNCE_GAP_MS) return;
      last = now;
      el.textContent = text;
    };
  }

  function setLive(live) {
    const state = $(document, ".link-state");
    state.dataset.live = String(live);
    state.textContent = live ? "LIVE" : "INSERT COIN";
    $$(document, ".coin-slot").forEach((s) => { s.dataset.live = String(live); });
  }

  function setTicker(text, announce) {
    const ticker = $(document, '[data-slot="ticker"]');
    if (ticker.textContent === text) return;
    announce(text.toLowerCase());
    ticker.textContent = text;
    ticker.classList.remove("wipe");
    void ticker.offsetWidth; // restart the wipe animation
    ticker.classList.add("wipe");
  }

  function applyCheck(row, check) {
    const insert = $(row, ".insert");
    insert.dataset.check = check.state;
    $(row, ".check-label").textContent = check.label;
    $(row, ".check-detail").textContent = check.detail;
    const time = $(row, ".check-time");
    if (check.atMs) {
      time.dataset.atMs = String(check.atMs);
      time.dateTime = new Date(check.atMs).toISOString();
    } else {
      delete time.dataset.atMs;
    }
  }

  function applyAgent(agent, announce) {
    const lane = $(document, '[data-agent="' + agent.kind + '"]');
    if (!lane) return;
    const meta = $(lane, '[data-slot="meta"]');
    if (meta.textContent !== agent.meta) {
      meta.textContent = agent.meta;
      announce(lane.querySelector(".lane-name").textContent + ": " + agent.meta);
    }
    agent.checks.forEach((check) => {
      const row = $(lane, '[data-check-id="' + check.id + '"]');
      if (row) applyCheck(row, check);
    });
    const note = $(lane, '[data-slot="note"]');
    note.textContent = agent.note || "";
    note.hidden = !agent.note;
    const diff = $(lane, '[data-slot="diff"]');
    $(diff, "code").textContent = agent.diff || "";
    diff.hidden = !agent.diff;
  }

  function tickTimes() {
    $$(document, ".check-time").forEach((t) => {
      t.textContent = t.dataset.atMs ? ago(Number(t.dataset.atMs)) : "";
    });
  }

  function apply(view, announce) {
    $(document, '[data-slot="ready"]').textContent = view.ready + "/" + view.found;
    setTicker(view.ticker, announce);
    view.agents.forEach((agent) => applyAgent(agent, announce));
    $(document, '[data-slot="brief"]').textContent = view.agentBrief;
    tickTimes();
  }

  function startPolling(announce) {
    let timer = null;
    async function poll() {
      try {
        const res = await fetch("/api/setup", { cache: "no-store" });
        if (!res.ok) throw new Error(String(res.status));
        apply(await res.json(), announce);
        setLive(true);
      } catch (_) {
        setLive(false);
      }
      timer = setTimeout(poll, POLL_MS);
    }
    document.addEventListener("visibilitychange", () => {
      clearTimeout(timer);
      if (!document.hidden) poll();
    });
    poll();
  }

  function wireCopy() {
    $$(document, "[data-copy]").forEach((button) => {
      button.addEventListener("click", async () => {
        const source = document.getElementById(button.dataset.copy);
        try {
          await navigator.clipboard.writeText(source.textContent);
          button.textContent = "Copied";
        } catch (_) {
          getSelection().selectAllChildren(source);
          button.textContent = "Press Ctrl+C";
        }
        setTimeout(() => { button.textContent = "Copy brief"; }, 2000);
      });
    });
  }

  fillDecor();
  wireCopy();
  tickTimes();
  startPolling(createAnnouncer(document.getElementById("announcer")));
})();
