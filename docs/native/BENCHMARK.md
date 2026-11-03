# Benchmark: Hangar 1.12.1 and 2.0

This file gives the method and the raw figures behind the table in the
"Performance" section of the [README](../../README.md). It also says what the
figures do not show.

Everything was measured on one machine, on 3 November 2026, in about two hours.
The figures behind the README table are listed trial by trial.

## The figures in the README

| | 1.12.1 | 2.0 | Where it comes from |
|---|---|---|---|
| Start, cold | 0.96 s | 0.57 s | `idle4`, time until all four prompts can be read. Median of 5 trials: 0.962 s and 0.566 s. |
| Memory at rest | 238 MiB | 166 MiB | `idle4`, PSS at 60 s. Median of 5 trials: 238.3 MiB and 165.6 MiB. |
| 16 terminals printing non-stop for 30 s: memory | 8.1 GiB | 249 MiB | `busy16`, PSS at the end of the 30 s. Median of the 4 trials: 8,308 MiB and 249 MiB. |
| 16 terminals printing non-stop for 30 s: processor | 4.4 cores | 2.4 cores | `busy16`, CPU over the 30 s. Median of the 4 trials: 436.6 % and 239.7 % of one core. |
| Echo of text sent to a pane while 15 others print | no echo within a minute | 2.5 ms | `lat16`, median echo with 15 busy panes. 2.0: 2.525 ms over the 150 echoes of the 5 completed trials. 1.12.1: no echo was seen in the 61 s the harness waited, in any trial. |

Every figure is the median of every completed trial. Sizes are in mebibytes
(1 MiB is 1,048,576 bytes): 8,308 MiB is 8.1 GiB, or 8.7 GB in decimal units.
An earlier version of the README and the site gave the medians of the first
three trials of `busy16` and `lat16` (2.7 cores and 2.4 ms), and wrote MiB as
MB. See [Results](#results) and [Limits](#limits).

## What was compared

| | Build |
|---|---|
| 1.12.1 | The Tauri and web view app, built from the `v1.12.1` tag with the release profile. It draws with WebKitGTK 2.52.6 (Ubuntu package). It ran with `WEBKIT_DISABLE_DMABUF_RENDERER=1`, which the 1.x launcher on this machine sets. |
| 2.0 | The native app, built from commit `0f6010c` of the `feat/native-rust` branch, with the release profile. The version in the manifest was still 1.12.1 at that commit. 38 later commits (the 2.0.0 version number, a title bar of its own on Linux, kit and text-size changes, documentation) were not measured. |

Both were built on the machine itself, shortly before the runs, with the Rust
toolchain installed there (1.99.1 when this file was written).

## Machine and software

| | |
|---|---|
| Processor | AMD Ryzen 9 7950X3D, 16 cores, 32 threads |
| Memory | 62,131 MiB visible to the system (60.7 GiB) |
| Graphics | NVIDIA GeForce RTX 4080 SUPER, driver 595.99.02 |
| System | Ubuntu 26.04.1 LTS, Linux 7.0.0-42-generic |
| Desktop | GNOME, Wayland session |
| Displays | Two monitors. One is 2560x1440 at 280 Hz. |
| Python | 3, standard library only |

The machine was also in ordinary use. Before each trial, the harness waited for
a quiet moment (see below), so the background load was small but not zero.

## Method

### What every trial does

- **Fresh data.** Each trial starts the app with new data, config and cache
  folders, and a `state.json` the harness writes. The panes are plain `bash`
  shells in a balanced split layout (2x2 for 4 panes, 4x4 for 16). The settings
  come from the project's `typical.json` test fixture, with auto-resume,
  notifications, the Claude bridge, voice, Discord and hibernation off.
  "Cold" means a new process with no state from an earlier run. The harness does
  not empty the operating system's file cache.
- **Isolation.** The harness removes `HANGAR_*`, `WEBKIT_*` and `WAYLAND_DEBUG`
  from the environment, then sets its own folders and a marker variable. It
  hashes the Hangar data folders used every day before and after each trial.
  None of the 1.12.1 and 2.0 trials changed them. At the end of each trial it
  stops the app and kills anything that still carries the marker. No process was
  left over.
- **Control.** The app writes an endpoint file with a port and a token. The
  harness calls the app's own control API on `127.0.0.1` (`pane_list`,
  `pane_read`, `pane_send`) to read and type in panes. Both apps offer the same
  operations. In 1.12.1 the request is relayed to the window (the web view). When
  the window does not answer, the API says "Hangar's window did not answer in
  time" after 20 s. In 2.0 the operations run on the async runtime, not on the
  UI thread (`crates/hangar-app/src/control/mod.rs`).
- **A prompt is ready** when `pane_read` shows text that ends in `$`, `❯`, `#`,
  `>` or `%`. The shell's own start-up time is in every start figure, for both
  apps.
- **Quiet machine.** Before the app starts, the harness waits until the 1-minute
  load average is under 4 and fewer than 3 cores are busy over 2 s. It waits at
  most 3 minutes (1 minute for `busy16`), then runs anyway and records it. None
  of the `busy16` and `lat16` trials used here ran past that gate. In the
  `idle4` trials, the 1-minute load average at the start was above 4 for
  1.12.1 trials 1 and 3 (4.42, 5.10) and 2.0 trial 1 (5.85). Their figures are
  in line with the others.
- **Order.** The apps were run one after the other, rotated for each repetition,
  so slow changes on the machine fall on all of them.

### What is measured, and how

- **Memory.** The "app" is the app's process tree plus any process that carries
  the trial's marker. For 1.12.1 that is the main process, `WebKitWebProcess`,
  `WebKitNetworkProcess` and a few small helpers (`bwrap`, `glycin-svg`). For 2.0
  it is the main process alone. The panes' shells and what they run are not in
  the app's figures.
  - **PSS** (proportional set size) is the figure the README uses. It counts a
    page shared by several processes once in total, split among them.
  - **RSS** counts such a page in full in every process, so it overstates a tree
    of several processes. It is given as well.
  - Both come from `/proc` (`smaps_rollup` and `stat`), in MiB.
- **Processor.** The sum of user and system time of the app's processes over a
  window, divided by the window. 100 % is one core, so 438.5 % is 4.4 cores. The
  panes' shells and `ls` are counted apart.
- **Graphics memory.** In `idle4` and `busy16`, `nvidia-smi` is asked once per
  trial for the app's processes. It is not in the memory figures.
- **Was the window drawn?** Some trials ran with `WAYLAND_DEBUG=1`. From that
  log, `frames.py` counts the `wl_surface.commit` calls on the window per
  second, and how long the window's frame callbacks went unanswered by the
  compositor. A window that the compositor does not paint (another workspace,
  covered) gets no frame callbacks, and the app then skips drawing. Its CPU
  figure then leaves out the drawing.

## Scenarios

| Scenario | What it does | Measured | Trials |
|---|---|---|---|
| `idle4` | Starts the app with 4 shell panes and leaves it alone. | Time until each milestone. Memory 30 s after launch and again at 60 s. CPU from 30 s to 60 s. Graphics memory at 30 s. | 5 for each app |
| `map4` | Starts the app with 4 shell panes, with the Wayland log on. At 12 s, it watches 5 s. | When the window first gets a buffer. Commits per second when idle. | 3 for each app |
| `busy16` | Starts 16 shell panes. Types a loop into each: `ls --color=always -la /usr/bin`, repeated, and counts the passes. A FIFO holds all 16 until one moment (about 8 s after launch), so they start together. About 150 KB of output per pass (the summary script assumes 154,496 bytes). After 3 more seconds it measures for 30 s. | CPU over the 30 s. Memory at the end of the 30 s (and every second). `ls` passes. Whether `pane_list` answers at the end. How long the app takes to show the loops' last line and to calm down after a stop. | 1.12.1: 4 (trials 1, 2, 3, 9). 2.0: 4 (1, 2, 3, 9). |
| `lat16` | Starts 16 shell panes. In pane P1 (quiet), measures echo with the app idle. Then starts the loops in the other 15 panes, waits 3 s, and measures echo again. | Echo time, `pane_read` and `pane_send` round trips. | 1.12.1: 6 completed of 7 started. 2.0: 5 completed of 8 started. |

Details:

- **`busy16` limit.** The harness writes a stop file when the app's RSS passes
  12,000 MiB, so that a runaway app cannot fill the machine. That is the
  "memory guard" below. After the 30 s, the harness also stops the loops and
  waits up to 240 s for the app's CPU to fall under 30 % for 3 seconds.
- **`lat16` echo.** The harness sends a marker (`zq` and 8 random letters) to P1
  with `pane_send`, not submitted, as if pasted. It then calls `pane_read` in a
  loop until the marker shows. The echo time runs from just before the
  `pane_send` call to the first `pane_read` that shows the marker, so it
  includes the `pane_send` round trip. It sends Ctrl+C, waits 50 to 150 ms, and
  repeats. Each phase is 50 `pane_read` round trips, then 30 echoes, with a
  budget of 60 s. When the app answers, a phase takes about 5 s.
- **Trials that failed.** `lat16` trial 6 of 1.12.1 and trials 4, 5 and 6 of 2.0
  ended with "no prompt in P1" before measuring anything: the first pane's
  prompt was not seen. The cause was not recorded. They have no figures.
- **Why there are more than three trials.** The series (`matrix.sh`) is 5
  trials of `idle4` and 3 of each other scenario, then one more `busy16` per app
  (trial 9) with the Wayland log on. `rerun_lat.sh` repeated `lat16` "at low
  load" (trials 4 to 6). Trials 7 and 8 of 2.0 and trial 7 of 1.12.1 are in
  neither script's log.

## Results

All sizes are in MiB. All times are medians unless a row says otherwise. Ranges
are minimum to maximum.

### Start

Time from exec until all four panes show a prompt (`idle4`):

| Trial | 1.12.1 | 2.0 |
|---|---|---|
| 1 | 0.962 s | 0.566 s |
| 2 | 0.909 s | 0.558 s |
| 3 | 0.966 s | 0.557 s |
| 4 | 0.909 s | 0.575 s |
| 5 | 0.979 s | 0.573 s |
| **Median** | **0.962 s** | **0.566 s** |

If only the trials that started with a 1-minute load average under 4 are kept
(`summarize.py` does this for its start table), the medians are 0.909 s for
1.12.1 (trials 2, 4, 5) and 0.566 s for 2.0 (trials 2 to 5).

The milestones on the way (median of the 5 `idle4` trials):

| Milestone | 1.12.1 | 2.0 |
|---|---|---|
| Endpoint file written | 0.244 s | 0.009 s |
| Control API answers `pane_list` | 0.731 s | 0.390 s |
| First pane's prompt readable | 0.950 s | 0.554 s |
| All four prompts readable | 0.962 s | 0.566 s |
| Window gets its first buffer (`map4`, 3 trials) | 0.263 s | 0.378 s |

So the window of 1.12.1 gets its first buffer first (0.26 s against 0.38 s),
and 2.0 is ready to use first. The README figure is the time to a usable app,
not the time to the first window.

### Memory at rest

`idle4`, 4 shell panes, nothing running. PSS at 60 s after launch:

| Trial | 1.12.1 | 2.0 |
|---|---|---|
| 1 | 241.6 | 177.4 |
| 2 | 238.1 | 177.2 |
| 3 | 238.3 | 165.6 |
| 4 | 236.0 | 164.9 |
| 5 | 240.4 | 165.6 |
| **Median** | **238.3** | **165.6** |

Other measures, median of the 5 trials (range):

| | 1.12.1 | 2.0 |
|---|---|---|
| PSS at 30 s | 247.0 (244.1 to 249.2) | 165.6 (158.0 to 177.4) |
| PSS at 60 s | 238.3 (236.0 to 241.6) | 165.6 (164.9 to 177.4) |
| RSS at 30 s | 515.7 (513.9 to 516.4) | 281.0 (279.9 to 281.8) |
| RSS at 60 s | 496.7 (494.6 to 497.1) | 281.0 (279.9 to 281.8) |
| Anonymous memory at 30 s | 121.1 (119.8 to 121.9) | 58.0 (57.8 to 58.0) |
| Private memory at 30 s | 184.7 (179.6 to 187.0) | 138.6 (136.3 to 153.7) |
| CPU, 30 s to 60 s (% of one core) | 0.0 (0.0 to 0.1) | 0.1 (0.1 to 0.3) |
| Graphics memory (`nvidia-smi`) | 3 | 72 |

The four panes' `bash` shells add about 24 MiB of RSS in both cases. They are
not in these figures.

`summarize.py` prints PSS at 30 s (247 MiB for 1.12.1). The README uses PSS at
60 s (238 MiB).

### 16 terminals printing non-stop

`busy16`. CPU is over the 30 s window. Memory is at its end, about 41 s after
launch.

| Trial | 1.12.1 CPU | 1.12.1 PSS | 1.12.1 RSS | 2.0 CPU | 2.0 PSS | 2.0 RSS |
|---|---|---|---|---|---|---|
| 1 | 434.7 % | 8,332 | 8,597 | 267.2 % | 249 | 362 |
| 2 | 438.5 % | 8,285 | 8,548 | 212.2 % | 249 | 363 |
| 3 | 448.2 % | 8,592 | 8,847 | 268.8 % | 249 | 361 |
| 9 | 413.1 % | 7,045 | 7,302 | 211.2 % | 244 | 361 |
| Median, trials 1 to 3 | 438.5 % | 8,332 | 8,597 | 267.2 % | 249 | 362 |
| **Median, all four** | **436.6 %** | **8,308** | **8,572** | **239.7 %** | **249** | **361** |

The README uses the medians of all four trials: 4.4 and 2.4 cores, 8.1 GiB and
249 MiB.

Was the window drawn in each trial?

| Trial | 1.12.1 | 2.0 |
|---|---|---|
| 1 | No Wayland log. | No Wayland log. The UI thread used 55 % of a core. |
| 2 | No frame committed in the 30 s. The compositor left the frame callbacks unanswered for 29.9 s. | No Wayland log. The UI thread used less than 14 %. |
| 3 | No frame committed in the 30 s. | 479 commits per second, which is two per frame callback, at about 240 callbacks per second. The UI thread used 52 %. |
| 9 | No frame committed in the 30 s. | No frame committed. The compositor left the frame callbacks unanswered for all 30 s. The UI thread used 0.6 %. |

So the 2.0 CPU figure is higher when the window is drawn (about 2.7 cores in
trials 1 and 3, judging by the UI thread for trial 1) than when it is not (2.1
cores in trials 2 and 9). The 1.12.1 window drew nothing in any trial that has a
log. Its CPU is the cost of taking in the output, while the window does not
update.

Other figures, median of the four trials (range):

| | 1.12.1 | 2.0 |
|---|---|---|
| `ls` passes in 30 s, all 16 panes | 16,775 (15,753 to 17,169) | 18,094 (17,725 to 18,315) |
| Output read from the panes, MB/s (passes x 154,496 bytes) | 86 (81 to 88) | 93 (91 to 94) |
| Panes' shells and `ls`, in cores | 15.6 | 15.8 |
| Whole machine, in cores (of 32 threads) | 24.9 | 22.5 |
| App RSS before the loops start | 566 (556 to 570) | 284 (283 to 286) |
| App PSS before the loops start | 297 (287 to 300) | 170 (164 to 171) |
| App RSS, peak of the 1 s samples | 15,814 (15,551 to 16,134) | 362 (361 to 363) |
| Memory guard (RSS over 12,000 MiB) | Reached in 4 of 4 trials, 46 to 52 s after the loops started | Not reached |
| Main process, main thread (% of a core) | 90 to 96 | 55, under 14, 52, 0.6 |
| `WebKitWebProcess`, main thread (% of a core) | 97 to 99 | none |
| Graphics memory (`nvidia-smi`) | 3 | 144 |
| `pane_list` at the end of the 30 s | No answer in 20 s (the API's limit), 4 of 4 | 1.2 ms (1.1 to 5.2) |
| After the stop: time until all 16 panes show the loops' last line | None did within 30 s, 4 of 4 | 0.06 s (0.05 to 0.06) |
| After the stop: time until the app's CPU is under 30 % | 175 s (155 to 185) | 0.3 to 0.4 s |
| App RSS after the stop signal (the harness waits up to 30 s for the panes) | 14,176 (13,562 to 14,637) | 362 |

In 1.12.1 the guard wrote the stop file in every trial. The loops were ended by
the harness, not by the app running out of memory.

### Echo with 15 terminals printing

`lat16`. Echo of the marker in the quiet pane P1.

2.0, with 15 panes printing (all times in ms; the "window drawn" column comes
from the Wayland log):

| Trial | Echo, idle app (median) | Echo, 15 busy (median) | Echo, 15 busy (90th percentile) | `pane_send`, 15 busy (median) | Window drawn |
|---|---|---|---|---|---|
| 1 | 0.95 | 2.22 | 3.03 | 1.53 | No |
| 2 | 1.05 | 2.48 | 3.00 | 1.75 | No |
| 3 | 0.94 | 2.35 | 3.18 | 1.60 | No |
| 7 | 1.22 | 2.49 | 3.25 | 1.70 | No |
| 8 | 1.68 | 3.00 | 4.16 | 1.72 | Yes (about 556 commits per second) |

With 15 busy panes, the median of the 150 echoes of the five trials is
**2.5 ms** (2.525), with a 90th percentile of 3.62 ms and a slowest echo of
6.06 ms; that is the README's figure. Trials 1 to 3 alone (90 echoes) give
2.35 ms, with a 90th percentile of 3.11 ms. The median of the five trial medians is 2.48 ms. With the app
idle, the median of the 150 echoes is 1.15 ms.

1.12.1:

| Trial | Echo, idle app (median) | 15 busy: first `pane_read` | 15 busy: `pane_send` | 15 busy: echo |
|---|---|---|---|---|
| 1 | 1.23 ms | answered after 10.9 s | no answer in 20 s | not seen in 61 s |
| 2 | 1.75 ms | answered after 11.6 s | no answer in 20 s | not seen in 61 s |
| 3 | 1.64 ms | answered after 11.5 s | no answer in 20 s | not seen in 61 s |
| 4 | 1.53 ms | answered after 10.5 s | no answer in 20 s | not seen in 61 s |
| 5 | 2.10 ms | answered after 10.3 s | no answer in 20 s | not seen in 61 s |
| 7 | 1.88 ms | answered after 10.5 s | no answer in 20 s | not seen in 61 s |

With the app idle, the median of the 180 echoes is 1.73 ms. With 15 panes
printing, the second `pane_read` of each trial failed, the `pane_send` got no
answer, and the harness did not see the marker in the 61 s it allowed. There is
no 1.12.1 echo time to report: the README says "no echo within a minute", for
the 61 s the harness waited. The API gives up on each call after 20 s.

Round trips with the app idle, median: `pane_read` 0.65 ms (1.12.1) and 0.27 ms
(2.0); `pane_send` 0.84 ms and 0.73 ms.

## Limits

- **One machine, a few trials.** 3 to 5 trials each, on one computer, one
  desktop and one display set-up. The ranges above show how much they vary. The
  machine was in ordinary use, with a load of 0.2 to 2.0 busy cores at the
  start of the `busy16` trials.
- **The window was not always drawn.** The harness does not control whether the
  compositor paints the window, and it was often not painting it. Where it was
  not, the app skips drawing and its CPU does not include it. For 2.0 in
  `busy16`, that is the difference between about 2.7 and 2.1 cores. In `lat16`,
  the window of 2.0 was drawn only in trial 8, which has the slowest echo (3.00
  ms). In `busy16`, the window of 1.12.1 drew nothing in the three trials that
  have a log, so its CPU figure holds no drawing either.
- **2.0 draws on a fast display.** In the one drawn `busy16` trial, the window
  got about 240 frame callbacks per second. On a 60 Hz display it would get at
  most 60, so the drawing part of the CPU figure would be smaller.
- **Echo is not a key press.** The marker goes in through the control API and is
  read back through it. The harness does not press keys in the window and does
  not time any drawing. In 2.0 that path does not go through the UI thread. In
  1.12.1 it goes through the window, which fits a busy window stopping it from
  answering. A real key press in 1.12.1 needs the same web view, so it is
  likely stalled too, but that was not measured.
- **1.12.1 never gave an echo time.** "No echo" is the result. It is not a
  number: the minute is how long the harness waited, not a measured delay.
- **The memory guard.** The guard stops the loops when the app passes 12,000
  MiB of RSS. 1.12.1 reached it in every `busy16` trial. The 8.1 GiB figure is
  the PSS at the end of the 30 s window (33 s after the loops started), before
  the guard. The memory of 1.12.1 was still growing at that point: its RSS was
  8,572 MiB then (median), and passed 12,000 MiB some 15 s later.
- **PSS and RSS.** The README gives PSS. By RSS the gaps are 496.7 against 281.0
  MiB at rest and 8,572 against 361 MiB under load.
- **Graphics memory is separate.** 2.0 used 72 MiB at rest and 144 MiB under
  load on the graphics card. 1.12.1 used 3 MiB. The figures above do not
  include it.
- **1.12.1 ran with the DMA-BUF renderer off.** That is how the 1.x launcher on
  this machine starts it. The harness did not run it the other way.
- **Start is time to a usable app.** It includes the shell's own start-up, for
  both apps, and a warm file cache. The 1.12.1 window appears sooner (0.26 s
  against 0.38 s) and fills in later.
- **A snapshot of 2.0.** The commit measured is not the last one of the branch
  (see [What was compared](#what-was-compared)).
- **Failed trials.** 4 `lat16` trials (1.12.1: one, 2.0: three) failed before
  measuring and have no figures.
- **Not measured.** Input latency with a real key press, drawing latency, other
  platforms, and long-running memory use.

## Running it again

The harness, a few Python scripts, is not in this repository yet: its scripts
still name the folders of the machine it ran on. What follows assumes it in a
`bench/` folder, with the two builds beside it. It needs Linux (it reads
`/proc`), Python 3, a build of each app, and a Wayland session for the frame
figures. `nvidia-smi`
is optional.

Before the first run, edit the paths in the scripts: `BENCH`, `APPS`,
`REAL_STATE` and `WATCHED` at the top of `bench/harness/bench.py` (the folder,
the command of each app, and the Hangar data folders to leave untouched), and the
`cd` line of `matrix.sh` and `rerun_lat.sh`. As written, `bench.py` expects the
1.12.1 build at `bench/hangar-old/src-tauri/target/release/hangar-ai` and the
native build at `bench/hangar-native/target/release/hangar-ai`. It also reads the
fixture `bench/hangar-native/crates/hangar-app/src/state/fixtures/typical.json`.

Leave the machine alone while a trial runs, and keep the window on the active
workspace and uncovered. Check `frames.py` afterwards to see whether it was drawn.

One trial:

```
python3 -I bench/harness/bench.py <app> <scenario> <trial> [--wayland-debug]
```

- `<app>` is `old121` (1.12.1), `native` (2.0) or `old120` (the installed 1.12.0
  package, not used here).
- `<scenario>` is `idle4`, `map4`, `busy16` or `lat16`.
- `<trial>` is a number that goes in the file name. The harness writes
  `bench/results/<app>-<scenario>-<trial>-<time>.json`, and the app's output and
  Wayland log next to it as `.log`.

The whole series, apps rotated:

```
bench/harness/matrix.sh
bench/harness/rerun_lat.sh
```

Wayland figures and summary tables:

```
python3 -I bench/harness/frames.py bench/results
python3 -I bench/harness/summarize.py bench/results
```

`frames.py` writes `frames.json` next to the results. `summarize.py` prints
Markdown tables of medians to the terminal. Its start and latency tables keep
only the trials that started on a quiet machine, and its `idle4` table shows PSS
at 30 s. Its `busy16` and `lat16` tables use every completed trial, so for 2.0 it
shows 2.4 cores and 2.5 ms, the README's figures.

In `bench/results/`, files that begin with `old120` are 1.12.0 runs, which this
file does not use. `bench/results/trials/` holds the trial-0 runs made while the
harness was being written. They are not used either.

`bench/harness/probe.py` and `probe_redraw.py` are small helpers for setting the
harness up: the first prints what `pane_read` returns from a freshly started
app, the second checks whether polling makes the app redraw.
