// A scripted stand-in for copper.agent, for development and screenshots
// (COPPER_AGENT_UI_DEMO=1, or ?demo=1 in a debug build). Same shape as the
// bridge's agent: state(), setModel(), send(), stop(), clear() and the
// `agent` event. The conversations are the Mac's AgentPane bench seeds.
(function () {
  "use strict";

  var MODELS = [
    { id: "haiku", name: "Haiku", ready: true, detail: "Quick" },
    { id: "sonnet", name: "Sonnet", ready: true, detail: "The balance" },
    { id: "opus", name: "Opus", ready: true, detail: "Thinks hardest" }
  ];

  function iso(t) { return new Date(t).toISOString(); }

  function make() {
    var now = Date.now();
    var n = 0;
    var listeners = [];
    var timers = [];
    var st = { available: true, busy: false, models: MODELS.slice(), model: "sonnet", items: [], status: "" };

    function id() { return "demo-" + (++n); }
    function at(sec) { return iso(now + sec * 1000); }
    function emit() {
      var copy = JSON.parse(JSON.stringify(st));
      listeners.forEach(function (f) { try { f(copy); } catch (_) {} });
    }
    function user(text, t) { st.items.push({ id: id(), role: "user", text: text, at: at(t) }); }
    function answer(text, t) { st.items.push({ id: id(), role: "assistant", text: text, at: at(t) }); }
    function error(text, t) { st.items.push({ id: id(), role: "error", text: text, at: at(t) }); }
    function activity(t, steps, opts) {
      opts = opts || {};
      var d = { started: at(t), steps: steps };
      if (opts.ended != null) d.ended = at(opts.ended);
      if (opts.current) d.current = opts.current;
      var it = { id: id(), role: "activity", text: "", at: at(t), busy: !!opts.busy, detail: JSON.stringify(d) };
      st.items.push(it);
      return it;
    }
    function step(tool, title, ms, extra) {
      var s = { tool: tool, title: title, ms: ms, ok: true, running: false };
      for (var k in extra || {}) s[k] = extra[k];
      return s;
    }

    function summary() {
      user("Summarise this page", -190);
      activity(-189, [step("browser_get_text", "Read the page's text", 380)], { ended: -188.4 });
      answer("**Hacker News**, front page right now:\n\n" +
        "- **Copper ships one agent pane** — 412 points, 180 comments\n" +
        "- A deep dive into WebKit's new process model\n" +
        "- `sqlite-vec` reaches 1.0\n\n" +
        "Most of the talk under the first is about *seeing what an agent does* while it drives.", -184);
    }
    function monkeys() {
      user("create 100 notes with different monkey species and then draw lines between the closest related", -120);
      activity(-119, [
        step("canvas_read", "Read 5 shapes on Personal", 210),
        step("canvas_apply", "Changing the canvas", 0, { ok: false, error: "Cut off at the output limit — not run" })
      ], { ended: -95 });
      error("The model's reply ran past its output limit (16,000 tokens) while it was writing a canvas apply call, so that call was not run. Asking it to split the work into smaller calls.", -95);
      activity(-94, [
        step("", "Splitting it up: 40 species at a time, then the arrows.", 0, { aside: true }),
        step("canvas_apply", "40 operations applied on Personal", 1400),
        step("canvas_apply", "40 operations applied on Personal", 1300),
        step("canvas_apply", "20 operations applied on Personal", 800),
        step("canvas_apply", "58 operations applied, 2 failed on Personal", 1100, { warning: "2 of 60 not applied — op 7 (connect): `from`: no shape n107" }),
        step("canvas_apply", "2 operations applied on Personal", 300)
      ], { ended: -30 });
      answer("Done — **100 monkey species** on Personal, grouped by family:\n\n" +
        "1. Great apes and gibbons\n" +
        "2. Old World monkeys — macaques, baboons, colobus\n" +
        "3. New World monkeys — capuchins, howlers, marmosets\n\n" +
        "**60 arrows** join the closest relatives. Two named a note that wasn't there yet; I sent those again.", -29);
    }
    function snippet() {
      user("How do I list my open tabs from a terminal?", -20);
      activity(-19.5, [step("browser_tabs", "4 tabs open", 90)], { ended: -19.3 });
      answer("With the command-line client:\n\n" +
        "```\ncopper tabs\ncopper show 2\n```\n\n" +
        "`copper tabs` lists every tab with its number; `copper show 2` brings the second one to the front. " +
        "See [the agents guide](https://copper.example.com/docs/agents) for the rest.", -18);
    }
    function jevCycles(live) {
      var c = [
        { text: "Typed “JFK” into Where to?", ms: 1610, ok: true },
        { text: "Clicked Search", ms: 2270, ok: true },
        { text: "Clicked Nonstop only", ms: 1930, ok: true }
      ];
      if (live) c.push({ text: "Asking Jev", ms: 0, ok: true, running: true });
      else c.push({ text: "Done — found 3 under $300", ms: 700, ok: true });
      return c;
    }

    function reset() {
      timers.forEach(clearTimeout);
      timers = [];
      now = Date.now();
      st = { available: true, busy: false, models: MODELS.slice(), model: st.model || "sonnet", items: [], status: "" };
    }

    var scenes = {
      empty: function () {},
      unconfigured: function () {
        st.available = false;
        st.models = MODELS.map(function (m) { return { id: m.id, name: m.name, ready: false, detail: "Not set up" }; });
      },
      thinking: function () {
        summary();
        user("What can I do here?", -2);
        st.busy = true;
        st.status = "Thinking…";
      },
      running: function () {
        summary();
        user("Group these notes into frames by theme", -6);
        activity(-5.5, [
          step("canvas_read", "Read 105 shapes on Personal", 180),
          step("", "Five themes stand out — a frame for each.", 0, { aside: true }),
          step("canvas_apply", "Changing the canvas · 12 ops", 0, { running: true })
        ], { busy: true });
        st.busy = true;
        st.status = "Changing the canvas · 12 ops…";
      },
      jev: function () {
        summary();
        user("Find a nonstop flight SFO → JFK on Friday under $300", -14);
        activity(-13, [
          step("jev_run", "Jev: nonstop SFO → JFK, Friday, under $300", 0, { running: true, jev: { cycles: jevCycles(true), earlier: 0 } })
        ], { busy: true });
        st.busy = true;
        st.status = "Jev is driving…";
      },
      "jev-done": function () {
        summary();
        user("Find a nonstop flight SFO → JFK on Friday under $300", -40);
        activity(-39, [
          step("jev_run", "Jev: nonstop SFO → JFK, Friday, under $300", 8400, { jev: { cycles: jevCycles(false), earlier: 0, status: "Done — Found 3 under $300" } })
        ], { ended: -30.5 });
        answer("Three nonstops on Friday come in under **$300**:\n\n" +
          "- **JetBlue 916** — 7:05 → 15:40, $248\n" +
          "- **Alaska 12** — 9:30 → 18:05, $271\n" +
          "- **United 2210** — 13:15 → 21:52, $296\n\n" +
          "The results are open in your tab, filtered to nonstop.", -30);
      },
      chat: function () {
        summary();
        monkeys();
        snippet();
      },
      long: function () {
        summary();
        monkeys();
        snippet();
        summary();
        monkeys();
      }
    };

    function scene(name) {
      reset();
      (scenes[name] || scenes.empty)();
      emit();
      return name in scenes;
    }

    // A question played out: thinking, steps (with a Jev run), an answer.
    function play(text) {
      now = Date.now();
      user(text, 0);
      st.busy = true;
      st.status = "Thinking…";
      emit();
      var act = null;
      var steps = [];
      function later(ms, f) { timers.push(setTimeout(function () { f(); emit(); }, ms)); }
      function setSteps(busy, extra) {
        var d = { started: act.at, steps: steps };
        if (extra) for (var k in extra) d[k] = extra[k];
        act.detail = JSON.stringify(d);
        act.busy = busy;
      }
      later(900, function () {
        act = { id: id(), role: "activity", text: "", at: iso(Date.now()), busy: true, detail: "" };
        st.items.push(act);
        steps = [step("browser_snapshot", "Reading the page", 0, { running: true })];
        setSteps(true);
        st.status = "Reading the page…";
      });
      later(1900, function () {
        steps[0] = step("browser_snapshot", "Read 48 controls on the page", 640);
        steps.push(step("jev_run", "Jev: " + text, 0, { running: true, jev: { cycles: [{ text: "Reading the page", running: true, ok: true, ms: 0 }] } }));
        setSteps(true);
        st.status = "Jev is driving…";
      });
      later(3200, function () {
        steps[1].jev.cycles = [{ text: "Clicked Search", ok: true, ms: 1180 }, { text: "Asking Jev", running: true, ok: true, ms: 0 }];
        setSteps(true);
      });
      later(4600, function () {
        steps[1] = step("jev_run", "Jev: " + text, 2700, { jev: { cycles: [{ text: "Clicked Search", ok: true, ms: 1180 }, { text: "Done — found it", ok: true, ms: 420 }], status: "Done — found it" } });
        setSteps(false, { ended: iso(Date.now()) });
        answer("Done. I handed Jev the goal and it **found it** on the page — the result is open in your tab.", 4.6);
        st.busy = false;
        st.status = "";
      });
    }

    return (window.CopperAgentDemo.current = {
      state: function () { return Promise.resolve(JSON.parse(JSON.stringify(st))); },
      setModel: function (m) {
        if (!MODELS.some(function (x) { return x.id === m; })) return Promise.reject(new Error("No such model"));
        st.model = m;
        emit();
        return Promise.resolve(null);
      },
      send: function (text) {
        if (st.busy) return Promise.reject(new Error("The agent is busy"));
        play(String(text));
        return Promise.resolve(null);
      },
      stop: function () {
        if (!st.busy) return Promise.resolve(null);
        timers.forEach(clearTimeout);
        timers = [];
        st.items.forEach(function (it) {
          if (it.role === "activity" && it.busy) {
            it.busy = false;
            try {
              var d = JSON.parse(it.detail || "{}");
              (d.steps || []).forEach(function (s) { if (s.running) { s.running = false; s.ok = false; s.error = "Stopped"; } });
              d.ended = iso(Date.now());
              it.detail = JSON.stringify(d);
            } catch (_) {}
          }
        });
        error("Stopped", 0);
        st.busy = false;
        st.status = "";
        emit();
        return Promise.resolve(null);
      },
      clear: function () {
        reset();
        emit();
        return Promise.resolve(null);
      },
      on: function (f) { listeners.push(f); },
      scene: scene,
      scenes: Object.keys(scenes)
    });
  }

  window.CopperAgentDemo = { make: make, current: null };
})();
