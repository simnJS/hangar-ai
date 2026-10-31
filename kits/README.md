# Kits

This folder holds **example kits** for [Hangar](https://github.com/simnJS/hangar-ai):
packages that bring a domain into a workspace — a Claude Code plugin (skills,
hooks, MCP servers), tabs for the side panel, commands to run on tools already on
the machine. Hangar ships none of them; a person adds the ones they want. The
contract between a kit and Hangar is [docs/kits.md](../docs/kits.md).

| Kit                  | What it shows                                                                  |
| -------------------- | ------------------------------------------------------------------------------ |
| [ci-status](ci-status) | A tab following a GitHub Actions run, opened by a hook after `git push`.       |

## Use one

In Hangar, Settings › Kits, add the GitHub link of its folder, for instance `https://github.com/simnJS/hangar-ai/tree/main/kits/ci-status`. Or link a
local copy of the folder. Then enable it on a workspace: you are shown everything
it can run (commands, hooks, setup steps) and the permissions its tabs ask for,
before anything does.

## Make your own from one

1. **Copy a kit's folder** out of this repository (`cp -r kits/ci-status ../my-kit`)
   and link it from Settings › Kits. A linked folder is read live: edit, reload the
   tab, see.
2. **Rename it.** In `hangar-kit.json` change `id` (`[a-z0-9-]`, unique on the
   machine), `name`, `description`, `version`; in `.claude-plugin/plugin.json` give
   the plugin the same name as the id; in `hooks/lib.js` change the kit id the hook
   asks Hangar to open.
3. **Replace what the kit does.** Declare your own `commands` (an argv, with a rule
   for each `{placeholder}`), your tab's `params` and `permissions` (one
   `command:<name>` per command it runs), and rewrite the page. Keep what is
   cheap to keep: logic and behaviour in files that take `window.hangar` as an
   argument, so a test can pass a fake one.
4. **Drop what you do not need.** No hook? Delete `hooks/`. No skill? Delete
   `skills/`. No plugin at all? Delete `.claude-plugin/` and the kit is just tabs.
5. **Test it** with plain `node --test`, as `ci-status/test` does.
6. **Publish it** in a git repository and share the folder's link. Keep anything
   private (tokens, internal hosts) out of it: a kit installed from GitHub is code
   other people read and run.

Before you share a kit, read it as the person enabling it will: the activation view
shows its commands, hooks and permissions verbatim, so keep each as small as the
job allows.
