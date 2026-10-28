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
 * Push notifications (task M4) add their handlers here.
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
