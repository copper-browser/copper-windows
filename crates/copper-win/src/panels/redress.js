// Copper (Windows): put a site's hidden things on the LIVE document.
//
// WebView2 user style sheets are document-created scripts that adopt a
// constructed sheet: adding or removing one only reaches documents made
// afterwards. The document on screen keeps the sheet it was born with, so
// a peeked or restored element stays `display: none` and a new hide does
// not show until a reload. This runs in the page world of the top frame:
//
// 1. drop the curtain's rules (`<selector> { display: none !important; }`,
//    one per selector) for `selectors` from every adopted sheet that is not
//    ours — only those exact rules, so other sheets' rules are untouched;
// 2. carry `css` (the site's sheet, `peek` left out) in one adopted sheet
//    of our own, kept last so its `!important` rules win as a user sheet's;
// 3. outline `peek` and scroll it to the middle (Mac `Tab.peek`), or clear
//    the outline.
//
// Arguments: css (string), selectors (string[]), peek (string|null).
const KEY = Symbol.for('copper.curtainLive');
let own = globalThis[KEY];
if (!own || own.doc !== document) {
  own = { doc: document, veil: new CSSStyleSheet(), mark: new CSSStyleSheet() };
  Object.defineProperty(globalThis, KEY, { value: own, configurable: true });
}
const canon = (sel) => {
  try {
    const t = new CSSStyleSheet();
    t.replaceSync(sel + ' {}');
    const r = t.cssRules[0];
    return r && r.selectorText;
  } catch (_) {
    return null;
  }
};
const drop = new Set(selectors.map(canon).filter(Boolean));
for (const sheet of document.adoptedStyleSheets) {
  if (sheet === own.veil || sheet === own.mark) continue;
  let rules;
  try { rules = sheet.cssRules; } catch (_) { continue; }
  for (let i = rules.length - 1; i >= 0; i--) {
    const r = rules[i];
    if (!(r instanceof CSSStyleRule) || !drop.has(r.selectorText)) continue;
    const s = r.style;
    if (s.length === 1 && s.getPropertyValue('display') === 'none'
        && s.getPropertyPriority('display') === 'important') {
      sheet.deleteRule(i);
    }
  }
}
try { own.veil.replaceSync(css); } catch (_) {}
// The picker's own sheets (`office-veil`, `office-peek`) say the same.
const sheetEl = (id) => document.getElementById(id);
if (sheetEl('office-veil')) sheetEl('office-veil').textContent = css;
if (sheetEl('office-peek')) sheetEl('office-peek').textContent = '';
try {
  own.mark.replaceSync(peek
    ? peek + ' { outline: 2px solid rgba(23,23,23,.9) !important; outline-offset: 2px !important; }'
    : '');
} catch (_) {}
document.adoptedStyleSheets = [
  ...document.adoptedStyleSheets.filter((s) => s !== own.veil && s !== own.mark),
  own.veil,
  own.mark,
];
if (peek) {
  try {
    const el = document.querySelector(peek);
    if (el) el.scrollIntoView({ block: 'center', behavior: 'smooth' });
    return !!el;
  } catch (_) {
    return false;
  }
}
return true;
