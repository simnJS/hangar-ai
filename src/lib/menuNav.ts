/**
 * Where an arrow key lands in a list of entries — a menu, the session picker,
 * the settings sidebar.
 *
 * `current` is the focused entry, or -1 when focus is outside the list (on the
 * button that opened it, say): Down then enters at the top and Up at the
 * bottom. A menu wraps around its ends, the way the system's do; a list
 * stops at them. Null means there is nowhere to go.
 */
export function stepIndex(
  current: number,
  count: number,
  step: 1 | -1,
  wrap: boolean,
): number | null {
  if (count <= 0) return null;
  if (current < 0 || current >= count) return step === 1 ? 0 : count - 1;
  const next = current + step;
  if (next >= 0 && next < count) return next;
  return wrap ? (next + count) % count : null;
}

/** The step an arrow key asks for, or null for any other key. */
export function arrowStep(key: string): 1 | -1 | null {
  if (key === "ArrowDown") return 1;
  if (key === "ArrowUp") return -1;
  return null;
}
