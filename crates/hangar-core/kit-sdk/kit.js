/*
 * Hangar's kit SDK, served to every kit tab at /_hangar/kit.js (docs/kits.md).
 *
 *   const panes = await window.hangar.call("panes.list");
 *   const off = window.hangar.on("panes.changed", () => { ... });
 *
 * It only wraps the tab message protocol: a request posted to the frame's
 * parent, `{ hangar: 1, id, method, params }`, answered by Hangar with
 * `{ hangar: 1, id, result }` or `{ hangar: 1, id, error: { code, message } }`;
 * events arrive as `{ hangar: 1, event, data }`. A call fails with an Error
 * carrying the protocol's `code` ("denied", "invalid", ...).
 *
 * It also keeps the page in Hangar's theme: the `theme` event sets the tokens of
 * /_hangar/kit.css on the root element (docs/kit-ui.md). Without that event the
 * sheet's own values apply, and prefers-color-scheme decides between them.
 */
(function () {
  "use strict";
  if (window.hangar) return;

  var host = window.parent;
  /** Long enough for any call Hangar answers; a call nobody answers fails. */
  var TIMEOUT_MS = 30000;
  /** A declared command may run for 120 seconds, and is answered right after. */
  var COMMAND_TIMEOUT_MS = 130000;
  var pending = new Map();
  var listeners = new Map();
  var next = 0;

  /** The tokens the last `theme` event set, so that the next can take back what it leaves out. */
  var themed = [];

  function applyTheme(data) {
    if (!data || typeof data !== "object") return;
    var root = document.documentElement;
    var tokens = data.tokens && typeof data.tokens === "object" ? data.tokens : {};
    var names = Object.keys(tokens).filter(function (name) {
      return /^--kit-[a-z0-9-]+$/.test(name) && typeof tokens[name] === "string";
    });
    themed.forEach(function (name) {
      if (names.indexOf(name) < 0) root.style.removeProperty(name);
    });
    names.forEach(function (name) {
      root.style.setProperty(name, tokens[name]);
    });
    themed = names;
    if (data.scheme === "dark" || data.scheme === "light") {
      root.setAttribute("data-kit-scheme", data.scheme);
      root.style.colorScheme = data.scheme;
    }
    if (typeof data.locale === "string" && /^[A-Za-z]{2,3}(-[A-Za-z0-9-]+)*$/.test(data.locale)) {
      root.setAttribute("data-kit-locale", data.locale);
    }
  }

  function failure(code, message) {
    var error = new Error(message);
    error.code = code;
    return error;
  }

  window.addEventListener("message", function (event) {
    // Hangar is the frame's parent; nothing else speaks for it.
    if (host === window || event.source !== host) return;
    var message = event.data;
    if (!message || typeof message !== "object" || message.hangar !== 1) return;
    if (typeof message.event === "string") {
      if (message.event === "theme") applyTheme(message.data);
      var handlers = listeners.get(message.event);
      if (!handlers) return;
      Array.from(handlers).forEach(function (handler) {
        try {
          handler(message.data);
        } catch (error) {
          // One handler failing does not keep the others from hearing it.
          setTimeout(function () {
            throw error;
          });
        }
      });
      return;
    }
    var waiting = pending.get(message.id);
    if (!waiting) return;
    pending.delete(message.id);
    clearTimeout(waiting.timer);
    if (message.error && typeof message.error === "object") {
      waiting.reject(failure(String(message.error.code), String(message.error.message)));
    } else {
      waiting.resolve(message.result);
    }
  });

  function call(method, params) {
    return new Promise(function (resolve, reject) {
      if (host === window) {
        reject(failure("unavailable", "this page is not open as a tab in Hangar"));
        return;
      }
      if (typeof method !== "string" || !method) {
        reject(failure("invalid", "call needs a method name"));
        return;
      }
      var id = "k" + ++next;
      var timer = setTimeout(function () {
        pending.delete(id);
        reject(failure("timeout", "Hangar did not answer " + method));
      }, method === "command.run" ? COMMAND_TIMEOUT_MS : TIMEOUT_MS);
      pending.set(id, { resolve: resolve, reject: reject, timer: timer });
      var request = { hangar: 1, id: id, method: method };
      if (params !== undefined) request.params = params;
      try {
        // The frame has no origin of its own to name, and Hangar's is fixed.
        host.postMessage(request, "*");
      } catch (error) {
        pending.delete(id);
        clearTimeout(timer);
        reject(failure("invalid", "params cannot be sent: " + error.message));
      }
    });
  }

  function on(name, handler) {
    if (typeof name !== "string" || typeof handler !== "function") {
      throw failure("invalid", "on needs an event name and a function");
    }
    var handlers = listeners.get(name);
    if (!handlers) {
      handlers = new Set();
      listeners.set(name, handlers);
    }
    handlers.add(handler);
    return function off() {
      handlers.delete(handler);
    };
  }

  var sdk = { version: 1, call: call, on: on };
  // Filled by /_hangar/kit-ui.js, once: the rest of the object stays frozen.
  var ui;
  Object.defineProperty(sdk, "ui", {
    enumerable: true,
    get: function () {
      return ui;
    },
    set: function (value) {
      if (ui === undefined) ui = value;
    },
  });
  Object.defineProperty(window, "hangar", {
    value: Object.freeze(sdk),
    enumerable: true,
  });
})();
