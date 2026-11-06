# Install and getting started

[← Hangar](../../README.md) · [Guide](README.md)

Download the file for your system from the
**[latest release](https://github.com/hangar-ai/hangar/releases/latest)**, under
**Assets**. You only download once: after that, the app offers each new version and
installs it in one click. A Hangar 1.x install offers 2.0 the same way.

| System | File | Notes |
|---|---|---|
| Windows (x64) | `Hangar.AI_<version>_x64-setup.exe` | Installs for your user only, without administrator rights. HTML kit tabs and PDFs use WebView2, which ships with Windows 11; on Windows 10 the installer adds it if it is missing. |
| macOS (Apple Silicon and Intel) | `Hangar.AI_<version>_universal.dmg` | One universal build for both, macOS 10.15 or later. |
| Linux (x86_64) | `.AppImage`, `.deb` or `.rpm` | The AppImage runs without installing (`chmod +x` it first) on distributions with glibc 2.35 or later (2022 on). HTML kit tabs and PDFs need WebKitGTK 4.1: apt and dnf install it with the `.deb` and `.rpm`; with the AppImage, install `libwebkit2gtk-4.1-0` (Debian, Ubuntu) or `webkit2gtk4.1` (Fedora) if your system lacks it. |

> [!NOTE]
> **Windows: SmartScreen.** The installer is not code-signed, so on first run SmartScreen
> shows "Windows protected your PC". Click **More info**, then **Run anyway**.
>
> **macOS: Gatekeeper.** The macOS build is signed and notarised, so it opens after
> macOS's usual first-launch confirmation. If a release's notes ever say the macOS build is
> unsigned (or you built it yourself), macOS blocks the first launch: right-click the app
> and choose **Open**. On macOS 15 and later, go to **System Settings → Privacy &
> Security** and click **Open Anyway**.

You also need [Claude Code](https://code.claude.com/docs/en/overview) installed and
signed in. Hangar launches it; it does not install it.

**Coming from 1.x.** Accept the update, or install 2.0 over 1.x: it uses the same folders
and the same `state.json`, so your workspaces, boards, memory, settings, shortcuts, kits and
Claude accounts are there. The kit tabs you had left open and each account's cached usage
lived in the old web view, and are not carried over. The [changelog](../../CHANGELOG.md) lists
what 2.0 does not do yet: mainly, there is no screen reader support, and Windows and macOS
have been tested by CI but not yet used by a person.

## Getting started

1. **Create a workspace** with `Ctrl`/`Cmd`+`Shift`+`N`. Pick a project folder and a
   layout, then click a cell to choose Claude Code or a shell. If Claude Code is not
   installed, the dialog says so.
2. **Connect Claude Code to Hangar.** Open the MCP panel (`Ctrl`/`Cmd`+`Alt`+`M`; `Alt` is
   `Option` on a Mac) and click
   **Configure**. The panel can also write a short playbook into the project's `CLAUDE.md`,
   telling agents to claim tasks before they start. Restart the agents so they load the
   server.
3. **Give them work.** Add tasks on the board (`Ctrl`/`Cmd`+`Alt`+`B`), or ask an agent to
   split a goal into tasks. Other agents pick tasks up with `board_next_task`.
4. **Try orchestration.** In a git repository, run **New orchestrator** from the command
   palette (`Ctrl`/`Cmd`+`Shift`+`P`), or pin an existing Claude Code pane from its **…**
   menu. Then describe the goal to it. **Orchestration…** in the same menu opens the
   workspace's settings: models, permissions, check command and automatic merging.

The command palette lists every command. Shortcuts can be changed in
**Settings → Shortcuts**, and the defaults use `Ctrl` on Windows and Linux and `Cmd` on
macOS.
