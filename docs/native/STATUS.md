# Hangar, native — status

The log of the port: newest entries at the top of each section. The plan is in
[PLAN.md](PLAN.md).

## Where things stand

| Package | State | Branch / notes |
| --- | --- | --- |
| W1.core — `hangar-core` without Tauri | running | agent worktree |
| W1.term — `hangar-term` | running | agent worktree |
| W1.theme — `hangar-theme` | running | agent worktree |
| W1.kits — kit-ui, kit-sdk, kit-host, example | running | agent worktree |
| W1.webview — `hangar-webview` | running | agent worktree |
| W1.inventory — inventory-frontend.md | running | writes in place |
| W2.ui, W2.shell | waiting on W1.theme, W1.core | |
| Wave 3 surfaces | waiting on W2 and the inventory | |

## Log

### 2026-11-03

- Workspace skeleton committed (`44cfd25`): backend moved to `crates/hangar-core` by pure renames, stub crates for theme, ui, term, kit-ui, kit-sdk, kit-host, webview, app. `Cargo.lock` seeded from the old one so pinned versions (notably `windows` 0.61 for cpal) carry over.
- Branch `feat/native-rust` pushed to `hangar-ai/hangar`.
- Wave 1 started: six agents in parallel (core, term, theme, kits, webview, inventory).
- sccache set up locally (untracked `.cargo/config.toml`, `jobs = 16`).

## Decisions taken without the owner

Taken while the owner was away, each with its reason; revisit any of them freely.

- **The old app stays on the branch until wave 5.** It is the reference for behaviour; deleting it early would make porting harder and gain nothing.
- **`src-tauri/main.rs.legacy`** keeps the old entry point for reference; the native `main` lives in `crates/hangar-app/src/main.rs`.
- **HTML kit tabs and PDFs open in their own window** (the webview helper is a separate process): on Wayland a webview cannot be embedded in a winit window, and a separate process keeps the main executable free of any browser engine.
- **Native kits are WebAssembly only**, not native dynamic libraries: no stable ABI, no sandbox, one build per platform otherwise (PLAN.md 4.6).
