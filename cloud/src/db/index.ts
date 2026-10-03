import { neon } from "@neondatabase/serverless";
import { drizzle } from "drizzle-orm/neon-http";

import * as schema from "./schema";

type Database = ReturnType<typeof createDb>;

/**
 * Built on first use, never at import time.
 *
 * `next build` imports every route module to collect its exports, so a client
 * created at module scope would read DATABASE_URL during the build and fail on
 * any machine — or CI job — that has no database configured. Keeping the
 * client behind a function is what makes the build environment-free.
 *
 * A module-level `let` and not a Proxy on purpose: a Proxy around the drizzle
 * client breaks libraries that introspect it (instanceof checks, symbol
 * lookups, `util.inspect`), and the failures it produces are opaque.
 */
let cached: Database | undefined;

function createDb() {
  const url = process.env.DATABASE_URL;
  if (!url) {
    // Reached only at runtime, so the message is for an operator reading logs.
    throw new Error(
      "DATABASE_URL is not set. Run `vercel env pull .env.local` (see cloud/README.md).",
    );
  }
  // neon-http speaks to Neon over fetch: one round trip per statement, no
  // connection to keep alive, which is what a serverless function wants. The
  // trade-off is no interactive transactions — see src/lib/mutations.ts.
  return drizzle(neon(url), { schema });
}

export function getDb(): Database {
  cached ??= createDb();
  return cached;
}

/**
 * Postgres `unique_violation`. Used where a duplicate is a legitimate answer
 * to give the caller (a task id it chose is already taken) rather than a bug.
 */
export function isUniqueViolation(error: unknown): boolean {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "23505"
  );
}

export { schema };
