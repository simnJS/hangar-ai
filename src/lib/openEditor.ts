import { invoke } from "@tauri-apps/api/core";
import type { Translator } from "../i18n";
import type { Workspace } from "../types";

/** « Ouvrir avec VS Code »: the typed contract of src-tauri/src/open_editor.rs. */
export type EditorOutcome =
  | { status: "opened" }
  | { status: "missingFolder" }
  | { status: "notFound" }
  | { status: "failed"; error: string };

/**
 * Opens a workspace in VS Code: its `.code-workspace` when it came from one
 * (it then has other roots), its folder otherwise. A broken IPC comes back as
 * a `failed` outcome rather than a rejection, so the caller has one path.
 */
export async function openInVSCode(ws: Pick<Workspace, "cwd" | "extraRoots">): Promise<EditorOutcome> {
  try {
    return await invoke<EditorOutcome>("open_in_vscode", {
      cwd: ws.cwd,
      fromCodeWorkspace: (ws.extraRoots?.length ?? 0) > 0,
    });
  } catch (err) {
    return { status: "failed", error: String(err) };
  }
}

/** The one line to show for an outcome, or null when there is nothing to say. */
export function editorNotice(
  outcome: EditorOutcome,
  name: string,
  t: Translator,
): string | null {
  switch (outcome.status) {
    case "opened":
      return null;
    case "missingFolder":
      return t("editor.missingFolder", { name });
    case "notFound":
      return t("editor.notFound");
    case "failed":
      return t("editor.failed", { error: outcome.error });
  }
}
