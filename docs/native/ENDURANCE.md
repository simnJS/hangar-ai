# Endurance: 16 and 32 busy panes for an hour

Hangar had never been run for long under load. This file reports two
one-hour runs, with 16 and then 32 agent panes printing all the time, and a
steady mix of other activity. It says whether memory, threads, open files or
CPU grow over time, and how to run the test again.

Measured on 6 November 2026, on the machine described in
[BENCHMARK.md](BENCHMARK.md) (Ryzen 9 7950X3D, 32 threads, 60 GiB). The
person was using the machine during the runs, and other agents were building
beside them.

## The answer

- **No leak, no drift.** After the warm-up, every measure is flat for the
  rest of the hour. Anonymous memory grows by at most 5 to 21 MB an hour, and
  the counts in the app explain that growth: terminal grids that are still
  filling, and board tasks, which are kept by design. Threads, open files,
  tokio tasks, terminals and pty reader threads do not move at all.
- **The terminal history stays capped.** No terminal ever holds more than its
  10,000 lines of scrollback (`term.max_lines` stays at 10,000 all hour). The
  pty replay buffers stay under 512 KiB per pane.
- **Closing panes gives memory back.** Closing half the agent panes after the
  hour cut anonymous memory by 87 MB (16 panes) and 88 MB (32 panes). It also
  cut threads by 16 and 32, and open files by 24 and 48: two threads and
  three file descriptors per closed pane. Closing and reopening a pane every two
  minutes (30 times an hour) left no terminal, thread or file behind.
- **The CPU stays flat** at 1.5 to 4 % of one core once every pane's
  scrollback is full. With 32 panes, the first 15 minutes cost more (10 to 16
  %, with peaks near 30 %); see [Open questions](#open-questions).
- **One open question, not a leak:** with 32 narrow panes, the scrollback
  costs about three times more memory per line than the visible cells
  explain (697 bytes against about 180 to 250 in the other runs). It is
  stable over the hour and bounded by the 10,000-line cap. The likely cause is
  that alacritty's rows keep their capacity after a resize. See
  [Open questions](#open-questions).

## Method

### What ran

An isolated Hangar: the release build of `main` at `cc1990a`, with a
throwaway probe added (see [Probes](#probes)). It ran with its own data root,
its own XDG folders, endpoint file and HOME. Every inherited `HANGAR_*`
variable was dropped. The person's own Hangar files were fingerprinted
before and after each run. The only files that changed were the ones the
person's running Hangar writes itself (`account-usage.json`, `state.json`,
its MCP files), never the bench instance's.

One window, in which no workspace was ever switched through the GUI. The
content of the window:

| Workspace | Panes |
|---|---|
| `bench1`, `bench2` (16-pane run) or `bench1` to `bench3` (32-pane run) | the N fake Claude panes, spread evenly (8 + 8, or 11 + 11 + 10). Hangar holds at most 16 panes in a workspace. |
| `mix` | `B1`, `B2`: shells printing bursts of 3,000 UTF-8 lines, then resting 25 s. `C1`: a fake Claude pane that is closed and created again every 2 minutes. |
| `chat` | in the 2.2 chat view (`chat-view` experiment): a fake Claude orchestrator resuming a real transcript, and a shell. |

The fake Claude is `~/dev/bench/runs/resume-data/fake-claude.sh` with
`BENCH_BUSY=1`. Hangar types its resume command into the pane, then the script
prints `● working on step N — reading src/module_K.rs` every 50 ms, without
end.

The activity, all through the control API on its own clock:

- every 30 s, `pane_focus` on the next workspace in turn (all five or six of
  them). This switches the active workspace without raising the window;
- every 60 s, a board task is created on the workspace folder (`POST
  /api/tasks`, a 400-character description), and the previous task is moved
  a column on (`PATCH /api/tasks/{id}`);
- every 120 s, `C1` is closed (`pane_close`) and created again (`pane_create`,
  agent `claude`). It worked 30 times out of 30 in each run;
- after 60 minutes, every other fake pane of each `bench` workspace is
  closed, and sampling goes on for 4 more minutes.

### What was sampled, every 10 s

- from `/proc/<app>/smaps_rollup`: Rss, Pss and Anonymous;
- the threads (`/proc/<app>/task`), with the CPU of each one, added up by
  thread name; open file descriptors (`/proc/<app>/fd`); the app's CPU;
- the panes' processes, and how many fake Claudes are alive;
- the probe's counters (next section).

The warm-up is the first 5 minutes. The slopes are least-squares fits over
5 to 60 minutes, in units per hour.

### Probes

The control API cannot show the size of a terminal's history, or the replay
buffers. A throwaway commit (local branch `probe/endurance-throwaway`, never
delivered) added atomic counters, written to the app's log every 10 s:

- `hangar-term` (`Terminal`): the live terminals, their total history lines,
  their grid cells (`(history + rows) × columns`) and the largest history any
  of them reached;
- `hangar-core` (`pty.rs`): the live replay buffers (`History`) and their
  bytes (length and capacity), the live pty reader threads, and tokio's alive
  tasks and global queue (`Handle::metrics`).

The counters cost a relaxed atomic add per chunk of output. The app's
behaviour is the same.

## Results

### 16 panes (`native-endurance16-74966`)

| Metric | at 5 min | at 60 min | slope over 5 to 60 min | before closing | 2 to 4 min after closing 8 |
|---|---|---|---|---|---|
| RSS | 405 MB | 367 MB | −70 MB/h (r² 0.37) | 366 MB | 280 MB |
| PSS | 296 MB | 348 MB | +10 MB/h (r² 0.08) | 347 MB | 261 MB |
| Anonymous | 230 MB | 309 MB | +21 MB/h (r² 0.19) | 308 MB | 222 MB |
| Threads | 108 | 109 | 0 | 109 | 93 |
| Open fds | 136 | 139 | +0.2/h | 139 | 115 |
| App CPU | 1.7 % | 1.2 % | 0 | 1.6 % | 1.4 % |
| Terminals / pty readers / replay buffers | 21 | 21 | 0 | 21 | 13 |
| History lines, all terminals | 146,967 | 190,716 | (filling until about 10 min) | 190,285 | 113,682 |
| Largest history of one terminal | 10,000 | 10,000 | 0 | 10,000 | 10,000 |
| Replay buffers (capacity) | 7.5 MiB | 13.6 MiB | +1.1 MiB/h | 13.6 MiB | 7.9 MiB |
| Tokio alive tasks | 33 | 31 | 0 | 33 | 33 |

From 5–20 min to the last 15 min, anonymous memory grew by 12.5 MB. The grid
cells grew by 525,000 in the same time (2 terminals still filling), which is
12.6 MB at 24 bytes a cell. With the grids taken out, what is left grows by
3.4 MB an hour (92.6 to 94.6 MB).

```text
RSS MB  (260.2 .. 459.5; x: 0 .. 64 min; | = half closed)
   459.5 ┤         ******                                                   |
   441.3 ┤        *      *                                                  |
   423.2 ┤       *                                                          |
   405.1 ┤     **                                                           |
   387.0 ┤    *                                                             |
   368.9 ┤   *            **************************************************|
   350.8 ┤                                                                  *
   332.7 ┤  *                                                               |
   314.6 ┤                                                                  |
   296.5 ┤ *                                                                |
   278.4 ┤                                                                  |*****
   260.2 ┤*                                                                 |
         └────────────────────────────────────────────────────────────────────────
```

```text
Anonymous MB  (83.5 .. 310.6; x: 0 .. 64 min; | = half closed)
   310.6 ┤          ********************************************************|
   290.0 ┤        **                                                        *
   269.3 ┤       *                                                          |
   248.7 ┤      *                                                           |
   228.0 ┤     *                                                            |*****
   207.4 ┤    *                                                             |
   186.7 ┤   *                                                              |
   166.1 ┤  *                                                               |
   145.4 ┤                                                                  |
   124.8 ┤ *                                                                |
   104.1 ┤                                                                  |
    83.5 ┤*                                                                 |
         └────────────────────────────────────────────────────────────────────────
```

```text
app CPU %  (1.2 .. 2.2; x: 0 .. 64 min; | = half closed)
     2.2 ┤                     *                                            |
     2.1 ┤              *                                                   |
     2.0 ┤                           *    *                                 |
     1.9 ┤                                                 *                |
     1.8 ┤               ** ***                *               * *       *  |
     1.7 ┤  *    *     *         *      **                        * * *    *|
     1.7 ┤           *            *                   **      *        *    |*
     1.6 ┤          *      *    *  ** **    * * **** *   *      *  * *  * * *
     1.5 ┤     *   *  *                    * *      *   * * *               |   *
     1.4 ┤ * ** * *                                          *              |  * *
     1.3 ┤*                                                                 |
     1.2 ┤                                                                  | *
         └────────────────────────────────────────────────────────────────────────
```

### 32 panes (`native-endurance32-78810`)

| Metric | at 5 min | at 60 min | slope over 5 to 60 min | before closing | 2 to 4 min after closing 16 |
|---|---|---|---|---|---|
| RSS | 604 MB | 545 MB | −101 MB/h (r² 0.81) | 545 MB | 458 MB |
| PSS | 519 MB | 511 MB | −37 MB/h (r² 0.50) | 510 MB | 424 MB |
| Anonymous | 436 MB | 456 MB | +5 MB/h (r² 0.19) | 456 MB | 368 MB |
| Threads | 145 | 146 | 0 | 146 | 114 |
| Open fds | 184 | 187 | 0 | 187 | 139 |
| App CPU | 9.5 % | 2.7 % | −10 %/h (high until about 15 min) | 4.1 % | 8.4 % |
| Terminals / pty readers / replay buffers | 37 | 37 | 0 | 37 | 21 |
| History lines, all terminals | 345,491 | 350,820 | +2,217/h | 350,716 | 193,091 |
| Largest history of one terminal | 10,000 | 10,000 | 0 | 10,000 | 10,000 |
| Replay buffers (capacity) | 13.5 MiB | 25.5 MiB | +2.5 MiB/h | 25.5 MiB | 13.8 MiB |
| Tokio alive tasks | 30 | 30 | 0 | 31 | 31 |

With the grids taken out, what is left grows by 1.9 MB an hour (292 to 294
MB).

```text
RSS MB  (276.0 .. 628.3; x: 0 .. 64 min; | = half closed)
   628.3 ┤       ***************************    *                           |
   596.3 ┤    ***                           **** *****                      |
   564.3 ┤   *                                        **********            |
   532.2 ┤                                                      *************
   500.2 ┤                                                                  |
   468.2 ┤                                                                  | ****
   436.1 ┤  *                                                               |*
   404.1 ┤                                                                  |
   372.1 ┤                                                                  |
   340.0 ┤ *                                                                |
   308.0 ┤                                                                  |
   276.0 ┤*                                                                 |
         └────────────────────────────────────────────────────────────────────────
```

```text
Anonymous MB  (96.6 .. 460.3; x: 0 .. 64 min; | = half closed)
   460.3 ┤       ***********************************************************|
   427.2 ┤    ***                                                           *
   394.2 ┤   *                                                              |    *
   361.1 ┤                                                                  |****
   328.1 ┤                                                                  |
   295.0 ┤                                                                  |
   261.9 ┤                                                                  |
   228.9 ┤  *                                                               |
   195.8 ┤                                                                  |
   162.8 ┤ *                                                                |
   129.7 ┤                                                                  |
    96.6 ┤*                                                                 |
         └────────────────────────────────────────────────────────────────────────
```

```text
app CPU %  (1.6 .. 30.0; x: 0 .. 64 min; | = half closed)
    30.0 ┤   *                                                              |
    27.4 ┤               *                                                  |
    24.8 ┤                                                                  |
    22.2 ┤      *       *                                                   |   **
    19.7 ┤        *                                                         |
    17.1 ┤    ** * *                                                        |
    14.5 ┤                                                                  |
    11.9 ┤                *                                                 |
     9.3 ┤                                                                  |
     6.7 ┤ **                                                               |
     4.1 ┤          *      ****       *** *** ************* ** ** ** * ** ****
     1.6 ┤*          ***       *******   *   *             *  *  *  * *  *  | **
         └────────────────────────────────────────────────────────────────────────
```

```text
open fds  (129.0 .. 187.4; x: 0 .. 64 min; | = half closed)
   187.4 ┤  ****************************************************************|
   182.1 ┤ *                                                                |
   176.8 ┤                                                                  *
   171.5 ┤                                                                  |
   166.2 ┤                                                                  |
   160.9 ┤                                                                  |
   155.5 ┤                                                                  |
   150.2 ┤                                                                  |
   144.9 ┤                                                                  |
   139.6 ┤                                                                  |*****
   134.3 ┤                                                                  |
   129.0 ┤*                                                                 |
         └────────────────────────────────────────────────────────────────────────
```

### What the small growth is

Three short runs after the hour repeated one action quickly, with 4 busy
panes, for 12 minutes. Each figure is the anonymous memory less the grids,
from minutes 3–5 to the end:

| Action | Repeats | Left behind |
|---|---|---|
| Close and reopen a pane (every 5 s) | 119 | +1.4 MB, about 12 KB each |
| Switch workspace (every 2 s) | 296 | +1.0 MB, about 3 KB each |
| Create a board task and move the previous one (every 2 s) | 296 | +5.5 MB, about 19 KB each |

The fourth run, with no action, was stopped after 5 minutes (−1.4 MB). These
differences are close to the noise of the allocator. The board's share is
what the board holds: every task stays on it, with its description, so it
grows with the tasks by design. At the endurance rates (60 tasks, 30
reopened panes and 120 switches an hour), this adds up to the 2 to 3 MB an
hour seen above.

Threads and open files came back to their starting count after every one of
the 119 reopened panes.

### RSS and PSS fall while anonymous memory holds

In both runs, RSS climbs during the first 10 minutes and then falls by 50
to 100 MB, while anonymous memory holds. What falls is shared and file
pages (fonts, GPU driver mappings), most likely reclaimed by the kernel
under memory pressure: the machine was in use, and its swap was full. Since
the app's own memory holds, the fall does not come from Hangar.

## Open questions

1. **More memory per scrollback line with 32 narrow panes.** Anonymous
   memory less the visible cells (24 bytes each) is 93 to 103 MB in the
   16-pane runs, but 294 MB in the 32-pane run. That is about 700 bytes per
   history line, against 180 to 250. It stays the same for the whole hour, so
   it is bounded by the 10,000-line cap (worst case: 10,000 × the widest
   width the pane ever had × 24 bytes per terminal). The likely cause is in
   alacritty_terminal 0.26:
   - `Row::shrink` keeps the row's allocation (`Vec::split_off` leaves the
     capacity in place);
   - `Row::grow` (`resize_with`) can double it;
   - rows that scroll out of the history are recycled, never reallocated.

   So a row keeps, in memory, the widest width its pane ever had. This is
   not confirmed: confirming it needs a probe of each row's capacity, which
   alacritty does not expose, or a heap profile (dhat on a throwaway branch:
   there is no heaptrack or valgrind on this machine). If it is confirmed,
   the fix belongs in `hangar-term`: on a resize to fewer columns, rebuild the
   grid's rows at the new width (or ask alacritty upstream for a
   `shrink_to_fit` on shrink).
2. **CPU during the first 15 minutes with 32 panes:** 10 to 16 % of one
   core, with peaks near 30 %, then 2 to 4 % for the rest of the hour. The
   16-pane run shows no such phase (1.5 % throughout). The scrollbacks were
   full by 5 minutes, so filling them is not the whole cause. Load from the
   rest of the machine cannot be ruled out. The per-thread CPU in the samples
   (`by_name`) puts it on the threads named after the binary, the main
   thread among them.
3. **Memory the allocator keeps.** It was not measured whether
   `malloc_trim(0)` would give more back after the panes are closed. The
   experiment (`TRIM_LIB` below) was prepared but stopped before it ran.

## The first attempt, and what it taught

A first 16-pane run put all 18 panes in one workspace. Hangar holds at most
16 panes in a workspace, so every `pane_create` was refused after the first
close ("is full: a workspace holds at most 16 panes"). That run had no
churn, and switched only between two workspaces. Its figures agree with the
ones above: anonymous memory +6 MB/h, threads, open files and terminals
flat, CPU 8 % (15 narrow panes always on screen). The first 32-pane attempt
had the same problem and was stopped after a minute. The harness now spreads
the panes over workspaces of at most 15.

## Running it again

The harness lives in `~/dev/bench/harness` (not in this repository):

- `endurance.py <app> <N> [minutes=60] [close_minutes=4]`: the run above;
  one JSON line per sample in `~/dev/bench/endurance/<run-id>.jsonl`, and
  the app's log next to it;
- `endurance_report.py <run.jsonl> [warmup_s=300]`: the tables, slopes and
  ASCII plots;
- `attribute.py <app> <churn|flip|board|none> <every_s> <minutes>`: one
  action repeated quickly;
- `TRIM_LIB=~/dev/bench/endurance/trim/libtrim.so endurance.py ...`: preloads
  a library that calls `malloc_trim(0)` 2 minutes after the panes are closed,
  and logs RSS before and after.

```sh
# the probe build (optional: without it the term./pty. columns are empty),
# in a worktree of its own so that no checkout in use changes branch
git worktree add ../hangar-probe probe/endurance-throwaway && cd ../hangar-probe
cargo build --release --locked -p hangar-app --bin hangar-ai
cp target/release/hangar-ai ~/dev/bench/endurance/bin/hangar-ai-probe

cd ~/dev/bench/harness
python3 endurance.py ~/dev/bench/endurance/bin/hangar-ai-probe 16 60 4
python3 endurance.py ~/dev/bench/endurance/bin/hangar-ai-probe 32 60 4
python3 endurance_report.py ~/dev/bench/endurance/native-endurance32-*.jsonl
```

Each run opens one Hangar window for about 64 minutes and kills it, with
every process it started, at the end (or on Ctrl-C). The run's data root is
under `~/dev/bench/runs/<run-id>/` and can be deleted afterwards.
