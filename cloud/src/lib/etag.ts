/**
 * Board polling runs on one number.
 *
 * Agents and the desktop app poll `GET /boards/:id` in a loop. `boards.rev` is
 * bumped by every mutation, so the ETag is just that revision: the server can
 * read one row, compare, and answer 304 without touching the task table at
 * all. Cheap enough that a short poll interval is not a problem.
 */
export function boardETag(rev: number): string {
  return `"${rev}"`;
}

/** Drops the weak validator marker so `W/"3"` and `"3"` compare equal. */
function normalize(value: string): string {
  const trimmed = value.trim();
  return trimmed.startsWith("W/") ? trimmed.slice(2) : trimmed;
}

/**
 * True when the client already holds the current revision.
 *
 * `If-None-Match` may carry a list (`"3", "4"`) or the wildcard `*`; both are
 * part of the HTTP spec and cost nothing to honour here.
 */
export function ifNoneMatchSatisfied(
  header: string | null | undefined,
  etag: string,
): boolean {
  if (!header) return false;
  const wanted = normalize(etag);
  return header
    .split(",")
    .map(normalize)
    .some((candidate) => candidate === "*" || candidate === wanted);
}
