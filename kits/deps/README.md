# Dependencies

A Hangar kit that tells you what is vulnerable or out of date in the project, when you
ask, and hands any of it to an agent in one click.

## What it does

**A tab**, « Dépendances »: press **Lancer l’audit** and it lists, from the project's
lockfile:

- the **vulnerabilities**, most severe first, each with the installed version, the advisory
  (id and title) and the version that fixes it; the tab's badge counts the high and critical
  ones;
- the **out-of-date packages**, major gaps first, then minor, then patch.

It runs **on demand only**: opening the tab reads the last result from the kit's data folder
and calls nothing, so there is no network call until you click. It shows when the audit last
ran.

**Confier à un agent** sends one package to a Claude Code agent of the workspace (the
orchestrator by default, or the one you pick). The brief names the package, the versions and
the advisory, says it comes from the kit's audit and not from the person, and puts the
advisory's text, which comes from a public database, between markers as data to read, never
instructions to follow.

Designed states: not run yet, running, clean, no lockfile, tool missing (with the command that
installs it), one half of the audit failed (offline, for instance), command not allowed, and a
send refused.

**A skill** that tells agents to read the last recorded audit rather than run audit commands
again, when they are only asked what is vulnerable or out of date.

## What it runs

One declared command, `audit`: `node {kitDir}/tools/audit.js`, started in the workspace
folder. Hangar shows it to you verbatim when you enable the kit. The script looks for a
lockfile and runs the matching package manager's **read-only** commands, found on the PATH
(never in the project's folder), then prints one JSON document:

| Lockfile                                  | Vulnerabilities                                                        | Out of date                                  |
| ----------------------------------------- | ---------------------------------------------------------------------- | -------------------------------------------- |
| `pnpm-lock.yaml`                          | `pnpm audit --json`                                                    | `pnpm outdated --format json`                |
| `package-lock.json`, `npm-shrinkwrap.json` | `npm audit --json`                                                    | `npm outdated --json`                        |
| `yarn.lock`                               | `yarn audit --json` (`yarn npm audit --json` from Yarn 2)              | `yarn outdated --json` (none from Yarn 2)    |
| `Cargo.lock`                              | `cargo audit --json` (needs cargo-audit)                               | `cargo outdated --format json --workspace --exit-code 0` (needs cargo-outdated) |
| `requirements.txt`                        | `pip-audit -r requirements.txt --no-deps --disable-pip -f json` (needs pip-audit) | `pip list --outdated --format json`, cut to the packages the file names |

A JavaScript project with several lockfiles is audited with the manager its `package.json`
declares, else pnpm, yarn, npm in that order. A project with a lockfile of several ecosystems
(a `pnpm-lock.yaml` and a `Cargo.lock`) gets each.

- **Read-only.** It never installs, fixes, updates or writes a file, a lockfile included. The
  arguments it may pass are one fixed list in the script, and a test fails if one of them is an
  installing or fixing command or if the script could write to disk.
- **The tools talk to their registries**: `npm audit` and friends send your dependency list to
  the registry, and `cargo audit` fetches RustSec's advisory database into `~/.cargo`. That is
  why nothing runs until you click.
- A tool that is not installed is reported as such, never as a clean project; so is an audit
  that failed.
- **The tab** holds `command:audit` (to run that command), `data.read` and `data.write` (to
  keep the last result in the kit's data folder, `last.json`), `panes.read` (to list the
  agents) and `panes.send` (to send a package, when you click). It touches no file of the
  project.

## What it does not do

- It does not fix anything: updating a package is the agent's job, with your approval of its
  work like any other change.
- RustSec and pip-audit do not grade their findings: they show as « Not graded » and are not
  counted among the high and critical ones.
- `pip list --outdated` reads the Python environment on the PATH, so a requirements file whose
  packages are not installed there shows no outdated ones. `pip-audit` needs every requirement
  pinned (`name==version`).
- Yarn 2 and later have no outdated command; the tab says so.
- Poetry, uv, Pipfile and Bun lockfiles are not read.

## Tests and fixtures

`node --test kits/deps/`. The npm and pnpm outputs in `test/fixtures/` are recordings
(npm 11.6, pnpm 10.28); the yarn, cargo and pip ones are written from each tool's documented
output, because those tools were not installed where the kit was made. Run them against your
own tool and tell us if a field moved.

## Get it

Link `kits/deps` from Hangar, or clone
<https://github.com/simnJS/hangar-ai/tree/main/kits/deps>. Needs Hangar 1.9.0 or later and
Node.js on the PATH.
