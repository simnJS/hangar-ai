# Guardrails (Garde-fous)

A Hangar kit that lets agents run with fewer permission prompts without fearing the
worst. A hook reads each call before it runs and refuses what should never pass
unseen; you let one through once, in a click.

## What it does

**A hook** (Claude Code `PreToolUse`, plain Node, no dependency) reads each call of the tools
that run a command or touch a file:

- commands: `Bash`, `PowerShell`, `Monitor`;
- files: `Read`, `Edit`, `Write`, `NotebookEdit`, `Grep`, and `LSP`, which opens the file it is given;
- what sends a local file away: `Artifact`, `ArtifactData`, `DesignSync`.

[What stays unwatched](#what-stays-unwatched) is listed below. It refuses:

- **recursive deletes** outside the project, or of the whole project or its `.git`:
  `rm -rf`, `rimraf`, `find -delete`, `xargs rm -r`, `rd /s`, `Remove-Item -Recurse` and its
  aliases, `Get-ChildItem | Remove-Item`, `[IO.Directory]::Delete(x, $true)`.
  `rm -rf node_modules`, `Remove-Item -Recurse dist` and whatever else is inside the project
  pass, and so do the temporary folders' contents. A target the command line does not spell
  out (`rm -rf "$X"`, `$x | Remove-Item -Recurse`) is refused: nothing says where it leads;
- **force pushes on a default branch**: `--force`, `-f`, `--force-with-lease`, a
  `+refspec`, to `main`, `master`, `trunk` or what `origin/HEAD` names. A feature
  branch passes;
- `git reset --hard`, and `git clean` with `--force` unless it is a dry run;
- **dropping a database**: `DROP DATABASE` given to a database client, `dropdb`,
  `rails db:drop`, `prisma migrate reset`…;
- **a download into a shell**: `curl … | sh`, `bash <(curl …)`, `eval "$(curl …)"`,
  `irm … | iex`, `Invoke-WebRequest … | Invoke-Expression`, `iex (New-Object Net.WebClient).DownloadString(…)`,
  or saved and run in the same command line;
- **`powershell -EncodedCommand`**, and `pwsh -enc`, `-e`, `-ec`…: refused as unreadable, whatever
  the base64 holds, since nobody can read it before it runs;
- **reading or printing secrets**: `.env` (not `.env.example`), private keys, credential
  files, with the file tools or through `cat`, `grep`, `base64`, `cp`, `curl -d @…`,
  `Get-Content` (`gc`, `cat`, `type`), `Select-String`, `[IO.File]::ReadAllText`,
  a `python -c` one-liner and so on. Loading a `.env` for a program (`source .env`,
  `node --env-file=.env`) passes: nothing is shown;
- **changing the guard's own files**: `.hangar/guard.json`, its data folder, the kit's
  folder.

It reads what the shell would run: chained commands, `bash -c`, `eval`, `sudo`,
`find -exec`, `$(…)`, here-documents, `cd` and simple variables along the way, Git Bash,
`cmd /c`, `powershell -c` and `pwsh -c`. `echo "rm -rf /"` and a commit message that mentions
`git reset --hard` pass. The reason it gives the agent says what is wrong and to ask you.

PowerShell has a parser of its own, for what it does differently: the backtick that escapes,
quotes that double themselves, here-strings, `-Rec` and `-Path:x`, comma lists, aliases, pipelines
of objects, script blocks in `if`, `foreach`, `ForEach-Object`, `Invoke-Command` and `&`, `iex` and
`Start-Process` holding another command, `$env:X` and `Join-Path`. `Select-String .env notes.md`
searches notes.md and passes, `Get-Content .env` is refused, `Remove-Item -Recurse dist -WhatIf`
deletes nothing. `Monitor`'s command runs in the same shell as `Bash`, and is read as one.

**It never blocks by accident.** On any error of its own it lets the call through, logs
it, and exits 0, and the tab says so. A refusal never waits: the tab is asked for in a
detached process.

**The tab « Garde-fous »** opens by itself at the first refusal, without taking the
panel or the keyboard, its badge showing the count. It lists each refusal: agent,
command, why, when. **« Autoriser une fois »** writes a pass for that exact command, that
tool and that agent, valid for 2 minutes; the hook uses it up on the retry. Tell the agent to
retry. A row names its tool when it is not `Bash`: `PowerShell: Remove-Item …`. There is no blanket allow, and a pass only stops the guard from refusing: Claude
Code's own permission prompts apply as usual. The list covers every workspace the kit is on,
since its data folder is one; each row names its folder. English and French; drawn with
[Hangar UI](../../docs/kit-ui.md), so it follows Hangar's theme and holds at the panel's narrowest.

**A skill** tells agents, when refused, to explain to you why they needed it rather than
looking for another way around it.

## What it does not protect against

**It reads the command line, and nothing else. It is no sandbox.** It stops accidents and
an agent's bad idea, not an agent set on getting through, nor anyone hostile.

- **A script can do anything once started.** `bash deploy.sh`, `npm run clean` or
  `python tool.py` pass, whatever they hold. So does `python -c "import shutil; shutil.rmtree('/')"`,
  and a one-liner that builds a path out of pieces.
- **Only the tools above are watched.** See [what stays unwatched](#what-stays-unwatched). Commands a
  person types are not read either.
- **A search across a folder is not refused**, only one aimed at a secret: `Grep` over `src/`
  may reach a `.env` in it if ripgrep does not skip it.
- **A program its rules do not list** can read any file, `my-tool .env` for one. And printing
  the environment (`env`, `printenv`) is not refused.
- **Paths are compared as text.** A symlink is not followed: `rm -rf link/x` is judged by
  where the name is, not by where it leads.
- **A remote command** (`ssh host 'rm -rf /'`) is not read, nor what `wsl` is given.
- **In PowerShell, what only a running session knows is not followed**: a function or alias defined
  in a profile, `Set-Alias`, a string built by concatenation, a `.NET` call it does not list,
  `${C:\x\.env}` reading a file as a variable. A value it cannot read in a delete is refused; elsewhere
  the call passes. A script block is read as if it ran, even one nothing calls.
- **Past 64 levels of nesting**, it gives up, lets the call through, and logs it.
- **The project's `.hangar/guard.json` can turn rules off.** A repository you clone can carry
  one. Read it if you do not trust the repository.

## What stays unwatched

Read from the tool list of Claude Code 2.1.316 (the `init` event of `claude -p`, and a session's tool
definitions). `MultiEdit` does not exist there, so it is not in the matcher. A tool a later version adds
is not watched until it is added to `hooks/hooks.json`. These run no command of their own and open no
file for the agent:

- `Glob`: it lists names, never contents, like `find . -name .env`, which passes as well;
- `Task` and `Workflow`: they start subagents, whose own calls go through the hook, each under its own
  name. `Workflow` can load its script from a file, and that loading is not read;
- `Skill`: what a skill then calls goes through the hook, but a shell line a skill's text runs by itself
  is not known to pass through it;
- `EnterWorktree` and `ExitWorktree`: `ExitWorktree` can delete a worktree it made itself, and refuses
  to drop changes unless told to;
- `WebFetch`, `WebSearch`, `ToolSearch`, `ListAgents`, `SendMessage`, `TaskStop`, `CronCreate`, `CronDelete`,
  `CronList`, `ScheduleWakeup`, `RemoteTrigger`, `PushNotification`, `ReportFindings`, `AskUserQuestion`,
  `EnterPlanMode`, `ExitPlanMode`, `ArtifactComments`, `EndConversation`, `SendFeedback`: no local file, no command;
- **every MCP server's tools** (`mcp__…`): they can do anything, and nothing says what.

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
counting as a bug as much as a missed one, with Windows's paths and shells, and another of several
hundred PowerShell lines; the hook itself as a process (exit 0, the deny's format, its own failures,
the route to the tab); passes from the tab's click to the hook's retry; the tab's logic, controller
and view against a fake `window.hangar`.

## Proof in a real Claude Code

`node kits/guard/proof/prove.js` runs `claude -p` in a throwaway repository with this kit loaded through
`CLAUDE_CODE_PLUGIN_DIRS`, and has the agent try a refused command through `Bash`, `PowerShell` and `Monitor`,
an allowed one, and a one-time pass on `PowerShell`. It asks the model, so it costs a little and is no part of
the tests. [`proof/RESULT.md`](proof/RESULT.md) is what it recorded: exactly what ran, what came back to the
agent, and that a refused command did not run.
