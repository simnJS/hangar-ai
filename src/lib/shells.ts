import type { ShellInfo } from "../types";

/**
 * Resolves the shell for a pane by walking the override chain
 * (pane → workspace → global), falling back to the first detected shell.
 * Ids that no longer exist — an uninstalled shell — are skipped.
 */
export function resolveShell(
  shells: ShellInfo[],
  ...preferred: (string | null | undefined)[]
): ShellInfo | null {
  for (const id of preferred) {
    if (!id) continue;
    const match = shells.find((shell) => shell.id === id);
    if (match) return match;
  }
  return shells[0] ?? null;
}

export const shellLabel = (shells: ShellInfo[], id: string | null): string =>
  shells.find((shell) => shell.id === id)?.label ?? "Auto";
