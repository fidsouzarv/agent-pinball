/*
 * Fake sessions and a simulated event stream for the design prototype.
 *
 * The objects mirror the JSON the real server will send over SSE
 * (see docs/SYSTEM_DESIGN.md, "HTTP surface"):
 *   { type: "board", sessions: [Session] }        on connect
 *   { type: "delta", session: Session }           whenever a session changes
 *   { type: "event", kind, sessionId, ... }       moments worth animating
 * so pinball.js can later swap this file for an EventSource unchanged.
 *
 * Paths, session ids and links below are fake. The real server builds every
 * link itself and percent-encodes each path segment.
 */
(function () {
  "use strict";

  var NOW = Date.now();
  var MIN = 60 * 1000;

  function history(start, end, points, saveAt) {
    var out = [];
    for (var i = 0; i < points; i += 1) {
      var t = i / (points - 1);
      var v = start + (end - start) * Math.pow(t, 1.3);
      if (saveAt !== undefined && i >= saveAt) {
        v = v * 0.32;
      }
      out.push(Math.round(v));
    }
    return out;
  }

  var WAITING_REASONS = ["approval", "approval", "question", "idle_prompt"];
  var HOME = "/Users/dev/src/";

  function shellQuote(text) {
    return "'" + text.replace(/'/g, "'\\''") + "'";
  }

  // What the server sends in session.links: the editor for Claude and Codex
  // comes from config (vscode here), Cursor always opens in Cursor, only
  // Codex has an app link, Cursor has no resume command.
  function linksFor(s) {
    var dir = HOME + s.project;
    var thread = s.id.replace(/^[a-z]+-/, "") + "-0000-4000-8000-00000000fake";
    var editor = s.kind === "cursor" ? "cursor" : "vscode";
    return {
      editor: { app: editor, href: editor + "://file" + dir },
      resume: s.kind === "cursor" ? null : {
        command: "cd " + shellQuote(dir) + " && " + (s.kind === "claude" ? "claude --resume " : "codex resume ") + thread
      },
      app: s.kind === "codex" ? { label: "Open in Codex app", href: "codex://threads/" + thread } : null
    };
  }

  function withWaiting(s) {
    s.waitingSince = s.status === "waiting" ? s.waitingSince : null;
    s.waitingReason = s.status === "waiting" ? s.waitingReason : null;
    s.links = linksFor(s);
    return s;
  }

  // Coins are raw tokens. Cursor does not record them locally, hence null.
  var sessions = [
    {
      id: "claude-7f3a",
      kind: "claude",
      project: "agent-pinball",
      title: "system design for the dashboard",
      model: "claude-opus-5-5",
      status: "running",
      context: { used: 412000, limit: 1000000 },
      coins: { input: 1840221, output: 96420, cacheRead: 7412900, cacheWrite: 512300 },
      turns: 38,
      compactions: 0,
      subagents: [
        { id: "claude-7f3a-a1", status: "running" },
        { id: "claude-7f3a-a2", status: "ended" }
      ],
      pid: 48122,
      startedAt: NOW - 94 * MIN,
      lastActivityAt: NOW - 2000,
      history: history(18000, 412000, 24)
    },
    {
      id: "claude-21bc",
      kind: "claude",
      project: "cxnch-platform",
      title: "address validation pipeline",
      model: "claude-sonnet-5-5",
      status: "idle",
      context: { used: 341000, limit: 1000000 },
      coins: { input: 902118, output: 51880, cacheRead: 3120400, cacheWrite: 288100 },
      turns: 21,
      compactions: 1,
      subagents: [],
      pid: 39910,
      startedAt: NOW - 3 * 60 * MIN,
      lastActivityAt: NOW - 6 * MIN,
      history: history(22000, 980000, 24, 15)
    },
    {
      id: "claude-3c4d",
      kind: "claude",
      project: "pinball-hooks",
      title: "awaiting-input hook receiver",
      model: "claude-opus-5-5",
      status: "waiting",
      waitingSince: NOW - 40000,
      waitingReason: "approval",
      context: { used: 233000, limit: 1000000 },
      coins: { input: 610400, output: 40210, cacheRead: 2210300, cacheWrite: 190020 },
      turns: 15,
      compactions: 0,
      subagents: [],
      pid: 52044,
      startedAt: NOW - 28 * MIN,
      lastActivityAt: NOW - 41000,
      history: history(16000, 233000, 24)
    },
    {
      id: "claude-9e02",
      kind: "claude",
      project: "docs-site",
      title: "migrate guides to new layout",
      model: "claude-haiku-5-5",
      status: "stuck",
      context: { used: 176000, limit: 200000 },
      coins: { input: 512004, output: 22013, cacheRead: 1402200, cacheWrite: 120044 },
      turns: 17,
      compactions: 0,
      subagents: [{ id: "claude-9e02-a1", status: "stuck" }],
      pid: 51200,
      startedAt: NOW - 51 * MIN,
      lastActivityAt: NOW - 9 * MIN,
      history: history(9000, 176000, 24)
    },
    {
      id: "codex-0a55",
      kind: "codex",
      project: "api-gateway",
      title: "rate limiter refactor",
      model: "gpt-6.1-sol",
      status: "running",
      context: { used: 302000, limit: 475000 },
      coins: { input: 4410550, output: 140220, cacheRead: 3900100, cacheWrite: 0 },
      turns: 44,
      compactions: 0,
      subagents: [{ id: "codex-0a55-t1", status: "running" }],
      pid: 60211,
      startedAt: NOW - 70 * MIN,
      lastActivityAt: NOW - 1000,
      history: history(14000, 302000, 24)
    },
    {
      id: "codex-4d18",
      kind: "codex",
      project: "infra",
      title: "terraform drift check",
      model: "gpt-6.1-sol",
      status: "idle",
      context: { used: 51000, limit: 475000 },
      coins: { input: 210400, output: 9100, cacheRead: 150000, cacheWrite: 0 },
      turns: 6,
      compactions: 0,
      subagents: [],
      pid: 60211,
      startedAt: NOW - 22 * MIN,
      lastActivityAt: NOW - 4 * MIN,
      history: history(12000, 51000, 24)
    },
    {
      id: "codex-b7c1",
      kind: "codex",
      project: "billing-svc",
      title: "invoice rounding bug",
      model: "gpt-6.1-sol",
      status: "error",
      context: { used: 128000, limit: 475000 },
      coins: { input: 640300, output: 30120, cacheRead: 402000, cacheWrite: 0 },
      turns: 12,
      compactions: 0,
      subagents: [],
      pid: 60211,
      startedAt: NOW - 40 * MIN,
      lastActivityAt: NOW - 3 * MIN,
      history: history(11000, 128000, 24)
    },
    {
      id: "cursor-c0de",
      kind: "cursor",
      project: "web-app",
      title: "checkout form redesign",
      model: "composer-2",
      status: "running",
      context: { used: 88000, limit: 200000 },
      coins: null,
      turns: 14,
      compactions: 0,
      subagents: [],
      pid: 7012,
      startedAt: NOW - 33 * MIN,
      lastActivityAt: NOW - 3000,
      history: history(8000, 88000, 24)
    },
    {
      id: "cursor-5a1e",
      kind: "cursor",
      project: "mobile",
      title: "push notification settings",
      model: "claude-sonnet-5-5",
      status: "ended",
      context: { used: 64000, limit: null },
      coins: null,
      turns: 9,
      compactions: 0,
      subagents: [],
      pid: null,
      startedAt: NOW - 2 * 60 * MIN,
      lastActivityAt: NOW - 25 * MIN,
      history: history(6000, 64000, 24)
    }
  ].map(withWaiting);

  var NEW_PROJECTS = [
    { kind: "claude", project: "playground", model: "claude-opus-5-5", limit: 1000000 },
    { kind: "codex", project: "search-indexer", model: "gpt-6.1-sol", limit: 475000 },
    { kind: "cursor", project: "design-tokens", model: "composer-2", limit: 200000 },
    { kind: "claude", project: "cli-tools", model: "claude-sonnet-5-5", limit: 1000000 }
  ];

  function clone(value) {
    return JSON.parse(JSON.stringify(value));
  }

  function pick(list) {
    return list[Math.floor(Math.random() * list.length)];
  }

  function totalCoins(f) {
    return f ? f.input + f.output + f.cacheRead + f.cacheWrite : 0;
  }

  /*
   * Plays a believable session lifecycle: turns grow the context, each turn
   * bills roughly the whole context again (that is why coins outrun
   * context), compaction fires near the limit, sessions launch and end.
   */
  function createSimulator(emit) {
    var state = clone(sessions);
    var timer = null;
    var launched = 0;

    function find(id) {
      for (var i = 0; i < state.length; i += 1) {
        if (state[i].id === id) return state[i];
      }
      return null;
    }

    function pushHistory(s) {
      s.history.push(s.context.used);
      if (s.history.length > 40) s.history.shift();
    }

    function burst(s) {
      var limit = s.context.limit || 200000;
      var growth = Math.round(limit * (0.004 + Math.random() * 0.018));
      var output = Math.round(800 + Math.random() * 5200);
      s.context.used = Math.min(limit, s.context.used + growth);
      var billed = s.context.used + output;
      if (s.coins) {
        s.coins.cacheRead += Math.round(billed * 0.78);
        s.coins.input += Math.round(billed * 0.18);
        s.coins.cacheWrite += Math.round(billed * 0.04);
        s.coins.output += output;
      }
      s.lastActivityAt = Date.now();
      pushHistory(s);
      emit({ type: "delta", session: clone(s) });
      emit({ type: "event", kind: "tokens", sessionId: s.id, amount: s.coins ? billed : growth });

      if (s.context.used / limit > 0.93) {
        var pre = s.context.used;
        s.context.used = Math.round(limit * 0.12);
        s.compactions += 1;
        pushHistory(s);
        emit({ type: "delta", session: clone(s) });
        emit({ type: "event", kind: "compacted", sessionId: s.id, preTokens: pre, postTokens: s.context.used });
      }
    }

    function finishTurn(s) {
      s.turns += 1;
      emit({ type: "delta", session: clone(s) });
      emit({ type: "event", kind: "turn_end", sessionId: s.id, turns: s.turns });
    }

    function changeStatus(s, status, reason) {
      s.status = status;
      s.waitingSince = status === "waiting" ? Date.now() : null;
      s.waitingReason = status === "waiting" ? reason : null;
      if (status !== "waiting") s.lastActivityAt = Date.now();
      emit({ type: "delta", session: clone(s) });
      emit({ type: "event", kind: "status", sessionId: s.id, status: status });
    }

    function launch() {
      var tpl = NEW_PROJECTS[launched % NEW_PROJECTS.length];
      launched += 1;
      var s = {
        id: tpl.kind + "-n" + launched,
        kind: tpl.kind,
        project: tpl.project,
        title: "new session",
        model: tpl.model,
        status: "running",
        context: { used: Math.round(tpl.limit * 0.02), limit: tpl.limit },
        coins: tpl.kind === "cursor" ? null : { input: 4200, output: 300, cacheRead: 0, cacheWrite: 9000 },
        turns: 0,
        compactions: 0,
        subagents: [],
        pid: 70000 + launched,
        startedAt: Date.now(),
        lastActivityAt: Date.now(),
        history: [Math.round(tpl.limit * 0.02)]
      };
      withWaiting(s);
      state.push(s);
      emit({ type: "event", kind: "launched", sessionId: s.id });
      emit({ type: "delta", session: clone(s) });
      return s;
    }

    // Cursor has no waiting signal yet, so the demo only parks Claude and Codex.
    function wait(target, reason) {
      var s = target;
      if (!s) {
        var candidates = state.filter(function (x) { return x.status === "running" && x.kind !== "cursor"; });
        if (!candidates.length) return null;
        s = pick(candidates);
      }
      if (s.kind === "cursor") return null;
      changeStatus(s, "waiting", reason || pick(WAITING_REASONS));
      return s;
    }

    function retireEnded() {
      for (var i = state.length - 1; i >= 0; i -= 1) {
        var s = state[i];
        if (s.status === "ended" && Date.now() - s.lastActivityAt > 40000) {
          state.splice(i, 1);
          emit({ type: "event", kind: "removed", sessionId: s.id });
        }
      }
    }

    function step() {
      var running = state.filter(function (s) { return s.status === "running"; });
      var roll = Math.random();

      if (running.length && roll < 0.62) {
        burst(pick(running));
      } else if (running.length && roll < 0.76) {
        finishTurn(pick(running));
      } else if (roll < 0.84) {
        var idle = state.filter(function (s) {
          return s.status === "idle" || s.status === "stuck" || s.status === "waiting";
        });
        if (idle.length) changeStatus(pick(idle), "running");
      } else if (roll < 0.87 && running.length > 2) {
        wait(pick(running));
      } else if (roll < 0.9 && running.length > 2) {
        changeStatus(pick(running), pick(["idle", "idle", "stuck", "error"]));
      } else if (roll < 0.93) {
        var errored = state.filter(function (s) { return s.status === "error"; });
        if (errored.length) changeStatus(pick(errored), "ended");
      } else if (roll < 0.96 && state.length < 11) {
        launch();
      }
      retireEnded();
    }

    function schedule() {
      timer = window.setTimeout(function () {
        step();
        schedule();
      }, 650 + Math.random() * 900);
    }

    return {
      start: function () {
        emit({ type: "board", sessions: clone(state) });
        if (!timer) schedule();
      },
      stop: function () {
        window.clearTimeout(timer);
        timer = null;
      },
      running: function () {
        return timer !== null;
      },
      launch: launch,
      wait: function () {
        // A button press should be heard, so pick a reason that chimes.
        return wait(null, pick(["approval", "question"]));
      },
      find: find
    };
  }

  window.PinballMock = {
    sessions: sessions,
    createSimulator: createSimulator,
    totalCoins: totalCoins
  };
})();
