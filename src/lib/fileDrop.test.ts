import { describe, expect, it } from "vitest";
import { dropText, pathStyleOf, quotePath, quoteStyleOf, translatePath } from "./fileDrop";
import type { ShellInfo } from "../types";

const shell = (id: string, program: string): ShellInfo => ({ id, label: id, program, args: [] });

const pwsh = shell("pwsh", "C:\\Program Files\\PowerShell\\7\\pwsh.exe");
const cmd = shell("cmd", "C:\\Windows\\System32\\cmd.exe");
const wsl = shell("wsl", "C:\\Windows\\System32\\wsl.exe");
const gitBash = shell("git-bash", "C:\\Program Files\\Git\\bin\\bash.exe");
const cygwin = shell("cygwin", "C:\\cygwin64\\bin\\bash.exe");
const zsh = shell("zsh", "/bin/zsh");

describe("translatePath", () => {
  it("leaves paths alone for a native shell", () => {
    expect(translatePath("C:\\Users\\me\\shot.png", "native")).toBe("C:\\Users\\me\\shot.png");
    expect(translatePath("/Users/me/shot.png", "native")).toBe("/Users/me/shot.png");
  });

  it("mounts drives under /mnt for WSL", () => {
    expect(translatePath("C:\\Users\\me\\My Shots\\a.png", "wsl")).toBe(
      "/mnt/c/Users/me/My Shots/a.png",
    );
    expect(translatePath("D:\\", "wsl")).toBe("/mnt/d");
    expect(translatePath("E:", "wsl")).toBe("/mnt/e");
  });

  it("reads a file from the WSL file system back as a Linux path", () => {
    expect(translatePath("\\\\wsl.localhost\\Ubuntu\\home\\me\\a.txt", "wsl")).toBe(
      "/home/me/a.txt",
    );
    expect(translatePath("\\\\wsl$\\Debian\\etc\\hosts", "wsl")).toBe("/etc/hosts");
  });

  it("uses the MSYS and Cygwin drive prefixes", () => {
    expect(translatePath("C:\\repo\\a.png", "msys")).toBe("/c/repo/a.png");
    expect(translatePath("C:\\repo\\a.png", "cygwin")).toBe("/cygdrive/c/repo/a.png");
  });

  it("does not touch what it cannot map", () => {
    expect(translatePath("\\\\server\\share\\a.png", "wsl")).toBe("\\\\server\\share\\a.png");
    expect(translatePath("/home/me/a.png", "wsl")).toBe("/home/me/a.png");
  });
});

describe("quotePath", () => {
  it("only quotes when it has to", () => {
    expect(quotePath("C:\\Users\\me\\a.png", "agent")).toBe("C:\\Users\\me\\a.png");
    expect(quotePath("C:\\Users\\me\\a.png", "single")).toBe("C:\\Users\\me\\a.png");
    expect(quotePath("C:\\Users\\me\\a.png", "cmd")).toBe("C:\\Users\\me\\a.png");
    expect(quotePath("/mnt/c/Users/me/a.png", "posix")).toBe("/mnt/c/Users/me/a.png");
  });

  it("quotes a path with spaces for each reader", () => {
    expect(quotePath("C:\\My Shots\\a.png", "agent")).toBe('"C:\\My Shots\\a.png"');
    expect(quotePath("C:\\My Shots\\a.png", "cmd")).toBe('"C:\\My Shots\\a.png"');
    expect(quotePath("C:\\My Shots\\a.png", "single")).toBe("'C:\\My Shots\\a.png'");
    expect(quotePath("/mnt/c/My Shots/a.png", "posix")).toBe("'/mnt/c/My Shots/a.png'");
  });

  it("escapes the quote it wraps with", () => {
    expect(quotePath("C:\\Bob's\\a.png", "single")).toBe("'C:\\Bob''s\\a.png'");
    expect(quotePath("/home/bob's/a.png", "posix")).toBe("'/home/bob'\\''s/a.png'");
  });

  it("quotes what a shell would expand", () => {
    expect(quotePath("C:\\$env\\a.png", "single")).toBe("'C:\\$env\\a.png'");
    expect(quotePath("/tmp/a(1).png", "posix")).toBe("'/tmp/a(1).png'");
    expect(quotePath("C:\\a&b\\x.png", "cmd")).toBe('"C:\\a&b\\x.png"');
  });
});

describe("styles", () => {
  it("picks the path style from the shell", () => {
    expect(pathStyleOf(wsl)).toBe("wsl");
    expect(pathStyleOf(gitBash)).toBe("msys");
    expect(pathStyleOf(cygwin)).toBe("cygwin");
    expect(pathStyleOf(pwsh)).toBe("native");
    expect(pathStyleOf(null)).toBe("native");
  });

  it("lets the agent read the paste, whatever shell started it", () => {
    expect(quoteStyleOf("claude", pwsh)).toBe("agent");
    expect(quoteStyleOf("shell", pwsh)).toBe("single");
    expect(quoteStyleOf("shell", cmd)).toBe("cmd");
    expect(quoteStyleOf("shell", wsl)).toBe("posix");
    expect(quoteStyleOf("shell", zsh)).toBe("posix");
  });
});

describe("dropText", () => {
  it("translates, quotes and joins, with a trailing space and no Enter", () => {
    const paths = ["C:\\Users\\me\\My Shots\\a.png", "C:\\Users\\me\\b.png"];
    expect(dropText(paths, "claude", wsl)).toBe('"/mnt/c/Users/me/My Shots/a.png" /mnt/c/Users/me/b.png ');
    expect(dropText(paths, "shell", pwsh)).toBe("'C:\\Users\\me\\My Shots\\a.png' C:\\Users\\me\\b.png ");
    expect(dropText(paths, "claude", pwsh)).toBe('"C:\\Users\\me\\My Shots\\a.png" C:\\Users\\me\\b.png ');
  });
});
