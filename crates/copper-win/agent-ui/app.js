// Copper's agent pane on Windows: one conversation with the agent in the
// window, the Mac's AgentPane drawn in the window's own palette. It talks to
// Copper only through window.copper (bridge v1): copper.agent.* for the
// conversation, page for the chip and suggestions, openUrl for links, and
// the built-in's own door (webkit.messageHandlers.copperBuiltin) for
// Settings › Intelligence and the page-context switch.
//
// Everything the agent or a page wrote goes in as text nodes (markdown.js
// builds elements, never HTML).
(function () {
  "use strict";

  var C = window.copper;
  var I = window.CopperIcons;
  var MD = window.CopperMarkdown;

  function $(id) { return document.getElementById(id); }
  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }

  // — what the page knows —

  var ui = {
    st: { available: false, busy: false, models: [], model: null, items: [] },
    // The bridge answered state() (else: no agent in this build).
    agentOk: false,
    page: null,
    pageContext: true,
    expanded: Object.create(null),
    atBottom: true,
    dev: false,
    demo: false,
    localError: "",
    menuOpen: false,
    opening: null,
    hello: false
  };
  var agent = null;

  // — the built-in's own door —

  var door = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.copperBuiltin;
  function tell(msg) {
    if (!door) return false;
    try { door.postMessage(JSON.stringify(msg)); return true; } catch (_) { return false; }
  }
  var helloWaiters = [];
  window.__copperBuiltin = {
    hello: function (h) {
      ui.hello = true;
      ui.dev = !!(h && h.dev);
      ui.demo = !!(h && h.demo) || (ui.dev && /[?&]demo=1\b/.test(location.search));
      if (h && typeof h.pageContext === "boolean") ui.pageContext = h.pageContext;
      var w = helloWaiters; helloWaiters = [];
      w.forEach(function (f) { f(); });
    },
    pageContext: function (on) {
      if (typeof ui.st.pageContext !== "boolean") ui.pageContext = !!on;
      renderComposer();
    }
  };
  function hello() {
    return new Promise(function (resolve) {
      if (!tell({ op: "hello" })) { resolve(); return; }
      helloWaiters.push(resolve);
      setTimeout(resolve, 1500);
    });
  }
  function openIntelligence() {
    if (!tell({ op: "settings", page: "intelligence" })) {
      ui.localError = "Open Settings › Intelligence to add an API key.";
      render();
    }
  }

  // — state —

  function models() { return Array.isArray(ui.st.models) ? ui.st.models : []; }
  function currentModel() {
    var ms = models();
    for (var i = 0; i < ms.length; i++) if (ms[i].id === ui.st.model) return ms[i];
    return null;
  }
  function ready() {
    if (!ui.agentOk || !ui.st.available) return false;
    var m = currentModel();
    return !m || m.ready !== false;
  }
  function items() { return Array.isArray(ui.st.items) ? ui.st.items : []; }
  function pageOn() {
    return typeof ui.st.pageContext === "boolean" ? ui.st.pageContext : ui.pageContext;
  }
  function busy() { return !!ui.st.busy; }

  function update(s) {
    if (!s || typeof s !== "object") return;
    var hadItems = items().length;
    ui.st = s;
    if (!items().length && hadItems) {
      ui.expanded = Object.create(null);
    }
    render();
  }

  // — the page in front —

  function pageKind() {
    var p = ui.page;
    var url = p && typeof p.url === "string" ? p.url : "";
    if (!url || url === "about:blank") return "blank";
    if (/^copper:\/\/canvas/i.test(url)) return "canvas";
    return "web";
  }
  function host(url) {
    var m = /^[a-z][a-z0-9+.-]*:\/\/([^/?#]+)/i.exec(url || "");
    return m ? m[1].replace(/^www\./, "") : "";
  }

  var SUGGEST = {
    blank: [["rectOnRect", "Sum up my Personal canvas"], ["squareOnSquare", "Which tabs do I have open?"], ["globe", "Find today's top story on Hacker News"]],
    canvas: [["textAlign", "Sum up this board"], ["grid", "Group these notes into frames by theme"], ["branch", "Draw arrows between related notes"], ["plusSquares", "Add a sticky for each next step"]],
    web: [["textAlign", "Summarise this page"], ["question", "What can I do here?"], ["link", "List the main links on this page"], ["table", "Pull the key facts into a table"]]
  };

  // — times —

  function when(at) {
    if (typeof at === "number") return at;
    var t = Date.parse(at || "");
    return isNaN(t) ? Date.now() : t;
  }
  // "4 s", "1 min 12 s" (DriveStyle.span).
  function span(seconds) {
    var t = Math.round(Math.max(seconds, 0));
    if (t < 60) return Math.max(t, 1) + " s";
    return t % 60 === 0 ? (t / 60) + " min" : Math.floor(t / 60) + " min " + (t % 60) + " s";
  }
  // "412 ms", "1.6 s" (DriveStyle.ms).
  function ms(n) {
    n = Math.round(n);
    return n < 1000 ? n + " ms" : (n / 1000).toFixed(1) + " s";
  }

  // — an activity's steps —

  function plain(tool) {
    tool = String(tool || "");
    var split = tool.indexOf("__");
    if (split > 0) return tool.slice(0, split) + " › " + tool.slice(split + 2).replace(/_/g, " ");
    return tool.replace(/_/g, " ");
  }

  var TOOL_ICON = {
    canvas_read: "eye", canvas_apply: "newChat", canvas_list: "rectOnRect", canvas_open: "rectOnRect",
    canvas_select: "scope", canvas_focus: "scope", canvas_create: "plusRect", canvas_invite: "personPlus",
    canvas_screenshot: "camera", browser_take_screenshot: "camera", jev_run: "sparkle", jev_step: "sparkle",
    jev_extract: "textViewfinder", jev_observe: "docSearch", browser_snapshot: "docSearch",
    browser_get_text: "docSearch", browser_find: "docSearch", browser_console_messages: "docSearch",
    browser_click: "cursorClick", browser_hover: "cursorClick", browser_drag: "cursorClick",
    browser_type: "keyboard", browser_fill_form: "keyboard", browser_press_key: "keyboard",
    browser_select_option: "keyboard", browser_navigate: "globe", browser_navigate_back: "globe",
    browser_navigate_forward: "globe", browser_tabs: "squareOnSquare", browser_close: "squareOnSquare",
    browser_groups: "squareOnSquare", browser_scroll: "arrowUpDown", browser_wait_for: "hourglass",
    browser_evaluate: "code", browser_sign_in: "key", browser_autofill: "key"
  };
  function toolIcon(tool) {
    return TOOL_ICON[tool] || (String(tool || "").indexOf("__") > 0 ? "puzzle" : "wrench");
  }
  // A Jev move, by its first word (DriveStep.icon).
  function moveIcon(text) {
    var t = String(text || "").toLowerCase();
    if (/^click|^press|^tap/.test(t)) return "cursorClick";
    if (/^typ|^fill|^key/.test(t)) return "keyboard";
    if (/^go|^open|^navigat/.test(t)) return "globe";
    if (/^read|^look|^observ/.test(t)) return "eye";
    if (/^scroll/.test(t)) return "arrowUpDown";
    if (/^wait/.test(t)) return "hourglass";
    if (/^select|^pick|^chose/.test(t)) return "list";
    if (/^done|^found/.test(t)) return "check";
    if (/^blocked/.test(t)) return "hand";
    if (/^ask/.test(t)) return "sparkle";
    return "circleDashed";
  }

  function bool(v, dflt) { return typeof v === "boolean" ? v : dflt; }
  function str(v) { return typeof v === "string" ? v : v == null ? "" : String(v); }

  // An activity item's detail: the pinned JSON shape, or plain lines.
  function activity(item) {
    var raw = item.detail;
    var d = null;
    if (raw && typeof raw === "object") d = raw;
    else if (typeof raw === "string" && /^\s*\{/.test(raw)) {
      try { d = JSON.parse(raw); } catch (_) { d = null; }
    }
    var out = { started: when(item.at), ended: null, current: "", steps: [] };
    if (d) {
      if (d.started) out.started = when(d.started);
      if (d.ended) out.ended = when(d.ended);
      out.current = str(d.current);
      (Array.isArray(d.steps) ? d.steps : []).forEach(function (s) {
        if (!s || typeof s !== "object") return;
        var jev = null;
        if (s.jev && typeof s.jev === "object") {
          jev = {
            cycles: (Array.isArray(s.jev.cycles) ? s.jev.cycles : []).map(function (c) {
              return { text: str(c && c.text), ms: Number(c && c.ms) || 0, ok: bool(c && c.ok, true), running: bool(c && c.running, false) };
            }),
            earlier: Number(s.jev.earlier) || 0,
            status: str(s.jev.status),
            failed: bool(s.jev.failed, false)
          };
        }
        out.steps.push({
          tool: str(s.tool), title: str(s.title), ms: Number(s.ms) || 0,
          ok: bool(s.ok, true) && !s.error, running: bool(s.running, false),
          warning: str(s.warning), error: str(s.error), aside: bool(s.aside, false), jev: jev,
          detail: str(s.detail)
        });
      });
    } else if (typeof raw === "string" && raw.trim()) {
      raw.split(/\r?\n/).forEach(function (line) {
        line = line.trim();
        if (!line) return;
        var failed = /^[✗×]\s*/.test(line);
        out.steps.push({ tool: "", title: line.replace(/^[✗×✓]\s*/, ""), ms: 0, ok: !failed, running: false, warning: "", error: "", aside: false, jev: null, detail: "" });
      });
    }
    if (!out.ended) {
      // No end said: the steps' own time, at least.
      var total = 0;
      out.steps.forEach(function (s) { total += s.ms; });
      out.ended = out.started + total;
    }
    return out;
  }

  // — the conversation's blocks (AgentPane.blocks) —

  function blocks() {
    var list = items();
    var out = [];
    var lastUser = -1;
    list.forEach(function (it) {
      if (!it || typeof it !== "object") return;
      var role = it.role;
      if (role === "user") { out.push({ kind: "user", item: it }); lastUser = out.length - 1; }
      else if (role === "assistant") out.push({ kind: "answer", item: it });
      // A note that is only context ("Stopped") comes as an activity
      // with nothing folded in it: a quiet line.
      else if (role === "activity") out.push({ kind: hasSteps(it) ? "activity" : "note", item: it });
      else if (role === "drive") out.push({ kind: "drive", item: it });
      else if (role === "error") out.push({ kind: quiet(it.text) ? "note" : "error", item: it });
      else out.push({ kind: "note", item: it });
    });
    if (busy()) {
      var running = false;
      for (var i = out.length - 1; i > lastUser; i--) {
        if (out[i].kind === "activity" && out[i].item.busy) { running = true; break; }
      }
      if (!running) out.push({ kind: "thinking" });
    }
    if (ui.localError) out.push({ kind: "error", item: { id: "local-error", text: ui.localError } });
    return out;
  }
  function hasSteps(it) {
    return typeof it.detail === "string" ? it.detail.trim().length > 0 : !!(it.detail && typeof it.detail === "object");
  }
  // A plain note is context and sits quietly; a failure is a callout.
  function quiet(text) {
    return /^(Stopped|Stopped by you|New chat)\.?$/.test(str(text).trim());
  }

  // — drawing —

  var view = Object.create(null); // block id → {el, sig}

  function blockId(b) {
    return b.kind === "thinking" ? "thinking" : b.kind + ":" + str(b.item.id);
  }

  function render() {
    renderHead();
    var empty = !items().length && !busy() && !ui.localError;
    $("empty").hidden = !empty;
    $("scroller").hidden = empty;
    if (empty) renderEmpty(); else renderTranscript();
    renderComposer();
  }

  function renderHead() {
    var ms = models();
    var m = currentModel();
    $("model").hidden = !ms.length;
    $("model-name").textContent = m ? m.name || m.id : ms.length ? "Model" : "";
    $("model-pill").title = "Model — " + (m ? (m.name || m.id) + (m.detail ? " · " + m.detail : "") : "not chosen");
    var nc = $("new-chat");
    var on = items().length > 0 || busy();
    nc.setAttribute("aria-disabled", on ? "false" : "true");
  }

  var lastEmpty = "";
  function renderEmpty() {
    var r = ready();
    var kind = pageKind();
    var sig = (r ? "r" : "u") + kind + (ui.agentOk ? 1 : 0);
    if (sig === lastEmpty) return;
    lastEmpty = sig;
    $("empty-title").textContent = r ? "What should we do?" : "Set up a model to start";
    $("empty-text").textContent = r
      ? "It reads the page, clicks and types for you, and draws on your canvases."
      : "Add an API key for a model gateway in Settings › Intelligence.";
    $("setup").hidden = r;
    var box = $("suggestions");
    box.textContent = "";
    box.hidden = !r;
    if (!r) return;
    SUGGEST[kind].forEach(function (s) {
      var b = el("button", "suggestion");
      b.type = "button";
      b.setAttribute("role", "listitem");
      var ic = el("span", "suggestion-icon");
      ic.appendChild(I.icon(s[0], 12, 1.9));
      b.appendChild(ic);
      b.appendChild(el("span", "suggestion-text", s[1]));
      var go = el("span", "suggestion-go");
      go.appendChild(I.icon("arrowUpRight", 9, 2.6));
      b.appendChild(go);
      b.addEventListener("click", function () { send(s[1]); });
      box.appendChild(b);
    });
  }

  function renderTranscript() {
    lastEmpty = "";
    var sc = $("scroller");
    var wasBottom = ui.atBottom;
    var list = blocks();
    var box = $("transcript");
    var seen = Object.create(null);
    var prev = null;
    list.forEach(function (b) {
      var id = blockId(b);
      seen[id] = true;
      var sig = signature(b);
      var v = view[id];
      if (!v || v.sig !== sig) {
        var fresh = draw(b);
        if (v && v.el.parentNode === box) box.replaceChild(fresh, v.el);
        v = view[id] = { el: fresh, sig: sig };
      }
      var want = prev ? prev.nextSibling : box.firstChild;
      if (want !== v.el) box.insertBefore(v.el, want);
      prev = v.el;
    });
    Object.keys(view).forEach(function (id) {
      if (!seen[id]) {
        if (view[id].el.parentNode === box) box.removeChild(view[id].el);
        delete view[id];
      }
    });
    var lastUser = list.length && list[list.length - 1].kind === "user";
    if (wasBottom || lastUser) toBottom(false);
    tick();
    scrolled();
    void sc;
  }

  function signature(b) {
    if (b.kind === "thinking") return "t|" + str(ui.st.status);
    var it = b.item;
    var s = b.kind + "|" + str(it.text) + "|" + str(it.at) + "|" + (it.busy ? 1 : 0) + "|" + (typeof it.detail === "string" ? it.detail : JSON.stringify(it.detail || null));
    if (b.kind === "activity") s += "|" + (ui.expanded[it.id] ? 1 : 0) + "|" + str(ui.st.status);
    return s;
  }

  function draw(b) {
    switch (b.kind) {
      case "user": return drawUser(b.item);
      case "answer": return drawAnswer(b.item);
      case "activity": return drawActivity(b.item);
      case "error": return drawError(b.item);
      case "note": return drawNote(b.item);
      case "drive": return drawDrive(b.item);
      default: return drawThinking();
    }
  }

  function drawUser(it) {
    var row = el("div", "user");
    row.appendChild(el("div", "bubble", str(it.text)));
    return row;
  }

  function drawAnswer(it) {
    var md = MD.render(str(it.text));
    md.classList.add("answer");
    return md;
  }

  function drawThinking() {
    var row = el("div", "thinking");
    var dots = el("span", "dots");
    dots.setAttribute("aria-hidden", "true");
    dots.appendChild(el("i")); dots.appendChild(el("i")); dots.appendChild(el("i"));
    row.appendChild(dots);
    row.appendChild(el("span", "quiet shimmer", str(ui.st.status) || "Thinking…"));
    return row;
  }

  function drawError(it) {
    var box = el("div", "callout");
    box.setAttribute("role", "alert");
    var ic = el("span", "callout-icon");
    ic.appendChild(I.icon("warnFill", 11));
    box.appendChild(ic);
    box.appendChild(el("span", "callout-text", str(it.text)));
    return box;
  }

  function drawNote(it) {
    var row = el("div", "note");
    row.appendChild(el("span", "note-text", str(it.text)));
    return row;
  }

  // An outside driver's run: who, how it stands, its last steps.
  function drawDrive(it) {
    var d = null;
    if (typeof it.detail === "string" && /^\s*\{/.test(it.detail)) { try { d = JSON.parse(it.detail); } catch (_) { d = null; } }
    var card = el("div", "card" + (it.busy ? " live" : ""));
    var head = el("div", "card-head");
    var hand = el("span", "hand");
    hand.appendChild(I.icon("sparkle", 10));
    head.appendChild(hand);
    var words = el("div", "card-words");
    words.appendChild(el("span", "card-title", str(it.text).split("\n")[0] || "An agent"));
    var plainDetail = !d && typeof it.detail === "string" ? it.detail.split("\n")[0] : "";
    var status = d && d.status ? str(d.status) : plainDetail || (it.busy ? "Driving" : "Done");
    words.appendChild(el("span", "card-status", status));
    head.appendChild(words);
    card.appendChild(head);
    var steps = d && Array.isArray(d.steps) ? d.steps.slice(-4) : [];
    if (steps.length) {
      var list = el("div", "jev-steps");
      list.style.paddingLeft = "0";
      steps.forEach(function (s) { list.appendChild(jevRow({ text: str(s.title || s.text), ms: Number(s.ms) || 0, ok: bool(s.ok, true), running: bool(s.running, false) })); });
      card.appendChild(list);
    }
    return card;
  }

  function drawActivity(it) {
    var a = activity(it);
    var running = !!it.busy;
    var open = !!ui.expanded[it.id];
    var steps = a.steps.filter(function (s) { return !s.aside; });
    var failures = steps.filter(function (s) { return !s.ok; }).length;
    var warnings = steps.filter(function (s) { return s.ok && s.warning; }).length;

    var box = el("div", "activity" + (open ? " open" : ""));
    var head = el("button", "activity-head");
    head.type = "button";
    head.setAttribute("aria-expanded", open ? "true" : "false");
    head.title = open ? "Hide the steps" : "Show the steps";
    if (running) {
      head.appendChild(I.ring(10, 1.5));
      head.appendChild(el("span", "what shimmer", current(a, steps)));
      var clock = el("span", "clock", "· " + span((Date.now() - a.started) / 1000));
      clock.setAttribute("data-since", String(a.started));
      head.appendChild(clock);
    } else {
      var st = el("span", "state" + (failures ? " bad" : ""));
      st.appendChild(I.icon(failures ? "warnFill" : "checkCircle", 11, 1.7));
      head.appendChild(st);
      var what = el("span", "what");
      what.appendChild(document.createTextNode("Worked for " + span((a.ended - a.started) / 1000) + " · " + steps.length + " step" + (steps.length === 1 ? "" : "s")));
      if (failures) what.appendChild(el("span", "bad", " · " + failures + " failed"));
      else if (warnings) what.appendChild(el("span", "bad", " · " + warnings + " with errors"));
      head.appendChild(what);
    }
    var chev = el("span", "chevron");
    chev.appendChild(I.icon("chevronRight", 8.5, 2.6));
    head.appendChild(chev);
    head.addEventListener("click", function () {
      if (ui.expanded[it.id]) delete ui.expanded[it.id];
      else { ui.expanded[it.id] = true; ui.opening = it.id; }
      render();
      ui.opening = null;
    });
    box.appendChild(head);

    if (open) {
      // Only the press that opens it slides the list in; a step landing
      // in an open list must not replay it.
      var wrap = el("div", "steps" + (ui.opening === it.id ? " arriving" : ""));
      var list = el("div", "steps-list");
      a.steps.forEach(function (s) { list.appendChild(s.aside ? drawAside(s) : drawStep(s)); });
      if (!a.steps.length) list.appendChild(el("div", "aside", running ? "Starting…" : "No steps."));
      wrap.appendChild(list);
      box.appendChild(wrap);
    }
    return box;
  }

  // The step in flight, in words.
  function current(a, steps) {
    if (a.current) return a.current;
    for (var i = steps.length - 1; i >= 0; i--) {
      var s = steps[i];
      if (!s.running) continue;
      var title = s.title || plain(s.tool);
      var cycles = s.jev && s.jev.cycles;
      var live = cycles && cycles.length && cycles[cycles.length - 1].running ? cycles[cycles.length - 1].text : "";
      return live ? title + " · " + live : title + "…";
    }
    return str(ui.st.status) || "Thinking…";
  }

  function drawAside(s) {
    var p = el("div", "aside");
    MD.inline(s.title, p);
    return p;
  }

  function drawStep(s) {
    var failed = !s.ok;
    var box = el("div", "step" + (failed ? " bad" : ""));
    var row = el("div", "step-row");
    var ic = el("span", "step-icon");
    ic.appendChild(I.icon(toolIcon(s.tool), 10.5, 1.8));
    row.appendChild(ic);
    var words = el("div", "step-words");
    words.appendChild(el("span", "step-title", s.title || plain(s.tool)));
    var why = s.warning || (failed ? s.error : "");
    if (why) words.appendChild(el("span", "step-why", why.replace(/^→\s*/, "")));
    row.appendChild(words);
    if (s.running) row.appendChild(I.ring(9, 1.4));
    else if (s.ms > 0) row.appendChild(el("span", "step-time", ms(s.ms)));
    // What it was asked and what came back (the Mac's tooltip).
    row.title = s.detail || plain(s.tool);
    box.appendChild(row);
    if (s.jev) box.appendChild(drawJev(s.jev, s.running));
    return box;
  }

  // A Jev run this step started: its moves, folded to lines, under it.
  function drawJev(j, live) {
    var box = el("div", "jev-steps");
    var shown = j.cycles.slice(-8);
    var earlier = j.earlier + (j.cycles.length - shown.length);
    if (earlier > 0) box.appendChild(el("div", "jev-more", earlier + " earlier step" + (earlier === 1 ? "" : "s")));
    shown.forEach(function (c) { box.appendChild(jevRow(c)); });
    if (!live && j.status) box.appendChild(el("div", "jev-status" + (j.failed ? " bad" : ""), j.status));
    return box;
  }

  function jevRow(c) {
    var row = el("div", "jev-step" + (c.ok ? "" : " bad"));
    var ic = el("span", "step-icon");
    ic.appendChild(I.icon(moveIcon(c.text), 10, 1.8));
    row.appendChild(ic);
    row.appendChild(el("span", "step-title" + (c.running ? " shimmer" : ""), c.text));
    if (c.running) row.appendChild(I.ring(9, 1.4));
    else if (c.ms > 0) row.appendChild(el("span", "step-time", ms(c.ms)));
    return row;
  }

  // The running clocks, once a second.
  function tick() {
    document.querySelectorAll(".clock[data-since]").forEach(function (c) {
      var t = "· " + span((Date.now() - Number(c.getAttribute("data-since"))) / 1000);
      if (c.textContent !== t) c.textContent = t;
    });
  }
  setInterval(tick, 1000);

  // — scrolling —

  function scrolled() {
    var sc = $("scroller");
    ui.atBottom = sc.scrollTop + sc.clientHeight >= sc.scrollHeight - 40;
    $("jump").hidden = ui.atBottom || $("scroller").hidden;
  }
  function toBottom(smooth) {
    var sc = $("scroller");
    sc.scrollTo({ top: sc.scrollHeight, behavior: smooth ? "smooth" : "auto" });
    ui.atBottom = true;
    $("jump").hidden = true;
  }

  // — the composer —

  var JEV = /^\/jev(\s|$)/;

  function draft() { return $("input").value; }
  function canSend() { return ready() && !busy() && draft().replace(JEV, "").trim().length > 0; }

  function placeholder() {
    if (!ready()) return "Add an API key to start";
    var k = pageKind();
    if (k === "blank") return "Ask anything, or say what to do…";
    return k === "canvas" ? "Ask about this canvas…" : "Ask about this page…";
  }

  function renderComposer() {
    var input = $("input");
    var r = ready();
    input.placeholder = placeholder();
    input.disabled = !r;
    // The page in front of every question.
    var chip = $("chip");
    var k = pageKind();
    if (k === "blank") {
      chip.hidden = true;
    } else {
      var on = pageOn();
      var p = ui.page || {};
      var canvas = k === "canvas";
      var name = canvas ? (str(p.title) || "Canvas") : (str(p.title) || host(p.url) || "This page");
      chip.hidden = false;
      chip.classList.toggle("off", !on);
      $("chip-text").textContent = name;
      var icon = $("chip-icon");
      icon.textContent = "";
      icon.appendChild(I.icon(on ? (canvas ? "rectOnRect" : "docText") : "eyeSlash", 10, 1.8));
      var what = canvas ? "canvas" : "page";
      chip.title = on
        ? "This " + what + " goes in front of every question — click to leave it out"
        : "Left out — click to put this " + what + " in front of every question";
      chip.setAttribute("aria-pressed", on ? "true" : "false");
      chip.setAttribute("aria-label", name + (on ? " — in front of every question" : " — left out"));
    }
    var jev = $("jev");
    var pressed = JEV.test(draft());
    jev.setAttribute("aria-pressed", pressed ? "true" : "false");
    jev.disabled = !r;
    var send = $("send");
    var b = busy();
    send.textContent = "";
    send.appendChild(I.icon(b ? "stop" : "arrowUp", b ? 10 : 12, 2.6));
    send.classList.toggle("on", b || canSend());
    send.title = b ? "Stop" : "Send (Return)";
    send.setAttribute("aria-label", b ? "Stop" : "Send");
    send.disabled = !b && !canSend();
  }

  function grow() {
    var input = $("input");
    input.style.height = "auto";
    var max = 18 * 8 + 2;
    var h = Math.min(input.scrollHeight, max);
    input.style.height = Math.max(h, 20) + "px";
    input.style.overflowY = input.scrollHeight > max ? "auto" : "hidden";
  }

  function send(text) {
    var t = String(text == null ? draft() : text).trim();
    if (!t || !ready() || busy()) return;
    var m = JEV.exec(t);
    if (m) {
      var goal = t.slice(m[0].length).trim();
      if (!goal) return;
      // The agent hands Jev the whole goal (jev_run).
      t = "Use Jev: " + goal;
    }
    if (text == null) { $("input").value = ""; grow(); }
    ui.localError = "";
    ui.atBottom = true;
    Promise.resolve(agent.send(t)).catch(function (e) {
      ui.localError = (e && e.message) || "The agent couldn't take that.";
      render();
    });
    renderComposer();
  }

  // — the model menu —

  function openMenu() {
    var menu = $("menu");
    menu.textContent = "";
    var cur = ui.st.model;
    models().forEach(function (m) {
      var b = el("button", "menu-item");
      b.type = "button";
      b.setAttribute("role", "menuitemradio");
      b.setAttribute("aria-checked", m.id === cur ? "true" : "false");
      var label = el("span", "label");
      label.appendChild(document.createTextNode(str(m.name) || str(m.id)));
      if (m.detail) label.appendChild(el("span", "detail", " — " + str(m.detail)));
      b.appendChild(label);
      var tick = el("span", "tick");
      if (m.id === cur) tick.appendChild(I.icon("check", 11, 2.4));
      b.appendChild(tick);
      if (m.ready === false) b.setAttribute("aria-disabled", "true");
      b.addEventListener("click", function () {
        closeMenu(true);
        if (m.id !== cur) Promise.resolve(agent.setModel(m.id)).catch(function () {});
      });
      menu.appendChild(b);
    });
    if (models().length) menu.appendChild(el("div", "menu-rule"));
    var acc = el("button", "menu-item");
    acc.type = "button";
    acc.setAttribute("role", "menuitem");
    acc.appendChild(el("span", "label", "Model access…"));
    acc.appendChild(el("span", "tick"));
    acc.addEventListener("click", function () { closeMenu(true); openIntelligence(); });
    menu.appendChild(acc);

    menu.hidden = false;
    placeMenu();
    ui.menuOpen = true;
    $("model-pill").setAttribute("aria-expanded", "true");
    var first = menu.querySelector('.menu-item[aria-checked="true"]') || menu.querySelector(".menu-item");
    if (first) first.focus();
  }

  // Under the pill, inside the pane.
  function placeMenu() {
    var menu = $("menu");
    var r = $("model-pill").getBoundingClientRect();
    var w = menu.offsetWidth;
    menu.style.left = Math.max(8, Math.min(r.left, window.innerWidth - w - 8)) + "px";
    menu.style.top = r.bottom + 5 + "px";
  }

  function closeMenu(refocus) {
    if (!ui.menuOpen) return;
    ui.menuOpen = false;
    $("menu").hidden = true;
    $("model-pill").setAttribute("aria-expanded", "false");
    if (refocus) $("model-pill").focus();
  }

  function menuKeys(e) {
    var all = Array.prototype.slice.call($("menu").querySelectorAll(".menu-item"));
    var i = all.indexOf(document.activeElement);
    if (e.key === "ArrowDown") { e.preventDefault(); all[(i + 1) % all.length].focus(); }
    else if (e.key === "ArrowUp") { e.preventDefault(); all[(i - 1 + all.length) % all.length].focus(); }
    else if (e.key === "Home") { e.preventDefault(); all[0].focus(); }
    else if (e.key === "End") { e.preventDefault(); all[all.length - 1].focus(); }
    else if (e.key === "Escape" || e.key === "Tab") { e.preventDefault(); closeMenu(true); }
  }

  // — focus —

  function focusComposer() {
    var input = $("input");
    if (!input.disabled) {
      input.focus();
      var n = input.value.length;
      input.setSelectionRange(n, n);
    } else {
      var b = $("setup-key");
      if (b && !$("setup").hidden && !$("empty").hidden) b.focus();
    }
  }

  // — wiring —

  function theme(t) {
    var dark = !!(t && t.dark);
    document.documentElement.setAttribute("data-theme", dark ? "dark" : "light");
  }

  function wire() {
    I.stamp(document);
    $("close").addEventListener("click", function () { C.ui.close().catch(function () {}); });
    $("new-chat").addEventListener("click", function () {
      if ($("new-chat").getAttribute("aria-disabled") === "true") return;
      ui.expanded = Object.create(null);
      ui.localError = "";
      Promise.resolve(agent.clear()).catch(function () {});
      focusComposer();
    });
    $("model-pill").addEventListener("click", function () { if (ui.menuOpen) closeMenu(true); else openMenu(); });
    $("model-pill").addEventListener("keydown", function (e) {
      if (e.key === "ArrowDown" && !ui.menuOpen) { e.preventDefault(); openMenu(); }
    });
    $("menu").addEventListener("keydown", menuKeys);
    document.addEventListener("mousedown", function (e) {
      if (ui.menuOpen && !$("menu").contains(e.target) && !$("model-pill").contains(e.target)) closeMenu(false);
    }, true);
    window.addEventListener("blur", function () { closeMenu(false); });
    window.addEventListener("resize", function () {
      if (ui.menuOpen) placeMenu();
      // Narrower or wider, the end of the conversation stays in view.
      if (ui.atBottom && !$("scroller").hidden) toBottom(false); else scrolled();
    });
    $("setup-key").addEventListener("click", openIntelligence);

    var input = $("input");
    input.addEventListener("input", function () { grow(); renderComposer(); });
    input.addEventListener("keydown", function (e) {
      if (e.key === "Enter" && !e.shiftKey && !e.altKey && !e.ctrlKey && !e.metaKey && !e.isComposing) {
        e.preventDefault();
        send();
      }
    });
    $("composer").addEventListener("mousedown", function (e) {
      if (e.target === $("composer") || e.target === $("composer").querySelector(".composer-row")) {
        e.preventDefault();
        focusComposer();
      }
    });
    $("send").addEventListener("click", function () {
      if (busy()) Promise.resolve(agent.stop()).catch(function () {});
      else send();
    });
    $("jev").addEventListener("click", function () {
      var v = input.value;
      input.value = JEV.test(v) ? v.replace(/^\/jev\s?/, "") : "/jev " + v.replace(/^\s+/, "");
      grow();
      renderComposer();
      focusComposer();
    });
    $("chip").addEventListener("click", function () {
      var on = !pageOn();
      ui.pageContext = on;
      tell({ op: "pageContext", on: on });
      renderComposer();
    });
    $("scroller").addEventListener("scroll", scrolled, { passive: true });
    $("jump").addEventListener("click", function () { toBottom(true); focusComposer(); });
    // Links in answers open in a new foreground tab.
    function link(e) {
      var a = e.target && e.target.closest ? e.target.closest("a[data-href]") : null;
      if (!a) return;
      e.preventDefault();
      if (e.type === "auxclick" && e.button !== 1) return;
      Promise.resolve(C.openUrl(a.getAttribute("data-href"))).catch(function () {});
    }
    $("transcript").addEventListener("click", link);
    $("transcript").addEventListener("auxclick", link);
    document.addEventListener("keydown", function (e) {
      if (e.key === "Escape" && busy() && document.activeElement === input) {
        Promise.resolve(agent.stop()).catch(function () {});
      }
    });
  }

  // The bridge's agent; with no agent in this build, one that says so.
  function realAgent() {
    return {
      state: function () { return C.agent.state(); },
      setModel: function (id) { return C.agent.setModel(id); },
      send: function (t) { return C.agent.send(t); },
      stop: function () { return C.agent.stop(); },
      clear: function () { return C.agent.clear(); },
      on: function (f) { C.on("agent", f); }
    };
  }

  async function start() {
    wire();
    try { theme((await C.info()).theme); } catch (_) {}
    C.on("theme", theme);
    C.on("visibility", function (v) { if (v && v.visible) focusComposer(); else closeMenu(false); });
    C.on("page", function (p) { ui.page = p || null; render(); });
    await hello();
    agent = ui.demo && window.CopperAgentDemo ? window.CopperAgentDemo.make() : realAgent();
    agent.on(function (s) { ui.agentOk = true; update(s); });
    try { ui.page = await C.page.current(); } catch (_) { ui.page = null; }
    try {
      var s = await agent.state();
      ui.agentOk = true;
      update(s);
    } catch (e) {
      ui.agentOk = false;
      render();
    }
    grow();
    focusComposer();
    document.documentElement.setAttribute("data-ready", "1");
  }

  // For the self-test and the demo: what is on screen.
  window.__agentUi = {
    state: function () { return ui; },
    blocks: function () { return blocks().map(function (b) { return b.kind; }); },
    expand: function (all) {
      items().forEach(function (it) { if (it.role === "activity") { if (all === false) delete ui.expanded[it.id]; else ui.expanded[it.id] = true; } });
      render();
    },
    menu: function (open) { if (open) openMenu(); else closeMenu(false); },
    draft: function (t) { if (t != null) { $("input").value = t; grow(); renderComposer(); } return $("input").value; },
    scrollTop: function () { $("scroller").scrollTop = 0; scrolled(); },
    focused: function () { return document.activeElement ? document.activeElement.id || document.activeElement.className : ""; },
    page: function (p) { ui.page = p; render(); }
  };

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start);
  else start();
})();
