/**
 * How Hangar names and places the worktrees it creates: the ones the person
 * makes in the worktree panel, and the one each worker gets
 * (lib/workerControl).
 */

/** A branch name is not a folder name: `feat/login` would nest a level. */
export const folderName = (branch: string) => branch.replace(/[\\/:*?"<>|\s]+/g, "-");

/**
 * Ported from microsoft/vscode, `sanitizeBranchName` in
 * extensions/git/src/commands.ts:650 (MIT). Everything git would refuse in a
 * ref name — and everything a shell would fight over — becomes a dash.
 *
 * The pass can itself leave a leading dash (".env" → "-env"), which git
 * refuses as an option lookalike, so dashes are stripped again after it. An
 * input with nothing else in it comes out empty, and an empty name is what
 * keeps the Create button off.
 */
export const sanitizeBranchName = (name: string) =>
  name
    .trim()
    .replace(/^-+/, "")
    .replace(
      /^\.|\/\.|\.\.|~|\^|:|\/$|\.lock$|\.lock\/|\\|\*|\s|^\s*$|\.$|\[|\]$/g,
      "-",
    )
    .replace(/^-+/, "");

/**
 * The files a checkout leaves behind because git ignores them. Defaults taken
 * from Git Worktree Manager's copy-on-create list, trimmed to what actually
 * stops a project from starting: secrets and local editor settings.
 */
export const COPY_PATTERNS = [".env", ".env.*", "*.local", ".vscode/**"];

/**
 * `<parent of the main repo>/<repo>-worktrees/<branch>`: siblings of the
 * checkout rather than children of it, so git never sees them as content.
 */
export function worktreeFolder(mainRoot: string, branch: string): string {
  const separator = mainRoot.includes("\\") ? "\\" : "/";
  const segments = mainRoot.replace(/[\\/]+$/, "").split(/[\\/]/);
  const repo = segments.pop() ?? "";
  return [...segments, `${repo}-worktrees`, folderName(branch)].join(separator);
}
