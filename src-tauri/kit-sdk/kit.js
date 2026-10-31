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
 */
(function () {
  "use strict";
  if (window.hangar) return;

  var host = window.parent;
  /** Long enough for any call Hangar answers; a call nobody answers fails. */
  var TIMEOUT_MS = 30000;
  var pending = new Map();
  var listeners = new Map();
  var next = 0;

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
      }, TIMEOUT_MS);
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

  Object.defineProperty(window, "hangar", {
    value: Object.freeze({ version: 1, call: call, on: on }),
    enumerable: true,
  });
})();
