import { clerkMiddleware } from "@clerk/nextjs/server";

/**
 * Clerk v7 (Core 3): `clerkMiddleware` only *attaches* the auth state, it does
 * not protect anything. That is deliberate here — the API is entered two ways,
 * and a middleware that redirected anonymous traffic to a sign-in page would
 * break every agent holding a board token. Each route decides for itself, via
 * `requireUser` / `requireBoardAccess`, and answers 401 as JSON.
 *
 * `await auth()` only works in a request that went through this middleware, so
 * the matcher has to cover /api even though most of it is token-authenticated.
 */
export default clerkMiddleware();

export const config = {
  matcher: [
    // Everything except Next internals and static files…
    "/((?!_next|[^?]*\\.(?:html?|css|js(?!on)|jpe?g|webp|png|gif|svg|ttf|woff2?|ico|csv|docx?|xlsx?|zip|webmanifest)).*)",
    // …and always the API.
    "/api/(.*)",
  ],
};
