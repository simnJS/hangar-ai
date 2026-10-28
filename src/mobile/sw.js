/* Hangar Mobile's service worker: the app shell, kept on the phone.
 *
 * The build (vite.mobile.config.ts) writes this file at the site's root with
 * the list of the shell's files and a version drawn from them: a new build is
 * a new worker, which caches the new shell and drops the old one.
 *
 * The shell opens from the cache, so the app starts at once and shows what
 * it last knew even with the PC out of reach. Nothing under /m/ is ever
 * cached: the API and the shared files always come from the PC.
 *
 * It also shows the push notifications the PC sends
 * (src-tauri/src/remote/push.rs), app closed or not, and opens the screen one
 * is about when it is tapped.
 */

const SHELL = /*SHELL*/[];
const VERSION = /*VERSION*/"dev";
const CACHE = `hangar-shell-${VERSION}`;

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll(SHELL))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(keys.filter((key) => key.startsWith("hangar-shell-") && key !== CACHE).map((key) => caches.delete(key))),
      )
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") return;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin || url.pathname.startsWith("/m/")) return;

  // Any address of the app (a ?code= link, a #/pane/… from a notification)
  // is the one page.
  if (request.mode === "navigate") {
    event.respondWith(caches.match("./", { ignoreSearch: true }).then((page) => page ?? fetch(request)));
    return;
  }

  // The shell's files are named by their content: a cached one is current.
  // Anything else of the app's (a font for another script) is kept once seen.
  event.respondWith(
    caches.match(request).then(
      (cached) =>
        cached ??
        fetch(request).then((response) => {
          if (response.ok && url.pathname.includes("/assets/")) {
            const copy = response.clone();
            void caches.open(CACHE).then((cache) => cache.put(request, copy));
          }
          return response;
        }),
    ),
  );
});

/* ── Push notifications ──────────────────────────────────────────── */

/** A screen of the app from a notification's `url`; anything off this site is the app's start. */
function screenOf(url) {
  try {
    const target = new URL(typeof url === "string" ? url : "./#/", self.registration.scope);
    return target.origin === self.location.origin ? target.href : self.registration.scope;
  } catch {
    return self.registration.scope;
  }
}

// Every push is shown: a browser takes one that shows nothing for abuse, and
// stops delivering them.
self.addEventListener("push", (event) => {
  let message = {};
  try {
    message = event.data ? event.data.json() : {};
  } catch {
    message = { body: event.data ? event.data.text() : "" };
  }
  const title = typeof message.title === "string" && message.title ? message.title : "Hangar";
  event.waitUntil(
    self.registration.showNotification(title, {
      body: typeof message.body === "string" ? message.body : "",
      icon: "./icon-192.png",
      badge: "./icon-192.png",
      data: { url: screenOf(message.url) },
    }),
  );
});

// The app already open is brought forward and told the screen (hooks.ts,
// useNotificationOpens); otherwise it opens on it.
self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const url = screenOf(event.notification.data && event.notification.data.url);
  event.waitUntil(
    (async () => {
      const windows = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
      const open = windows.find((client) => client.focused) ?? windows[0];
      if (open) {
        try {
          await open.focus();
          open.postMessage({ type: "hangar:open", url });
          return;
        } catch {
          // It could not be brought forward: a window of its own, then.
        }
      }
      await self.clients.openWindow(url);
    })(),
  );
});

// The browser moved this phone's subscription: the PC is given the new one,
// in the phone's language, rather than pushing to the old one in vain.
self.addEventListener("pushsubscriptionchange", (event) => {
  event.waitUntil(
    (async () => {
      const answer = await fetch("/m/api/push/key", { credentials: "same-origin", cache: "no-store" });
      if (!answer.ok) return;
      const { key } = await answer.json();
      const base64 = key.replace(/-/g, "+").replace(/_/g, "/");
      const raw = atob(base64.padEnd(Math.ceil(base64.length / 4) * 4, "="));
      const subscription = await self.registration.pushManager.subscribe({
        userVisibleOnly: true,
        applicationServerKey: Uint8Array.from(raw, (char) => char.charCodeAt(0)),
      });
      const lang = /^fr(-|_|$)/i.test(self.navigator.language || "") ? "fr" : "en";
      await fetch("/m/api/push/subscribe", {
        method: "POST",
        credentials: "same-origin",
        headers: { "Content-Type": "application/json", "X-Hangar": "1" },
        body: JSON.stringify({ subscription: subscription.toJSON(), lang }),
      });
    })().catch(() => {
      // The next time the app opens, it subscribes again.
    }),
  );
});
