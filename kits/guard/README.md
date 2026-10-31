# Guardrails (Garde-fous)

A Hangar kit that lets agents run with fewer permission prompts without fearing the
worst. A hook reads each call before it runs and refuses what should never pass
unseen; you let one through once, in a click.

## What it does

**A hook** (Claude Code `PreToolUse` on `Bash`, `Read`, `Edit`, `Write` and `Grep`,
plain Node, no dependency) refuses:

- **recursive deletes** outside the project, or of the whole project or its `.git`:
  `rm -rf`, `rimraf`, `find -delete`, `xargs rm -r`, `rd /s`, `Remove-Item -Recurse`.
  `rm -rf node_modules`, `rm -rf dist` and whatever else is inside the project pass,
  and so do the temporary folders' contents. A target the command line does not spell
  out (`rm -rf "$X"`) is refused: nothing says where it leads;
- **force pushes on a default branch**: `--force`, `-f`, `--force-with-lease`, a
  `+refspec`, to `main`, `master`, `trunk` or what `origin/HEAD` names. A feature
  branch passes;
- `git reset --hard`, and `git clean` with `--force` unless it is a dry run;
- **dropping a database**: `DROP DATABASE` given to a database client, `dropdb`,
  `rails db:drop`, `prisma migrate reset`…;
- **a download into a shell**: `curl … | sh`, `bash <(curl …)`, `eval "$(curl …)"`,
  `irm … | iex`, or saved and run in the same command line;
- **reading or printing secrets**: `.env` (not `.env.example`), private keys, credential
  files, with the file tools or through `cat`, `grep`, `base64`, `cp`, `curl -d @…`,
  a `python -c` one-liner and so on. Loading a `.env` for a program (`source .env`,
  `node --env-file=.env`) passes: nothing is shown;
- **changing the guard's own files**: `.hangar/guard.json`, its data folder, the kit's
  folder.

It reads what the shell would run: chained commands, `bash -c`, `eval`, `sudo`,
`find -exec`, `$(…)`, here-documents, `cd` and simple variables along the way, Git Bash,
`cmd /c` and `powershell -c`. `echo "rm -rf /"` and a commit message that mentions
`git reset --hard` pass. The reason it gives the agent says what is wrong and to ask you.

**It never blocks by accident.** On any error of its own it lets the call through, logs
it, and exits 0, and the tab says so. A refusal never waits: the tab is asked for in a
detached process.

**The tab « Garde-fous »** opens by itself at the first refusal, without taking the
panel or the keyboard, its badge showing the count. It lists each refusal: agent,
command, why, when. **« Autoriser une fois »** writes a pass for that exact command and
that agent, valid for 2 minutes; the hook uses it up on the retry. Tell the agent to
retry. There is no blanket allow, and a pass only stops the guard from refusing: Claude
Code's own permission prompts apply as usual.

**A skill** tells agents, when refused, to explain to you why they needed it rather than
looking for another way around it.

## What it does not protect against

**It reads the command line, and nothing else. It is no sandbox.** It stops accidents and
an agent's bad idea, not an agent set on getting through, nor anyone hostile.

- **A script can do anything once started.** `bash deploy.sh`, `npm run clean` or
  `python tool.py` pass, whatever they hold. So does `python -c "import shutil; shutil.rmtree('/')"`,
  and a one-liner that builds a path out of pieces.
- **Only the five tools above are watched.** The `PowerShell` tool on Windows, MCP servers
  and anything else an agent can call are not. Neither are commands a person types.
- **A search across a folder is not refused**, only one aimed at a secret: `Grep` over `src/`
  may reach a `.env` in it if ripgrep does not skip it.
- **A program its rules do not list** can read any file, `my-tool .env` for one. And printing
  the environment (`env`, `printenv`) is not refused.
- **Paths are compared as text.** A symlink is not followed: `rm -rf link/x` is judged by
  where the name is, not by where it leads.
- **A remote command** (`ssh host 'rm -rf /'`) is not read.
- **Past 64 levels of nesting**, it gives up, lets the call through, and logs it.
- **The project's `.hangar/guard.json` can turn rules off.** A repository you clone can carry
  one. Read it if you do not trust the repository.

## What it can touch

- **Its data folder**, `$HANGAR_KITS_DATA/guard` (`~/.hangar/kit-data/guard`): `refusals.jsonl`
  (the refused commands, agent names and folders as written, so a secret typed into a
  command line lands there too; never what a `Write` or `Edit` held), `errors.jsonl`,
  `state.json` and `passes/`. Hook and tab read and write these, nothing else. Delete the
  folder to get rid of them.
- **Hangar**, on `127.0.0.1`, to open the tab. Nothing leaves the machine.
- **Your project**: it only reads `.hangar/guard.json` and, for `git push`, `.git/HEAD` and
  `origin/HEAD`.

The tab holds `data.read` and `data.write`, and loads no external resource.

## Rules

They live in [`rules.json`](rules.json), one block per rule, each saying what it does. A project
overrides them in `.hangar/guard.json`, in the same shape: **a list adds to the kit's**, a value
replaces it, and what the kit does not know is ignored and reported in the tab.

```json
{
  "forcePush": { "defaultBranches": ["develop"] },
  "secrets": { "except": [".env.test"], "paths": ["config/master.key"] },
  "recursiveDelete": { "allowedOutsideProject": ["../shared-build"] },
  "gitClean": { "enabled": false }
}
```

A file that is not valid JSON is reported, and leaves the kit's rules as they are.

## Install

You need Hangar 1.9.0 or later, and `node` on your PATH for the hook (without it, the hook does
nothing and every call passes).

1. In Hangar, Settings › Kits, add the GitHub link:
   `https://github.com/simnJS/hangar-ai/tree/main/kits/guard`
   (or link a local copy of this folder).
2. Enable it on a workspace, and read what you accept: the hook and the tab's two permissions.
3. Restart the workspace's Claude Code panes so they load the plugin.

## Tests

```
node --test kits/guard/
```

No install step. A table of several hundred command lines, refused and allowed, a wrong refusal
counting as a bug as much as a missed one, with Windows's paths and shells; the hook itself as a
process (exit 0, the deny's format, its own failures, the route to the tab); passes from the
tab's click to the hook's retry; the tab's logic, controller and view against a fake `window.hangar`.
