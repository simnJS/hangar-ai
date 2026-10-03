import { requireUser } from "@/lib/auth";
import { json, route } from "@/lib/http";
import { serializeUser } from "@/lib/serialize";

/**
 * Route handlers read the session, a bearer token or the database — never a
 * build-time constant — so none of them may be prerendered. Declared on every
 * route module for the same reason: `next build` must not need a database.
 */
export const dynamic = "force-dynamic";

/**
 * The dashboard calls this first: it is what turns a Clerk identity into a row
 * other tables can point at.
 */
export const GET = route(async () => {
  const user = await requireUser({ refreshProfile: true });
  return json({ user: serializeUser(user) });
});
