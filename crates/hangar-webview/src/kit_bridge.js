/*
 * Hangar's kit bridge: what a kit's page was given by the frame it used to
 * live in (docs/kits.md, "The tab message protocol"), rebuilt for a page that
 * is a window of its own.
 *
 * A kit speaks to Hangar by `window.parent.postMessage({ hangar: 1, id,
 * method, params }, "*")`, and hears it by `message` events whose `source` is
 * `window.parent`. In a window of its own the page is its own parent, so
 * /_hangar/kit.js would answer every call with "unavailable". This script,
 * run by the helper before any script of the page, puts a stand-in where
 * `window.parent` was:
 *
 *   - `parent.postMessage(message)` hands the message, as JSON, to the helper
 *     (`window.ipc.postMessage`), which passes it to Hangar;
 *   - what Hangar posts back arrives as a `message` event on `window` whose
 *     `source` is the stand-in, so the check in kit.js, and in any kit that
 *     wrote its own, holds as it did.
 *
 * Nothing of the SDK is repeated here: kit.js runs as it is served, theme
 * handling and timeouts included. The page gains no power by this. It could
 * call `window.ipc.postMessage` itself, and Hangar reads what comes through
 * either way as what a page wrote.
 */
(function () {
  "use strict";
  var ipc = window.ipc;
  if (window.__hangarHost) return;
  if (!ipc || typeof ipc.postMessage !== "function") {
    console.error("Hangar: the kit bridge needs window.ipc, which this webview did not give the page");
    return;
  }
  // Kept now, so that a page replacing window.ipc later does not cut itself off.
  var send = ipc.postMessage.bind(ipc);

  var host = Object.freeze({
    postMessage: function (message) {
      // What a real postMessage does before it sends: copy by structured clone,
      // which refuses a function or a DOM node, as the frame's did.
      var copy = typeof structuredClone === "function" ? structuredClone(message) : message;
      var text = JSON.stringify(copy);
      if (typeof text === "string") send(text);
    },
  });

  try {
    Object.defineProperty(window, "parent", { value: host, configurable: true, enumerable: true });
  } catch (error) {
    console.error("Hangar: the kit bridge could not stand in for window.parent", error);
    return;
  }

  // The helper's way in, for `post_message`. Not enumerable, and not
  // replaceable: a page that calls it only sends a message to itself.
  Object.defineProperty(window, "__hangarHost", {
    value: Object.freeze({
      deliver: function (data) {
        var event = new MessageEvent("message", { data: data });
        // `source` has to be a Window for the constructor to take it, and the
        // stand-in is not one, so it is set on the event itself.
        Object.defineProperty(event, "source", { value: host });
        window.dispatchEvent(event);
      },
    }),
  });
})();
