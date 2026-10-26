import { describe, expect, it } from "vitest";
import { launchCommand } from "./agents";
import {
  INHERIT,
  isLoaded,
  needsRestart,
  presetAll,
  presetHangarOnly,
  quoteArg,
  shellFamily,
  stdioCount,
  toggleServer,
  workspaceMcp,
  type McpServer,
} from "./mcpScope";
import type { ShellInfo } from "../types";

const server = (name: string, patch: Partial<McpServer> = {}): McpServer => ({
  name,
  scope: "user",
  transport: "stdio",
  summary: name,
  status: "active",
  hangar: false,
  source: "~/.claude.json",
  ...patch,
});

const SERVERS: McpServer[] = [
  server("hangar", { scope: "project", hangar: true }),
  server("robloxstudio"),
  server("atlassian"),
  server("docs", { transport: "http" }),
  server("repo-tool", { scope: "project", status: "unapproved" }),
  server("muted", { status: "disabled" }),
];

const shell = (id: string, program = `${id}.exe`): ShellInfo => ({
  id,
  label: id,
  program,
  args: [],
});

describe("workspaceMcp", () => {
  it("reads a missing or malformed choice as Claude Code's own", () => {
    expect(workspaceMcp(undefined)).toEqual(INHERIT);
    expect(workspaceMcp({})).toEqual(INHERIT);
    expect(workspaceMcp({ mcp: { mode: "inherit", enabled: ["x"] } })).toEqual(INHERIT);
    // A hand-edited file.
    expect(
      workspaceMcp({ mcp: { mode: "custom", enabled: "x" } as never }),
    ).toEqual(INHERIT);
    expect(
      workspaceMcp({ mcp: { mode: "custom", enabled: ["a", 3, "b"] } as never }),
    ).toEqual({ mode: "custom", enabled: ["a", "b"] });
  });
});

describe("choosing servers", () => {
  it("starts « Tous » from what Claude Code would load, Hangar aside", () => {
    expect(presetAll(SERVERS)).toEqual({
      mode: "custom",
      enabled: ["robloxstudio", "atlassian", "docs"],
    });
  });

  it("keeps Hangar loaded with nothing else chosen", () => {
    const none = presetHangarOnly();
    expect(SERVERS.filter((s) => isLoaded(s, none)).map((s) => s.name)).toEqual(["hangar"]);
  });

  it("changes only the toggled server when leaving inherit", () => {
    const next = toggleServer(INHERIT, SERVERS, "robloxstudio", false);
    expect(next).toEqual({ mode: "custom", enabled: ["atlassian", "docs"] });
    // An unapproved server can still be chosen on purpose.
    expect(toggleServer(next, SERVERS, "repo-tool", true).enabled).toContain("repo-tool");
    // Toggling on twice lists it once.
    expect(toggleServer(next, SERVERS, "docs", true).enabled).toEqual(["atlassian", "docs"]);
  });

  it("counts stdio processes, not remote connections", () => {
    // hangar, robloxstudio, atlassian; docs is http, the others do not start.
    expect(stdioCount(SERVERS, INHERIT)).toBe(3);
    expect(stdioCount(SERVERS, presetHangarOnly())).toBe(1);
    // Hangar's own server is added in custom mode even where nothing
    // registers it.
    const without = SERVERS.filter((s) => !s.hangar);
    expect(stdioCount(without, INHERIT)).toBe(2);
    expect(stdioCount(without, presetHangarOnly())).toBe(1);
  });
});

describe("needsRestart", () => {
  const custom = { mode: "custom" as const, enabled: ["robloxstudio", "gone"] };

  it("leaves a pane already running the choice alone", () => {
    expect(needsRestart({ mode: "inherit" }, INHERIT, SERVERS)).toBe(false);
    // A name nothing defines any more was never in the file to begin with.
    expect(
      needsRestart({ mode: "custom", servers: ["robloxstudio", "hangar"] }, custom, SERVERS),
    ).toBe(false);
  });

  it("reaches a pane started under another choice, or never seen", () => {
    expect(needsRestart({ mode: "inherit" }, custom, SERVERS)).toBe(true);
    expect(needsRestart({ mode: "custom", servers: ["hangar"] }, custom, SERVERS)).toBe(true);
    expect(needsRestart({ mode: "custom", servers: ["hangar"] }, INHERIT, SERVERS)).toBe(true);
    expect(needsRestart(null, INHERIT, SERVERS)).toBe(true);
  });
});

describe("shellFamily", () => {
  it("knows the shells Hangar detects", () => {
    expect(shellFamily(shell("pwsh"))).toBe("powershell");
    expect(shellFamily(shell("powershell"))).toBe("powershell");
    expect(shellFamily(shell("cmd"))).toBe("cmd");
    expect(shellFamily(shell("git-bash", "C:\\Program Files\\Git\\bin\\bash.exe"))).toBe("posix");
    expect(shellFamily(shell("zsh", "/bin/zsh"))).toBe("posix");
    expect(shellFamily(shell("fish", "/usr/bin/fish"))).toBe("fish");
    expect(shellFamily(shell("nu"))).toBe("nu");
    expect(shellFamily(shell("wsl"))).toBe("wsl");
  });

  it("falls back on the program's name, then on not knowing", () => {
    expect(shellFamily(shell("custom", "C:\\tools\\PWSH.EXE"))).toBe("powershell");
    expect(shellFamily(shell("custom", "/opt/bin/xonsh"))).toBeNull();
    expect(shellFamily(null)).toBeNull();
  });
});

describe("quoteArg", () => {
  const path = "C:\\Users\\Ann\\AppData\\Roaming\\com.simnjs.hangar\\mcp\\ws-1.json";

  it("quotes a Windows path so no shell reads its backslashes", () => {
    expect(quoteArg(path, "powershell")).toBe(`'${path}'`);
    expect(quoteArg(path, "posix")).toBe(`'${path}'`);
    expect(quoteArg(path, "cmd")).toBe(`"${path}"`);
    expect(quoteArg(path, "nu")).toBe(`'${path}'`);
    expect(quoteArg(path, null)).toBe(`'${path}'`);
    expect(quoteArg("/home/ann/.local/share/mcp/ws.json", "fish")).toBe(
      "'/home/ann/.local/share/mcp/ws.json'",
    );
    expect(quoteArg(path, "fish")).toBe(`'${path.replace(/\\/g, "\\\\")}'`);
  });

  it("keeps what a shell would expand literal", () => {
    const odd = "C:\\Users\\$env:x `whoami` $(id)\\ws.json";
    expect(quoteArg(odd, "powershell")).toBe(`'${odd}'`);
    expect(quoteArg(odd, "posix")).toBe(`'${odd}'`);
    expect(quoteArg("C:\\Users\\%USERNAME%\\ws.json", "cmd")).toBeNull();
  });

  it("escapes a quote the way each shell wants it", () => {
    const quoted = "C:\\Users\\O'Neil\\ws.json";
    expect(quoteArg(quoted, "powershell")).toBe("'C:\\Users\\O''Neil\\ws.json'");
    expect(quoteArg(quoted, "posix")).toBe("'C:\\Users\\O'\\''Neil\\ws.json'");
    expect(quoteArg("/home/o'neil/ws.json", "fish")).toBe("'/home/o\\'neil/ws.json'");
    expect(quoteArg(quoted, "cmd")).toBe(`"${quoted}"`);
    expect(quoteArg(quoted, "nu")).toBeNull();
    expect(quoteArg(quoted, null)).toBeNull();
  });

  it("refuses a line break, and every WSL pane", () => {
    expect(quoteArg("a\nb", "powershell")).toBeNull();
    expect(quoteArg("a\rb", "posix")).toBeNull();
    expect(quoteArg(path, "wsl")).toBeNull();
  });
});

describe("launchCommand with a server choice", () => {
  const file = quoteArg("C:\\data\\mcp\\ws-1.json", "powershell")!;

  it("adds nothing in inherit mode", () => {
    expect(launchCommand("claude", null)).toBe("claude");
  });

  it("puts the config between the resume and the directories", () => {
    expect(
      launchCommand("claude", "abc", {
        name: "Ava",
        mcpConfig: file,
        extraRoots: ["D:\\shared"],
      }),
    ).toBe(
      `claude --resume abc --name Ava --mcp-config ${file} --strict-mcp-config --add-dir "D:\\shared"`,
    );
  });

  it("keeps a custom launcher in front", () => {
    expect(
      launchCommand("claude", null, {
        commands: { claude: "claude --model haiku" },
        mcpConfig: file,
      }),
    ).toBe(`claude --model haiku --mcp-config ${file} --strict-mcp-config`);
  });

  it("leaves the other agents alone", () => {
    expect(launchCommand("codex", "s1", { mcpConfig: file })).toBe("codex resume s1");
    expect(launchCommand("gemini", null, { mcpConfig: file })).toBe("gemini");
  });
});
