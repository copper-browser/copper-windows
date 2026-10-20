// The model's answer as it was meant to read (the Mac's MarkdownText):
// paragraphs, headings, lists, quotes, code blocks and rules, with bold,
// italics, `code` and links inside them. Built as DOM nodes from text —
// nothing the model writes is ever parsed as HTML.
(function () {
  "use strict";

  // Split into blocks exactly as the Mac does.
  function blocks(text) {
    var out = [];
    var paragraph = [];
    var code = null;
    function flush() {
      if (paragraph.length) out.push({ kind: "p", text: paragraph.join("\n") });
      paragraph = [];
    }
    var lines = String(text || "").replace(/\r\n?/g, "\n").split("\n");
    for (var i = 0; i < lines.length; i++) {
      var raw = lines[i];
      var line = raw.trim();
      if (line.indexOf("```") === 0) {
        if (code) {
          out.push({ kind: "code", text: code.join("\n") });
          code = null;
        } else {
          flush();
          code = [];
        }
        continue;
      }
      if (code) { code.push(raw); continue; }
      if (!line) { flush(); continue; }
      var indent = raw.match(/^[ \t]*/)[0].length;
      var depth = Math.min(Math.floor(indent / 2), 3);
      if (line === "---" || line === "***" || line === "___") { flush(); out.push({ kind: "rule" }); continue; }
      var h = line.match(/^(#{1,6}) (.*)$/);
      if (h) { flush(); out.push({ kind: "h", level: h[1].length, text: h[2].trim() }); continue; }
      var b = line.match(/^[-*+•] (.*)$/);
      if (b) { flush(); out.push({ kind: "li", marker: "•", depth: depth, text: b[1] }); continue; }
      var n = line.match(/^(\d+)[.)] (.*)$/);
      if (n) { flush(); out.push({ kind: "li", marker: n[1] + ".", depth: depth, text: n[2] }); continue; }
      if (line.charAt(0) === ">") { flush(); out.push({ kind: "quote", text: line.slice(1).trim() }); continue; }
      paragraph.push(line);
    }
    if (code) out.push({ kind: "code", text: code.join("\n") });
    flush();
    return out;
  }

  function isWeb(href) {
    return /^https?:\/\//i.test(href);
  }

  // Inline markdown into `into`: `code`, [links](https://…), <https://…>,
  // **bold** / __bold__, *italics* / _italics_. Unclosed marks are text.
  function inline(text, into) {
    var s = String(text || "");
    var i = 0;
    var buf = "";
    function emit() {
      if (buf) { into.appendChild(document.createTextNode(buf)); buf = ""; }
    }
    while (i < s.length) {
      var c = s.charAt(i);
      if (c === "\\" && i + 1 < s.length && /[\\`*_[\]()<>#+\-.!]/.test(s.charAt(i + 1))) {
        buf += s.charAt(i + 1);
        i += 2;
        continue;
      }
      if (c === "`") {
        var end = s.indexOf("`", i + 1);
        if (end > i + 1) {
          emit();
          var code = document.createElement("code");
          code.textContent = s.slice(i + 1, end);
          into.appendChild(code);
          i = end + 1;
          continue;
        }
      }
      if (c === "[") {
        var close = findClose(s, i + 1, "[", "]");
        if (close > i && s.charAt(close + 1) === "(") {
          var paren = findClose(s, close + 2, "(", ")");
          var href = paren > 0 ? s.slice(close + 2, paren).trim().split(/\s+/)[0] : "";
          if (paren > 0 && isWeb(href)) {
            emit();
            var a = link(href);
            inline(s.slice(i + 1, close), a);
            into.appendChild(a);
            i = paren + 1;
            continue;
          }
        }
      }
      if (c === "<") {
        var gt = s.indexOf(">", i + 1);
        var inner = gt > 0 ? s.slice(i + 1, gt) : "";
        if (gt > 0 && isWeb(inner) && !/\s/.test(inner)) {
          emit();
          var auto = link(inner);
          auto.textContent = inner;
          into.appendChild(auto);
          i = gt + 1;
          continue;
        }
      }
      if ((c === "*" || c === "_") && s.charAt(i + 1) === c) {
        var mark = c + c;
        var stop = s.indexOf(mark, i + 2);
        if (stop > i + 2 && s.charAt(i + 2) !== " ") {
          emit();
          var strong = document.createElement("strong");
          inline(s.slice(i + 2, stop), strong);
          into.appendChild(strong);
          i = stop + 2;
          continue;
        }
      }
      if (c === "*" || (c === "_" && (i === 0 || /[\s(]/.test(s.charAt(i - 1))))) {
        var j = i + 1;
        var found = -1;
        while (j < s.length) {
          var k = s.indexOf(c, j);
          if (k < 0) break;
          if (s.charAt(k + 1) === c) { j = k + 2; continue; }
          if (c === "_" && k + 1 < s.length && /\w/.test(s.charAt(k + 1))) { j = k + 1; continue; }
          found = k;
          break;
        }
        if (found > i + 1 && s.charAt(i + 1) !== " " && s.charAt(found - 1) !== " ") {
          emit();
          var em = document.createElement("em");
          inline(s.slice(i + 1, found), em);
          into.appendChild(em);
          i = found + 1;
          continue;
        }
      }
      buf += c;
      i++;
    }
    emit();
    return into;
  }

  function findClose(s, from, open, close) {
    var depth = 0;
    for (var i = from; i < s.length; i++) {
      var c = s.charAt(i);
      if (c === "\\") { i++; continue; }
      if (c === open) depth++;
      else if (c === close) {
        if (depth === 0) return i;
        depth--;
      }
    }
    return -1;
  }

  // Links open in a new foreground tab through the bridge (app.js listens).
  function link(href) {
    var a = document.createElement("a");
    a.setAttribute("href", href);
    a.setAttribute("data-href", href);
    a.setAttribute("title", href);
    return a;
  }

  function render(text) {
    var root = document.createElement("div");
    root.className = "md";
    blocks(text).forEach(function (b) {
      var el;
      switch (b.kind) {
        case "p":
          el = inline(b.text, document.createElement("p"));
          break;
        case "h":
          el = inline(b.text, document.createElement("h3"));
          el.className = b.level <= 1 ? "l1" : b.level === 2 ? "l2" : "l3";
          el.setAttribute("role", "heading");
          el.setAttribute("aria-level", String(Math.min(b.level + 2, 6)));
          break;
        case "li":
          el = document.createElement("div");
          el.className = "li";
          el.style.paddingLeft = b.depth * 16 + 2 + "px";
          var m = document.createElement("span");
          m.className = "marker" + (b.marker === "•" ? "" : " num");
          m.textContent = b.marker;
          el.appendChild(m);
          el.appendChild(inline(b.text, Object.assign(document.createElement("span"), { className: "li-text" })));
          break;
        case "quote":
          el = document.createElement("blockquote");
          el.appendChild(inline(b.text, document.createElement("span")));
          break;
        case "code":
          el = document.createElement("pre");
          var code = document.createElement("code");
          code.textContent = b.text;
          el.appendChild(code);
          break;
        case "rule":
          el = document.createElement("hr");
          break;
      }
      if (el) root.appendChild(el);
    });
    return root;
  }

  window.CopperMarkdown = { blocks: blocks, inline: inline, render: render };
})();
