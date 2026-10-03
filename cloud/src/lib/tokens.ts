import { createHash, randomBytes } from "node:crypto";

/**
 * Board tokens are what a machine — a desktop app, an agent, a CI job — sends
 * instead of a Clerk session. They are bearer secrets, so the rules below are
 * the whole security story:
 *
 *  - 32 random bytes, which is far beyond guessing;
 *  - a `hgr_` prefix so a leaked token is recognisable in a log or a secret
 *    scanner, and so a caller pasting the wrong string fails fast;
 *  - only the SHA-256 of the token is stored, so a database dump does not hand
 *    anyone a working credential.
 *
 * SHA-256 and not bcrypt/argon2 on purpose: the input is 256 bits of entropy,
 * not a human password, so there is nothing for a brute force to exploit and
 * we get a hash cheap enough to run on every request.
 */
export const TOKEN_PREFIX = "hgr_";

/** How often a token's `last_used_at` is refreshed, at most. */
export const TOKEN_TOUCH_INTERVAL_MS = 60_000;

export function generateBoardToken(): string {
  return `${TOKEN_PREFIX}${randomBytes(32).toString("base64url")}`;
}

/** Hashes the *full* token, prefix included — what is sent is what is hashed. */
export function hashBoardToken(token: string): string {
  return createHash("sha256").update(token, "utf8").digest("hex");
}

export function looksLikeBoardToken(value: string): boolean {
  return value.startsWith(TOKEN_PREFIX) && value.length > TOKEN_PREFIX.length;
}

/** Pulls the credential out of `Authorization: Bearer <token>`. */
export function parseBearer(header: string | null | undefined): string | null {
  if (!header) return null;
  const match = /^Bearer[ \t]+(.+)$/i.exec(header.trim());
  if (!match) return null;
  const token = match[1]?.trim();
  return token ? token : null;
}

/**
 * `last_used_at` is a diagnostic ("is this machine still using its token?"),
 * not an audit record. Writing it on every request would double the cost of
 * the busiest code path in the service — polling agents — for a value nobody
 * reads at that resolution, so it is refreshed once a minute at most.
 */
export function shouldTouchToken(
  lastUsedAt: number | null | undefined,
  now: number,
  interval = TOKEN_TOUCH_INTERVAL_MS,
): boolean {
  if (lastUsedAt === null || lastUsedAt === undefined) return true;
  return now - lastUsedAt >= interval;
}
