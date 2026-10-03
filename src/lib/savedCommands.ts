import { bridgeEnqueue, ptyWrite } from "./ipc";
import { getPaneActivity } from "./agentState";
import type { SavedCommand, Settings, Workspace } from "../types";

/**
 * Running a saved command, from the pane menu and from the keyboard alike.
 *
 * Both entry points need the same two answers — in what order the commands are
 * listed, and which panes a click writes to — and they sit in different trees,
 * so the answers live here rather than in either of them.
 */

/**
 * What a pane offers: the workspace's own commands, then the global ones. The
 * order is also what `command.runN` counts through, so the menu and the
 * shortcuts can never disagree on which one is the third.
 */
export const mergeCommands = (
  workspace: Pick<Workspace, "savedCommands">,
  settings: Pick<Settings, "savedCommands">,
): SavedCommand[] => [...workspace.savedCommands, ...settings.savedCommands];

/**
 * Types a command into every pane of `paneIds`.
 *
 * The trailing CR is what runs it: without `autoRun` the command lands on the
 * prompt and waits, which is the point of saving a line you finish by hand.
 *
 * A command that runs itself does not barge into a Claude Code that is busy:
 * typed into a permission dialog, it would answer the dialog. When the pane's
 * hangar-bridge mod is there to hold it, it waits for the turn to end.
 */
export function runSavedCommand(command: SavedCommand, paneIds: string[]) {
  const payload = command.autoRun ? `${command.command}\r` : command.command;
  for (const id of paneIds) {
    const pane = getPaneActivity(id);
    const busy = pane?.activity === "working" || pane?.activity === "waiting";
    if (command.autoRun && pane?.bridged && busy) {
      bridgeEnqueue(id, command.command).catch(() => undefined);
    } else {
      ptyWrite(id, payload).catch(() => undefined);
    }
  }
}
