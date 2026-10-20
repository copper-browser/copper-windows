// The pane's symbols: the Mac's SF Symbols, drawn as line icons on a 24-unit
// grid so they stay crisp at 9–17 px. Constants only — no page data reaches
// these strings.
(function () {
  "use strict";

  var S = {
    chevronDown: '<path d="M6 9l6 6 6-6"/>',
    chevronRight: '<path d="M9 5.5l6.5 6.5L9 18.5"/>',
    xmark: '<path d="M6.5 6.5l11 11M17.5 6.5l-11 11"/>',
    newChat: '<path d="M12 4H6.5A2.5 2.5 0 0 0 4 6.5v11A2.5 2.5 0 0 0 6.5 20h11a2.5 2.5 0 0 0 2.5-2.5V12"/><path d="M18.2 3.3a2 2 0 0 1 2.8 2.8l-8.3 8.3-3.6.9.9-3.6z"/>',
    sparkles: '<path class="f" d="M11 3c.6 4.7 2.8 6.9 7.5 7.5-4.7.6-6.9 2.8-7.5 7.5-.6-4.7-2.8-6.9-7.5-7.5C8.2 9.9 10.4 7.7 11 3z"/><path class="f" d="M18.5 14.5c.25 2 1.25 3 3.25 3.25-2 .25-3 1.25-3.25 3.25-.25-2-1.25-3-3.25-3.25 2-.25 3-1.25 3.25-3.25z"/><path class="f" d="M18.5 1.8c.2 1.5.9 2.2 2.4 2.4-1.5.2-2.2.9-2.4 2.4-.2-1.5-.9-2.2-2.4-2.4 1.5-.2 2.2-.9 2.4-2.4z"/>',
    sparkle: '<path class="f" d="M12 2.5c.65 5.2 3.3 7.85 8.5 8.5-5.2.65-7.85 3.3-8.5 8.5-.65-5.2-3.3-7.85-8.5-8.5 5.2-.65 7.85-3.3 8.5-8.5z"/>',
    arrowDown: '<path d="M12 4.5v15M6 13.5l6 6 6-6"/>',
    arrowUp: '<path d="M12 19.5v-15M6 10.5l6-6 6 6"/>',
    stop: '<rect class="f" x="6" y="6" width="12" height="12" rx="2.2"/>',
    terminal: '<rect x="3" y="4.5" width="18" height="15" rx="2.5"/><path d="M7 9.5l3 2.5-3 2.5M12.5 15h4.5"/>',
    check: '<path d="M5 12.5l4.5 4.5L19 7"/>',
    checkCircle: '<circle cx="12" cy="12" r="8.5"/><path d="M8.2 12.3l2.6 2.6 5-5.3"/>',
    warnFill: '<path class="f" fill-rule="evenodd" d="M10.3 4.1a2 2 0 0 1 3.4 0l8 13.9A2 2 0 0 1 20 21H4a2 2 0 0 1-1.7-3zM11 9h2v5.6h-2zM12 16.4a1.15 1.15 0 1 1 0 2.3 1.15 1.15 0 0 1 0-2.3z"/>',
    docText: '<path d="M14 3H7.5A2.5 2.5 0 0 0 5 5.5v13A2.5 2.5 0 0 0 7.5 21h9a2.5 2.5 0 0 0 2.5-2.5V8z"/><path d="M14 3v5h5M9 13h6M9 16.5h6"/>',
    rectOnRect: '<rect x="8" y="8" width="13" height="11" rx="2"/><path d="M4 15V6a2 2 0 0 1 2-2h10"/>',
    eye: '<path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z"/><circle cx="12" cy="12" r="3"/>',
    eyeSlash: '<path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z"/><circle cx="12" cy="12" r="3"/><path d="M4 3.5l16 17"/>',
    puzzle: '<path d="M10 4.5a2 2 0 0 1 4 0V6h3.5a1 1 0 0 1 1 1v3.5H20a2 2 0 0 1 0 4h-1.5V18a1 1 0 0 1-1 1H14v-1.5a2 2 0 0 0-4 0V19H6.5a1 1 0 0 1-1-1v-3.5H7a2 2 0 0 0 0-4H5.5V7a1 1 0 0 1 1-1H10z"/>',
    arrowUpRight: '<path d="M7 17L17 7M9 7h8v8"/>',
    globe: '<circle cx="12" cy="12" r="8.5"/><path d="M3.5 12h17M12 3.5c-2.6 2.6-3.5 5.4-3.5 8.5s.9 5.9 3.5 8.5c2.6-2.6 3.5-5.4 3.5-8.5S14.6 6.1 12 3.5z"/>',
    keyboard: '<rect x="2.5" y="6" width="19" height="12" rx="2.2"/><path d="M6.5 10h.01M10 10h.01M14 10h.01M17.5 10h.01M8 14h8"/>',
    cursorClick: '<path d="M10 10l4.6 10.6 1.7-4.5 4.5-1.7z"/><path d="M7.5 3l.8 2.8M3 7.5l2.8.8M13.5 4.2L11.6 6M4.2 13.5L6 11.6"/>',
    camera: '<path d="M4 7.5h3l1.8-2.5h6.4L17 7.5h3a1.5 1.5 0 0 1 1.5 1.5v9a1.5 1.5 0 0 1-1.5 1.5H4A1.5 1.5 0 0 1 2.5 18V9A1.5 1.5 0 0 1 4 7.5z"/><circle cx="12" cy="13" r="3.5"/>',
    docSearch: '<path d="M11 21H7.5A2.5 2.5 0 0 1 5 18.5v-13A2.5 2.5 0 0 1 7.5 3H14l5 5v2.5"/><path d="M14 3v5h5"/><circle cx="16" cy="16" r="3"/><path d="M18.2 18.2l2.6 2.6"/>',
    squareOnSquare: '<rect x="8.5" y="8.5" width="12" height="12" rx="2"/><path d="M15.5 8.5V5.5a2 2 0 0 0-2-2h-8a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h3"/>',
    arrowUpDown: '<path d="M12 3.5v17M8 7.5l4-4 4 4M8 16.5l4 4 4-4"/>',
    hourglass: '<path d="M6.5 3.5h11M6.5 20.5h11M7.5 3.5c0 4.8 4.5 5.6 4.5 8.5s-4.5 3.7-4.5 8.5M16.5 3.5c0 4.8-4.5 5.6-4.5 8.5s4.5 3.7 4.5 8.5"/>',
    code: '<path d="M8 7.5L3.5 12 8 16.5M16 7.5l4.5 4.5-4.5 4.5M13.8 4.5l-3.6 15"/>',
    key: '<circle cx="8" cy="15.5" r="4.5"/><path d="M11.2 12.3L20 3.5M17 6.5l2.5 2.5M14.6 8.9l2 2"/>',
    scope: '<circle cx="12" cy="12" r="7.5"/><circle cx="12" cy="12" r="2.5"/><path d="M12 2v3.5M12 18.5V22M2 12h3.5M18.5 12H22"/>',
    plusRect: '<rect x="8" y="8" width="13" height="11" rx="2"/><path d="M4 15V6a2 2 0 0 1 2-2h10M14.5 11v5M12 13.5h5"/>',
    personPlus: '<circle cx="9" cy="8" r="3.5"/><path d="M3 20c0-3.4 2.7-6 6-6s6 2.6 6 6M19 8v6M16 11h6"/>',
    textViewfinder: '<path d="M4 8V5.5A1.5 1.5 0 0 1 5.5 4H8M16 4h2.5A1.5 1.5 0 0 1 20 5.5V8M20 16v2.5a1.5 1.5 0 0 1-1.5 1.5H16M8 20H5.5A1.5 1.5 0 0 1 4 18.5V16M8 9.5h8M8 12h8M8 14.5h5"/>',
    wrench: '<path d="M14.6 6.4a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.4-3.4a5.5 5.5 0 0 1-7.3 7.3l-6.4 6.4a2 2 0 0 1-2.8-2.8l6.4-6.4a5.5 5.5 0 0 1 7.3-7.3z"/>',
    list: '<path d="M9 6.5h11M9 12h11M9 17.5h11M4.5 6.5h.01M4.5 12h.01M4.5 17.5h.01"/>',
    hand: '<path d="M17.5 11V6.5a1.75 1.75 0 0 0-3.5 0V11M14 10V4.5a1.75 1.75 0 0 0-3.5 0V10M10.5 10.5V6.5a1.75 1.75 0 0 0-3.5 0V14a7 7 0 0 0 14 0v-2.5a1.75 1.75 0 0 0-3.5 0"/>',
    textAlign: '<path d="M4 6.5h16M4 10.5h11M4 14.5h16M4 18.5h9"/>',
    question: '<circle cx="12" cy="12" r="8.5"/><path d="M9.6 9.6a2.5 2.5 0 0 1 4.8.9c0 1.7-2.4 2.2-2.4 3.7M12 17h.01"/>',
    link: '<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1"/><path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/>',
    table: '<rect x="3.5" y="4.5" width="17" height="15" rx="2"/><path d="M3.5 9.5h17M3.5 14.5h17M9.5 9.5v10M15 9.5v10"/>',
    grid: '<rect x="4" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="4" y="13.5" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.5"/>',
    branch: '<path d="M12 21v-6.5M12 14.5c0-3.5-6-4-6-8.5M12 14.5c0-3.5 6-4 6-8.5M3.8 8.2L6 6l2.2 2.2M15.8 8.2L18 6l2.2 2.2"/>',
    plusSquares: '<rect x="8" y="8" width="12.5" height="12.5" rx="2"/><path d="M15.5 8V5.5a2 2 0 0 0-2-2h-8a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2H8M14.25 11.5v6M11.25 14.5h6"/>',
    circleDashed: '<circle cx="12" cy="12" r="8" stroke-dasharray="3 3.3"/>',
    exclaim: '<path d="M12 5v9M12 18.5v.01"/>'
  };

  var NS = "http://www.w3.org/2000/svg";

  // An <svg> of `name` at `size` px; `weight` is the stroke in grid units
  // (1.7 ≈ the Mac's .medium at these sizes).
  function icon(name, size, weight) {
    var svg = document.createElementNS(NS, "svg");
    svg.setAttribute("viewBox", "0 0 24 24");
    svg.setAttribute("width", String(size || 12));
    svg.setAttribute("height", String(size || 12));
    svg.setAttribute("fill", "none");
    svg.setAttribute("stroke", "currentColor");
    svg.setAttribute("stroke-width", String(weight || 2));
    svg.setAttribute("stroke-linecap", "round");
    svg.setAttribute("stroke-linejoin", "round");
    svg.setAttribute("aria-hidden", "true");
    svg.setAttribute("focusable", "false");
    svg.classList.add("icon");
    svg.innerHTML = (S[name] || S.circleDashed).replace(/class="f"/g, 'fill="currentColor" stroke="none"');
    return svg;
  }

  // A ring that turns (the Mac's Ring: 0.78 of a circle).
  function ring(size, width) {
    var svg = document.createElementNS(NS, "svg");
    var r = (size - width) / 2;
    var c = 2 * Math.PI * r;
    svg.setAttribute("viewBox", "0 0 " + size + " " + size);
    svg.setAttribute("width", String(size));
    svg.setAttribute("height", String(size));
    svg.setAttribute("aria-hidden", "true");
    svg.classList.add("ring");
    var circle = document.createElementNS(NS, "circle");
    circle.setAttribute("cx", String(size / 2));
    circle.setAttribute("cy", String(size / 2));
    circle.setAttribute("r", String(r));
    circle.setAttribute("fill", "none");
    circle.setAttribute("stroke-width", String(width));
    circle.setAttribute("stroke-linecap", "round");
    circle.setAttribute("stroke-dasharray", (c * 0.78).toFixed(2) + " " + c.toFixed(2));
    svg.appendChild(circle);
    return svg;
  }

  // Fill every [data-icon] placeholder in the document once.
  function stamp(root) {
    var sizes = { xmark: 11, newChat: 12.5, sparkles: 19, chevronDown: 9, arrowDown: 10, arrowUp: 12, terminal: 12.5 };
    var weights = { arrowUp: 2.6, arrowDown: 2.4, chevronDown: 2.4, xmark: 2 };
    (root || document).querySelectorAll("[data-icon]").forEach(function (el) {
      var name = el.getAttribute("data-icon");
      el.textContent = "";
      el.appendChild(icon(name, sizes[name] || 12, weights[name]));
    });
  }

  window.CopperIcons = { icon: icon, ring: ring, stamp: stamp };
})();
