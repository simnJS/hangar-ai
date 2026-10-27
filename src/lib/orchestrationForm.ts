import { ORCHESTRATION_DEFAULTS, ORCHESTRATION_PERMISSION_MODES, orchestrationOf } from "./orchestration";
import type { OrchestrationSettings, Workspace } from "../types";

/**
 * The « Orchestration » settings of a workspace as its panel edits them
 * (components/OrchestrationPanel): what a field may hold, how what was typed
 * becomes the settings saved, and what is worth saving at all.
 *
 * Reading is lib/orchestration's: `orchestrationOf` takes whatever is stored,
 * hand-edited or not, and falls back field by field. This is the other way
 * round — a form the person is filling in — and holds them to a little more:
 * a range for the counts, a shape for the names a command line will carry.
 */

/** What the steppers offer, both ends included. */
export const ORCHESTRATION_LIMITS = {
  maxWorkers: { min: 1, max: 8 },
  maxAttempts: { min: 1, max: 5 },
} as const;

/** Offered under the model fields; Claude Code takes a full model id as well. */
export const MODEL_SUGGESTIONS = ["opus", "sonnet", "haiku"];

/** The fields as the panel holds them: text stays text until it is saved. */
export interface OrchestrationDraft {
  maxWorkers: number;
  maxAttempts: number;
  autoMerge: boolean;
  checkCommand: string;
  orchestratorModel: string;
  workerModel: string;
  reviewerModel: string;
  branch: string;
  /** "" for the person's own mode. */
  permissionMode: string;
}

export type DraftField = keyof OrchestrationDraft;

/** Why a field cannot be saved as it is. */
export type DraftProblem = "model" | "branch";

/** The panel opens on what the workspace runs with now, defaults included. */
export function draftOf(ws: Pick<Workspace, "orchestration">): OrchestrationDraft {
  const settings = orchestrationOf(ws);
  return {
    ...settings,
    checkCommand: settings.checkCommand ?? "",
    branch: settings.branch ?? "",
    permissionMode: settings.permissionMode ?? "",
  };
}

const clamp = (value: number, { min, max }: { min: number; max: number }, fallback: number) =>
  Number.isFinite(value) ? Math.min(max, Math.max(min, Math.round(value))) : fallback;

/**
 * A model name goes on a command line (`--model <name>`): an alias, or an id
 * as Claude Code spells them — `claude-opus-4-1`, `claude-sonnet-4-5[1m]`, a
 * Bedrock `us.anthropic…` id or an ARN. Never a space or a quote.
 */
const MODEL_NAME = /^[\w.:/@[\]-]+$/;

/**
 * What git refuses in a branch name, the cheap part of `check-ref-format`:
 * whitespace, the characters it reserves, `..`, a leading dash, a trailing
 * dot or slash, `.lock`.
 */
const BAD_BRANCH = /[\s~^:?*[\\]|\.\.|^-|[./]$|\.lock$|@\{/;

/** Each field that cannot be saved as typed, and why. Blank is never a problem. */
export function draftProblems(draft: OrchestrationDraft): Partial<Record<DraftField, DraftProblem>> {
  const problems: Partial<Record<DraftField, DraftProblem>> = {};
  for (const field of ["orchestratorModel", "workerModel", "reviewerModel"] as const) {
    const name = draft[field].trim();
    if (name && !MODEL_NAME.test(name)) problems[field] = "model";
  }
  const branch = draft.branch.trim();
  if (branch && BAD_BRANCH.test(branch)) problems.branch = "branch";
  return problems;
}

/**
 * The settings a draft stands for: counts held to their range, names
 * trimmed, a blank model back to its default, a blank check or branch to
 * none. Fields with a problem (`draftProblems`) are the caller's to refuse;
 * here they fall back to the default like a blank one.
 */
export function normalizeDraft(draft: OrchestrationDraft): OrchestrationSettings {
  const problems = draftProblems(draft);
  const model = (field: "orchestratorModel" | "workerModel" | "reviewerModel") =>
    (!problems[field] && draft[field].trim()) || ORCHESTRATION_DEFAULTS[field];
  const optional = (text: string) => text.trim() || null;
  return {
    maxWorkers: clamp(
      draft.maxWorkers,
      ORCHESTRATION_LIMITS.maxWorkers,
      ORCHESTRATION_DEFAULTS.maxWorkers,
    ),
    maxAttempts: clamp(
      draft.maxAttempts,
      ORCHESTRATION_LIMITS.maxAttempts,
      ORCHESTRATION_DEFAULTS.maxAttempts,
    ),
    autoMerge: draft.autoMerge,
    // A check is run through the person's shell exactly as typed, inner
    // spaces and all: only the ends are trimmed.
    checkCommand: optional(draft.checkCommand),
    orchestratorModel: model("orchestratorModel"),
    workerModel: model("workerModel"),
    reviewerModel: model("reviewerModel"),
    branch: problems.branch ? null : optional(draft.branch),
    permissionMode: (ORCHESTRATION_PERMISSION_MODES as readonly string[]).includes(draft.permissionMode)
      ? draft.permissionMode
      : null,
  };
}

export const sameSettings = (a: OrchestrationSettings, b: OrchestrationSettings) =>
  (Object.keys(ORCHESTRATION_DEFAULTS) as (keyof OrchestrationSettings)[]).every(
    (key) => a[key] === b[key],
  );

/**
 * What to store on the workspace: nothing at all when every field is its
 * default, so a workspace whose settings were put back is saved as one that
 * never had any — and follows the defaults a later version ships.
 */
export function storedOrchestration(
  settings: OrchestrationSettings,
): OrchestrationSettings | undefined {
  return sameSettings(settings, ORCHESTRATION_DEFAULTS) ? undefined : { ...settings };
}
