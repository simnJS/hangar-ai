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
[README → Build from source](README.md#build-from-source): Rust stable with the
`wasm32-unknown-unknown` target, cmake and LLVM/clang and your system's headers, and
Node.js 22 with pnpm 10 if you work on Hangar Mobile. Then:

```bash
scripts/native-dev.sh
```

The app is the Cargo workspace under `crates/` (map and rules in
[docs/native/PLAN.md](docs/native/PLAN.md)); `cargo run -p hangar-term --example shell`
and the other crates' examples run a part on its own. The old Tauri and React app is no
longer on this branch; read it at tag `v1.12.1` (`git show v1.12.1:src/lib/board.ts`).

For Hangar Mobile, the phone app under `mobile/`, the fastest loop needs no PC: it runs
against a fake one.

```bash
pnpm install
pnpm mobile:dev      # http://localhost:4310, `--host` to open it from a phone
```

### Running a dev build next to an installed Hangar

A plain `cargo run -p hangar-app` is the same app as the installed one. It reads and
writes the same workspaces and resumes the same agent sessions. It also takes over
`~/.hangar/endpoint.json`, so your installed agents' MCP calls reach the dev build, and it
deletes that file when it closes.

`scripts/native-dev.sh` keeps the two apart: it sets `HANGAR_DATA_ROOT` and
`HANGAR_ENDPOINT_FILE` so that the state, the app data, the endpoint file, the Claude Code
mods, kits and accounts all go under `target/dev-home/` (`HANGAR_DEV_HOME` names another
folder, for instance one holding a copy of a real `state.json`). Arguments after `--` go
to the app:

```bash
scripts/native-dev.sh --release                # an optimised build
HANGAR_DEV_HOME=/tmp/hangar-try scripts/native-dev.sh
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | scripts/native-dev.sh -- --mcp
```

The last one calls the MCP server of the dev Hangar running on the same folder.

## Checks

CI runs these checks on every pull request and every push to `main`. Run them before you
open a PR.

The workspace, from the repository root:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked    # ICED_TEST_BACKEND=tiny-skia draws the views' tests in software
```

Hangar Mobile and the generated files:

```bash
pnpm install
pnpm mobile:typecheck
pnpm test                                      # the phone app's tests
pnpm mobile:build
node crates/hangar-i18n/tools/generate.mjs --check
node scripts/gen-hangar-ui.mjs --check         # the HTML kits' stylesheet, if a theme or icon changed
```

The Claude Code mod, if you touched `crates/hangar-core/claude-mod/` (this one is not in
CI, so run it yourself):

```bash
claude plugin test crates/hangar-core/claude-mod/hangar-bridge
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
- **Tests.** Rust tests sit in a `tests` module at the end of each file, or in a
  `tests.rs` beside it. Hangar Mobile's logic has a `*.test.ts` beside it (vitest).
- **Translations.** `crates/hangar-i18n/dict/en.json` holds English, the reference, and
  `fr.json` French, key for key in the same order; run
  `node crates/hangar-i18n/tools/generate.mjs` and commit what it writes. A key missing
  from one of them fails the generator. Add both. If you do not speak French, say so in
  the PR and leave the English text in the French entry for review. Hangar Mobile has its
  own dictionary, `mobile/src/i18n.ts`.
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
- whether it changes the MCP tools agents see, since Claude Code has to understand them.

## Agent instruction files

`CLAUDE.md` and `AGENTS.md` are gitignored in this repository. Hangar's MCP panel writes a
board playbook into those files for any workspace. If you drive agents on this repository
with Hangar, generate your own; they stay on your machine.

## License

By contributing, you agree that your contributions are licensed under the
[MIT License](LICENSE).
