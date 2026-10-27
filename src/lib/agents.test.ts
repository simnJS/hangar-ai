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

describe("launchCommand for an orchestration pane", () => {
  it("adds Hangar's server on top of Claude Code's own, and lets its tools through", () => {
    expect(
      launchCommand("claude", null, {
        model: "sonnet",
        permissionMode: "auto",
        allowedTools: ["mcp__hangar"],
        mcpConfig: "/h.json",
        mcpStrict: false,
        extraRoots: ["/x"],
      }),
    ).toBe('claude --model sonnet --permission-mode auto --allowedTools mcp__hangar --mcp-config /h.json --add-dir "/x"');
    // The last flag, nothing to close its list.
    expect(launchCommand("claude", null, { mcpConfig: "/h.json", mcpStrict: false })).toBe(
      "claude --mcp-config /h.json",
    );
  });

  it("keeps the person's mode without one, and drops what a shell would misread", () => {
    expect(launchCommand("claude", null, { permissionMode: null, allowedTools: [] })).toBe("claude");
    expect(launchCommand("claude", null, { permissionMode: "auto; rm -rf ~" })).toBe("claude");
    expect(launchCommand("claude", null, { allowedTools: ["mcp__hangar", "Bash(rm *)"] })).toBe(
      "claude --allowedTools mcp__hangar",
    );
  });
});
