import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke as tauriInvoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import type { Translator } from "../i18n";
import { editorNotice, openInVSCode } from "./openEditor";

const invoke = vi.mocked(tauriInvoke);

const t: Translator = (key, vars) => `${key}${vars ? JSON.stringify(vars) : ""}`;

beforeEach(() => {
  invoke.mockReset();
});

describe("openInVSCode", () => {
  it("opens the folder of a plain workspace", async () => {
    invoke.mockResolvedValue({ status: "opened" });
    await openInVSCode({ cwd: "C:/a b", extraRoots: [] });
    expect(invoke).toHaveBeenCalledWith("open_in_vscode", {
      cwd: "C:/a b",
      fromCodeWorkspace: false,
    });
  });

  it("asks for the .code-workspace when the workspace has other roots", async () => {
    invoke.mockResolvedValue({ status: "opened" });
    await openInVSCode({ cwd: "/p", extraRoots: ["/q"] });
    expect(invoke).toHaveBeenCalledWith("open_in_vscode", {
      cwd: "/p",
      fromCodeWorkspace: true,
    });
  });

  it("turns a rejected IPC into a failed outcome", async () => {
    invoke.mockRejectedValue(new Error("boom"));
    expect(await openInVSCode({ cwd: "/p", extraRoots: [] })).toEqual({
      status: "failed",
      error: "Error: boom",
    });
  });
});

describe("editorNotice", () => {
  it("says nothing when it opened", () => {
    expect(editorNotice({ status: "opened" }, "W", t)).toBeNull();
  });
  it("names the workspace whose folder is gone", () => {
    expect(editorNotice({ status: "missingFolder" }, "W", t)).toBe(
      'editor.missingFolder{"name":"W"}',
    );
  });
  it("says VS Code is missing, and what failed otherwise", () => {
    expect(editorNotice({ status: "notFound" }, "W", t)).toBe("editor.notFound");
    expect(editorNotice({ status: "failed", error: "x" }, "W", t)).toBe(
      'editor.failed{"error":"x"}',
    );
  });
});
