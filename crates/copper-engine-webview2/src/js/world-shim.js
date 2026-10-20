// Copper message-handler shim, isolated worlds (WebView2 via CDP).
//
// `chrome.webview` does not exist in DevTools-created isolated worlds, so
// messages leave through a CDP binding (`Runtime.addBinding` with
// `executionContextName` = the world's name): calling the binding raises
// `Runtime.bindingCalled` with the string payload `{"__copper":name,"body":…}`.
// The binding is looked up at call time: V8 puts it into a new document's
// world only when the engine asks (by execution context id, at commit and
// `DOMContentLoaded`), which can be after document-created scripts ran, so
// messages wait in order until it is there. `__COPPER_NAMES__` is the JSON
// array of names registered for this world; `__COPPER_BINDING__` the
// binding's name. The world is private to Copper, so the name setter is a
// plain global here.
(() => {
  const KEY = Symbol.for('copper.messageHandlers');
  if (globalThis[KEY]) return;
  Object.defineProperty(globalThis, KEY, { value: true });
  const BINDING = __COPPER_BINDING__;
  const stringify = JSON.stringify;
  const names = new Set(__COPPER_NAMES__);
  const cache = new Map();
  const later = setTimeout;
  const held = [];
  let waiting = false;
  const flush = () => {
    const send = globalThis[BINDING];
    if (typeof send !== 'function') return false;
    while (held.length) send(held.shift());
    return true;
  };
  // Up to ~30 s, backing off; then what is held goes.
  const wait = (tries) => {
    later(() => {
      if (flush()) waiting = false;
      else if (tries < 150) wait(tries + 1);
      else {
        held.length = 0;
        waiting = false;
      }
    }, Math.min(10 + tries * 10, 250));
  };
  const handler = (name) => {
    let h = cache.get(name);
    if (!h) {
      h = Object.freeze({
        postMessage(body) {
          const s = body === undefined ? undefined : stringify(body);
          if (held.length < 1000) {
            held.push(stringify({ __copper: name, body: s === undefined ? null : JSON.parse(s) }));
          }
          if (!flush() && !waiting) {
            waiting = true;
            wait(0);
          }
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
  });
  Object.defineProperty(globalThis, 'webkit', {
    value: Object.freeze({ messageHandlers: handlers }),
    configurable: true,
  });
  Object.defineProperty(globalThis, '__copperSetNames', {
    value: (list) => {
      names.clear();
      for (const n of list) if (typeof n === 'string') names.add(n);
    },
  });
})();
