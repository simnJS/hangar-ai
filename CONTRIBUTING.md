# Contributing to Hangar.AI

Thanks for taking the time to contribute. Bug reports, fixes, translations and feature
ideas are all welcome. Please follow the [code of conduct](CODE_OF_CONDUCT.md) in every
interaction.

- **Question, or want to talk it through first?** Join the [Hangar.AI Discord](https://discord.gg/hangarai).
- **Security issue?** Do not open a public issue. See [SECURITY.md](SECURITY.md).
- **Bug?** Open an issue with the bug template: OS, Hangar version, agent and steps.
- **Idea?** Read [Proposing a feature](#proposing-a-feature) first.
- **Small fix** (typo, obvious bug, missing translation)? Open a pull request directly.

## Setup

Install the prerequisites listed in
[README → Build from source](README.md#build-from-source): Node.js 22, pnpm 10, Rust
stable, cmake and LLVM/clang, and the Tauri prerequisites for your system. Then:

```bash
pnpm install
pnpm tauri dev
```

For interface work, the browser demo is the fastest loop. It needs no Rust, no
pseudo-terminal and no agent:

```bash
pnpm demo:dev        # http://localhost:4300
```

The demo compiles `src/` unchanged against a fake backend in `demo/backend/`. If you
add or rename a Tauri command, answer it in `demo/backend/index.ts` too, or that panel
breaks in the demo. See [demo/README.md](demo/README.md).

### Running a dev build next to an installed Hangar

A plain `pnpm tauri dev` uses the same bundle identifier as the installed app. It reads
and writes the same workspaces and resumes the same agent sessions. It also takes over
`~/.hangar/endpoint.json`, so your installed agents' MCP calls reach the dev build, and it
deletes that file when it closes.

To keep the two apart, give the dev build its own identifier, its own empty state and its
own endpoint file:

```bash
# 1. An empty state for the dev identifier. Without it, the dev build copies the
#    installed app's workspaces on first launch.
#    Windows: %APPDATA%\com.simnjs.hangar-dev
#    macOS:   ~/Library/Application Support/com.simnjs.hangar-dev
#    Linux:   ~/.local/share/com.simnjs.hangar-dev
mkdir -p "<that folder>" && echo '{}' > "<that folder>/state.json"

# 2. Run under that identifier, with an endpoint file outside ~/.hangar
HANGAR_ENDPOINT_FILE="$HOME/hangar-dev-endpoint.json" \
  pnpm tauri dev --config '{"identifier":"com.simnjs.hangar-dev","productName":"Hangar.AI Dev"}'
```

To call the dev build's MCP server, run `src-tauri/target/debug/hangar-ai --mcp` with the
same `HANGAR_ENDPOINT_FILE`. When you are done, delete the `com.simnjs.hangar-dev` folders.

## Checks

CI runs these checks on every pull request and every push to `main`. Run them before you
open a PR.

Frontend, from the repository root:

```bash
pnpm exec tsc --noEmit -p .   # typecheck
pnpm exec vitest run          # unit tests
pnpm build                    # production bundle
pnpm demo:build               # the browser demo, as the website builds it (writes site/demo/, which is gitignored)
```

Rust, from `src-tauri/`:

```bash
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

The Claude Code mod, if you touched `src-tauri/claude-mod/` (this one is not in CI, so
run it yourself):

```bash
claude plugin test src-tauri/claude-mod/hangar-bridge
```

Some things to know:

- CI installs the **latest stable** Rust. Run `rustup update stable` before you trust a
  local clippy pass, because new lints land with each release.
- CI runs the Rust checks on Windows, macOS and Ubuntu 22.04. Much of the platform code
  is compiled out on the other systems, so a change that builds on yours can still fail
  on another.
- On Linux, the Rust checks need the system headers listed in the README, even for
  `cargo clippy`.

## Code conventions

- **Comments explain why.** Most modules start with a comment about the decision behind
  them. Read it before you change the module, and update it if the reason changes.
- **Tests.** Pure frontend logic lives in `src/lib/` with a `*.test.ts` beside it
  (vitest). Rust tests sit in a `tests` module at the end of each file.
- **Translations.** `src/i18n.ts` holds English, the reference, and French. The French
  table is typed against the English one, so a missing key fails the build. Add both.
  If you do not speak French, say so in the PR and leave the English text in the French
  entry for review.
- **Old names stay.** Some identifiers keep the project's former name `iabench` on
  purpose: the bundle identifier `com.simnjs.iabench` and the fallbacks that read files
  written before the rename. Do not rename them. Give anything new a `hangar` name.
- **Local only.** Hangar runs entirely on the user's machine. Changes that add a hosted
  service, accounts or telemetry will not be accepted.

## Commit messages

Write them in English, in the imperative mood, and describe the change as a user or a
reader of the code would notice it. Use no type prefix and no trailing period. For
example, from the history:

```
Give each workspace its own 3D preview
Keep a workspace's board out of git status through the repository's local excludes
Ask before a layout preset closes panes that are still busy
```

If the reason is not obvious, add a body after a blank line that explains why and how you
tested. Keep one logical change per commit.

## Pull requests

- Branch from `main` and keep the PR focused on one change.
- Fill in the template: what changed, why, how you tested it, and on which systems.
- For interface changes, add a screenshot or a short clip. A light theme and a dark theme
  are both worth checking.
- Do not bump the version. Releases are cut by the maintainer.

## Proposing a feature

Open a **feature request** issue before you write a large change. Describe the problem
you are trying to solve, not only the solution. Agreeing on the shape first saves you from
rewriting a finished PR.

Useful things to include:

- what you do today, and where it gets in the way;
- which agents it concerns (Claude Code, Codex, Gemini CLI, OpenCode);
- whether it changes the MCP tools agents see, since every agent has to understand them.

## Agent instruction files

`CLAUDE.md` and `AGENTS.md` are gitignored in this repository. Hangar's MCP panel writes a
board playbook into those files for any workspace. If you drive agents on this repository
with Hangar, generate your own; they stay on your machine.

## License

By contributing, you agree that your contributions are licensed under the
[MIT License](LICENSE).
