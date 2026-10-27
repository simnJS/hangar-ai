import { describe, expect, it } from "vitest";
import { launchCommand } from "./agents";

describe("launchCommand with a model", () => {
  it("starts Claude Code on the pane's model", () => {
    expect(launchCommand("claude", null, { model: "sonnet" })).toBe("claude --model sonnet");
    expect(launchCommand("claude", "abc", { model: "claude-sonnet-4-5", name: "Ava" })).toBe(
      "claude --resume abc --name Ava --model claude-sonnet-4-5",
    );
  });

  it("quotes a model a shell would read as a glob, and drops one it could not quote", () => {
    expect(launchCommand("claude", null, { model: "opus[1m]" })).toBe('claude --model "opus[1m]"');
    expect(launchCommand("claude", null, { model: 'opus"; rm -rf ~' })).toBe("claude");
  });

  it("keeps --model ahead of the variadic flags", () => {
    expect(
      launchCommand("claude", null, { model: "haiku", mcpConfig: "/m.json", extraRoots: ["/x"] }),
    ).toBe('claude --model haiku --mcp-config /m.json --strict-mcp-config --add-dir "/x"');
  });

  it("changes nothing without one, nor for the other agents", () => {
    expect(launchCommand("claude", null, { model: null })).toBe("claude");
    expect(launchCommand("claude", null, { model: "  " })).toBe("claude");
    expect(launchCommand("codex", null, { model: "sonnet" })).toBe("codex");
    expect(launchCommand("shell", null, { model: "sonnet" })).toBeNull();
  });
});
