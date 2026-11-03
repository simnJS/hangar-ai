# Brief for an agent porting one package of Hangar

You are porting one package of the Hangar.AI desktop app from React to native
Rust (iced 0.14), on branch `feat/native-rust`. Many agents work in parallel,
each on one package, each in its own git worktree. Read this page whole before
you start; it is the same for every package.

## 0. Before anything else

1. Your worktree may have been made from `main`. Run
   `git merge-base --is-ancestor 44cfd25 HEAD || git reset --hard feat/native-rust`.
   Then `git log --oneline -1` should show a recent `feat/native-rust` commit.
2. Never ask the owner anything: they are away. Decide, write the decision in
   your report (and in a code comment where it matters), and go on.
3. Commit on your worktree's branch. Do not push, do not merge, do not rebase
   other people's work.

## 1. What to read

- `docs/native/PLAN.md` — the architecture, the crates, the compatibility rules.
- `docs/native/inventory-frontend.md` — the porting map. Your package's row is
  in §8.1, its brief in §8.3, the shell's contract in §8.2; the surfaces and
  logic it ports are described in §1–§5 (search for its TS files).
- `crates/hangar-app/PACKAGES.md` — the shell's rules for packages: your
  directory, the functions the shell calls, how to add an `Intent`.
- `docs/native/core-api.md` — every backend function (former Tauri command)
  and event, by the TS name the frontend used.
- The TS you port, its tests, and its CSS. Port behaviour from the code, not
  from memory or guesses. The old app's sources (`src/`, `src-tauri/`, `demo/`)
  are no longer on the branch (wave 5): read them at tag `v1.12.1`, with
  `git show v1.12.1:src/lib/x.ts`, `git ls-tree -r --name-only v1.12.1 src/`
  to find a file, or `git grep <word> v1.12.1 -- src/` to search them. Hangar
  Mobile, the phone app, is still TypeScript, under `mobile/`.

## 2. The crates you build on

| Crate | Use it for |
| --- | --- |
| `hangar-core` | Everything that is not drawing. `Host` (paths, `subscribe()` to `Event`s), `Core`, and one function per former Tauri command (core-api.md). Call blocking ones off the UI thread (`Task::perform` with `spawn_blocking`). |
| `hangar-theme` | Colours, tokens, `style::*` functions. Views never write a colour or a size literal. |
| `hangar-ui` | Buttons, inputs, menus, popovers, modals, toasts, tooltips, icons, the palette, list rows, gauges, state glyphs… Use these before writing a widget; if one is missing and generic, add it to hangar-ui in its own commit and say so in your report. |
| `hangar-i18n` | Every string the person sees: `t`, `t_with`, `tn` with a `Key`. No literal UI text. A missing key: add it to `crates/hangar-i18n/dict/en.json` and `fr.json` (same place in both) and regenerate with `node crates/hangar-i18n/tools/generate.mjs`, in its own commit. |
| `hangar-term` | The terminal widget. |
| `hangar-kit-host`, `hangar-kit-ui` | Native kit tabs (WebAssembly). |
| `hangar-webview` | The browser-engine helper process (HTML kit tabs, PDF, web pages). |
| `hangar-model3d` | The 3D viewer widget. |
| `hangar-update` | The self-updater. |

## 3. Rules

- **Own your files.** Edit only your package's directory (and the files its
  row lists). The shared exceptions: appending one `Intent` variant in a commit
  of its own (PACKAGES.md), additions to the root `Cargo.toml`'s
  `[workspace.dependencies]`, additions to `Cargo.lock`, new i18n keys, new
  generic widgets in hangar-ui. Anything else outside your files: describe it in
  your report instead of doing it.
- **Port the tests.** Every TS test of the logic you port becomes a Rust test
  with the same cases. They define the behaviour.
- **State changes go through the shell** (`Model::apply(StoreAction)`); never
  write the persisted state directly.
- **Comments explain why**, in full sentences, as the existing code does
  (read a few files of `crates/hangar-core/src` to see the tone).
- **Commits**: plain imperative messages. No `Co-Authored-By` line, no mention
  of Claude.
- **Cargo.lock**: never a bare `cargo update`. If your build moves an
  existing package's version, put it back. `windows` and `windows-core` must
  stay the same version as each other (both 0.62 today).
- **Builds**: other agents build at the same time on this machine. Use
  `cargo check -p hangar-app` / `cargo test -p hangar-app <filter>`; never
  `cargo clean`. sccache is set up.
- **Running the app**: only through `scripts/native-dev.sh` (it keeps the app
  off the installed Hangar's data). A Wayland session is available; kill what
  you start after a few seconds. If you can, take a screenshot
  (`gnome-screenshot -w -f /tmp/<name>.png` while the window has focus, or the
  package's own snapshot tooling) and look at it.

## 4. Done means

- `cargo clippy -p hangar-app --all-targets -- -D warnings` and your tests pass.
- Your surfaces behave as the TS ones do, in English and French, in a dark and
  a light theme.
- Every string is a `hangar-i18n` key.
- No TODO left without a sentence saying what is missing and why.

## 5. Your final report (under 350 words)

Worktree path, branch, commits; what works and how you verified it; what you
changed outside your files (and why); decisions taken; what is left. The
coordinator merges your branch from that report.
