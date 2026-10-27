import { describe, expect, it } from "vitest";
import { ORCHESTRATION_DEFAULTS, orchestrationOf } from "./orchestration";
import {
  draftOf,
  draftProblems,
  normalizeDraft,
  ORCHESTRATION_LIMITS,
  sameSettings,
  storedOrchestration,
  type OrchestrationDraft,
} from "./orchestrationForm";
import type { OrchestrationSettings } from "../types";

const draft = (patch: Partial<OrchestrationDraft> = {}): OrchestrationDraft => ({
  ...draftOf({}),
  ...patch,
});

describe("draftOf", () => {
  it("opens a workspace that never had settings on the defaults, blanks for the optional fields", () => {
    expect(draftOf({})).toEqual({
      ...ORCHESTRATION_DEFAULTS,
      checkCommand: "",
      branch: "",
    });
  });

  it("opens on what the workspace runs with, a malformed field read as its default", () => {
    const saved = {
      ...ORCHESTRATION_DEFAULTS,
      maxWorkers: 5,
      checkCommand: "pnpm test",
      workerModel: "  ",
      branch: "main",
    } as OrchestrationSettings;
    expect(draftOf({ orchestration: saved })).toMatchObject({
      maxWorkers: 5,
      checkCommand: "pnpm test",
      workerModel: "sonnet",
      branch: "main",
    });
  });
});

describe("normalizeDraft", () => {
  it("holds the counts to the range the steppers offer, whole numbers only", () => {
    const { maxWorkers, maxAttempts } = ORCHESTRATION_LIMITS;
    expect(normalizeDraft(draft({ maxWorkers: 0, maxAttempts: 99 }))).toMatchObject({
      maxWorkers: maxWorkers.min,
      maxAttempts: maxAttempts.max,
    });
    expect(normalizeDraft(draft({ maxWorkers: 2.6 })).maxWorkers).toBe(3);
    expect(normalizeDraft(draft({ maxWorkers: Number.NaN })).maxWorkers).toBe(
      ORCHESTRATION_DEFAULTS.maxWorkers,
    );
  });

  it("trims names, and reads a blank one as its default", () => {
    expect(
      normalizeDraft(draft({ orchestratorModel: "  claude-opus-4-1 ", workerModel: "", reviewerModel: " " })),
    ).toMatchObject({
      orchestratorModel: "claude-opus-4-1",
      workerModel: ORCHESTRATION_DEFAULTS.workerModel,
      reviewerModel: ORCHESTRATION_DEFAULTS.reviewerModel,
    });
  });

  it("reads a blank check or branch as none, and keeps a check's inner spaces", () => {
    expect(normalizeDraft(draft({ checkCommand: "   ", branch: "" }))).toMatchObject({
      checkCommand: null,
      branch: null,
    });
    expect(normalizeDraft(draft({ checkCommand: "  cargo test --locked && pnpm  test " })).checkCommand).toBe(
      "cargo test --locked && pnpm  test",
    );
  });

  it("is what orchestrationOf reads back once stored", () => {
    const next = normalizeDraft(
      draft({ maxWorkers: 4, autoMerge: false, checkCommand: "make check", branch: "develop" }),
    );
    expect(orchestrationOf({ orchestration: next })).toEqual(next);
  });
});

describe("draftProblems", () => {
  it("accepts aliases and the model ids Claude Code takes", () => {
    for (const name of [
      "opus",
      "claude-sonnet-4-5",
      "claude-sonnet-4-5[1m]",
      "us.anthropic.claude-opus-4-1-v1:0",
      "arn:aws:bedrock:us-east-1:123:inference-profile/x",
      "",
    ]) {
      expect(draftProblems(draft({ workerModel: name })), name).toEqual({});
    }
  });

  it("refuses a model name a command line would split or quote", () => {
    expect(draftProblems(draft({ orchestratorModel: "claude opus" }))).toEqual({
      orchestratorModel: "model",
    });
    expect(draftProblems(draft({ reviewerModel: 'sonnet"; rm -rf' }))).toEqual({
      reviewerModel: "model",
    });
  });

  it("refuses a branch name git would", () => {
    for (const name of ["feat/orch ui", "-main", "a..b", "wip.", "topic/", "x.lock", "a~1", "what?", "a@{1}"]) {
      expect(draftProblems(draft({ branch: name })), name).toEqual({ branch: "branch" });
    }
    for (const name of ["main", "feat/orch-ui", "release/0.16", ""]) {
      expect(draftProblems(draft({ branch: name })), name).toEqual({});
    }
  });

  it("never lets a refused field through to what is saved", () => {
    const next = normalizeDraft(draft({ workerModel: "two words", branch: "bad name" }));
    expect(next.workerModel).toBe(ORCHESTRATION_DEFAULTS.workerModel);
    expect(next.branch).toBeNull();
  });
});

describe("storedOrchestration", () => {
  it("stores nothing for settings that are all the defaults", () => {
    expect(storedOrchestration(normalizeDraft(draftOf({})))).toBeUndefined();
    expect(storedOrchestration({ ...ORCHESTRATION_DEFAULTS })).toBeUndefined();
  });

  it("stores every field once one of them differs", () => {
    const next = { ...ORCHESTRATION_DEFAULTS, autoMerge: false };
    expect(storedOrchestration(next)).toEqual(next);
    expect(storedOrchestration(next)).not.toBe(next);
  });

  it("tells settings apart field by field", () => {
    expect(sameSettings(ORCHESTRATION_DEFAULTS, { ...ORCHESTRATION_DEFAULTS })).toBe(true);
    expect(sameSettings(ORCHESTRATION_DEFAULTS, { ...ORCHESTRATION_DEFAULTS, branch: "main" })).toBe(
      false,
    );
  });
});
