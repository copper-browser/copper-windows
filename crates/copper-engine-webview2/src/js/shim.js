// Copper message-handler shim, page world (WebView2).
//
// Gives page scripts WKWebView's `window.webkit.messageHandlers.<name>
// .postMessage(body)` on top of WebView2's `chrome.webview.postMessage`.
// Each message travels as `{__copper: name, body}`; the engine routes it by
// name against its own allow-list, so this list only decides which handler
// names a script can see (as under WebKit, unregistered names are
// `undefined`). Installed as the first document-created script of every
// frame; `__COPPER_NAMES__` is the JSON array of names registered for the
// page world when the document was created. Later registrations reach the
// live document as a `{__copperNames: [...]}` web message.
(() => {
  const KEY = Symbol.for('copper.messageHandlers');
  if (globalThis[KEY]) return;
  const webview = globalThis.chrome && globalThis.chrome.webview;
  if (!webview || typeof webview.postMessage !== 'function') return;
  Object.defineProperty(globalThis, KEY, { value: true });
  const post = webview.postMessage.bind(webview);
  const listen = webview.addEventListener.bind(webview);
  const stringify = JSON.stringify;
  const parse = JSON.parse;
  const isArray = Array.isArray;
  const names = new Set(__COPPER_NAMES__);
  const cache = new Map();
  // WKScriptMessage bodies are JSON-like: undefined and functions vanish.
  const clean = (body) => {
    if (body === undefined) return null;
    const s = stringify(body);
    return s === undefined ? null : parse(s);
  };
  const handler = (name) => {
    let h = cache.get(name);
    if (!h) {
      h = Object.freeze({
        postMessage(body) {
          post({ __copper: name, body: clean(body) });
        },
      });
      cache.set(name, h);
    }
    return h;
  };
  const has = (p) => typeof p === 'string' && names.has(p);
  const handlers = new Proxy(Object.create(null), {
    get: (_, p) => (has(p) ? handler(p) : undefined),
    has: (_, p) => has(p),
    ownKeys: () => [...names],
    getOwnPropertyDescriptor: (_, p) =>
      has(p) ? { value: handler(p), enumerable: true, configurable: true, writable: false } : undefined,
    set: () => false,
    defineProperty: () => false,
    deleteProperty: () => false,
  });
  let defined = false;
  // `window.webkit` exists only while some handler does, as under WebKit.
  const define = () => {
    if (defined || names.size === 0 || 'webkit' in globalThis) return;
    defined = true;
    Object.defineProperty(globalThis, 'webkit', {
      value: Object.freeze({ messageHandlers: handlers }),
      configurable: true,
      enumerable: false,
      writable: false,
    });
  };
  define();
  listen('message', (e) => {
    const d = e && e.data;
    if (!d || typeof d !== 'object' || !isArray(d.__copperNames)) return;
    names.clear();
    for (const n of d.__copperNames) if (typeof n === 'string') names.add(n);
    define();
  });
})();
