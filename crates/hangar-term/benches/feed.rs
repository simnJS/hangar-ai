//! What parsing a pane's output costs: `cargo bench -p hangar-term --bench feed`.
//!
//! It feeds recorded output to a [`Terminal`] of 200x50 with 10 000 lines of
//! scrollback, the way a pane's reader thread does (chunks of 8 KiB by
//! default; core's reader now hands out what the PTY holds, up to 64 KiB,
//! which `--chunk 65536` matches), and reports MB/s per sample:
//!
//! - `ls`: `ls --color=always -la /usr/bin` as a PTY delivers it (`\r\n`
//!   line ends), from `benches/fixtures/ls-usr-bin.txt`; the "16 busy
//!   terminals" scenario prints it in a loop.
//! - `claude`: what an agent's interface prints, generated here: redraws in
//!   synchronized updates, box drawing, wide and accented characters, cursor
//!   moves, truecolor.
//!
//! Options, after `--`:
//!
//! - `--passes N`: how many times each sample is fed (default 40).
//! - `--sample ls|claude`: only that sample.
//! - `--chunk BYTES`: the size of each feed (default 8192; 0 feeds each pass
//!   in one call).
//! - `--profile DIR` (unix): samples the run with `pprof` (signal based, needs
//!   no perf permissions) and writes `DIR/feed-<sample>.svg`, a flamegraph.
//! - `--scrollback LINES`: the terminal's history (default 10 000).
//! - `--threads N`: N terminals fed at once, one thread each, as sixteen
//!   busy panes are; the MB/s is theirs together (default 1).
//! - `--pty` (unix): also the real path, `pty-<sample>`: a shell `cat`s the
//!   sample `--passes` times into a PTY spawned by core's `pty_spawn`, and
//!   the sink feeds the terminal under its lock as the app's does. Its MB/s
//!   is bounded by the PTY and `cat`; what it tells is the reader thread's
//!   CPU time (reading, decoding, history, feeding) per MB, and the size of
//!   the chunks the reader hands out.
//! - `--ls` (with `--pty`): the shell runs `ls --color=always -la /usr/bin`
//!   instead of `cat`ting the sample, as each of bench `busy16`'s panes
//!   does: written to the PTY a line at a time (stdout is line-buffered on
//!   a terminal), which is what makes the reader hand out small pieces.
//! - `--no-feed` (with `--pty`): the sink drops the output, so what is left
//!   is the reader thread's own cost.

use std::time::{Duration, Instant};

use hangar_term::{Config, Terminal};

const COLUMNS: u16 = 200;
const ROWS: u16 = 50;
const SCROLLBACK: usize = 10_000;
const LS: &[u8] = include_bytes!("fixtures/ls-usr-bin.txt");

/// A few seconds of an agent's interface: each frame redraws a box of
/// transcript lines and a status line inside a synchronized update, as
/// Claude Code's renderer does, then the transcript scrolls on.
fn claude_sample() -> Vec<u8> {
    let words = [
        "Reading",
        "crates/hangar-term/src/terminal.rs",
        "→",
        "fn feed(&mut self, bytes: &[u8])",
        "déjà",
        "naïve",
        "日本語",
        "✓",
        "⏺",
        "…",
        "résumé",
        "🙂",
        "Update(",
        "Bash(cargo test)",
    ];
    let mut out = String::new();
    let mut state = 0x9e37_79b9_u32;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    let width = COLUMNS as usize - 4;
    for frame in 0..400 {
        out.push_str("\x1b[?2026h");
        // The transcript: a few new lines in the scrolling region.
        for _ in 0..3 {
            out.push_str("\x1b[38;2;215;119;87m⏺\x1b[39m ");
            let mut used = 2;
            while used < width - 20 {
                let word = words[next() as usize % words.len()];
                let r = next() % 256;
                let g = next() % 256;
                out.push_str(&format!("\x1b[38;2;{r};{g};180m{word}\x1b[0m "));
                used += word.chars().count() + 1;
            }
            out.push_str("\r\n");
        }
        // The prompt box, redrawn in place below.
        out.push_str("\x1b7\x1b[44;1H\x1b[J");
        out.push_str("\x1b[2m╭");
        out.push_str(&"─".repeat(width));
        out.push_str("╮\x1b[0m\r\n");
        for row in 0..3 {
            out.push_str("\x1b[2m│\x1b[0m ");
            let text = if row == 0 { "> " } else { "" };
            out.push_str(text);
            out.push_str(&" ".repeat(width - 1 - text.len()));
            out.push_str("\x1b[2m│\x1b[0m\r\n");
        }
        out.push_str("\x1b[2m╰");
        out.push_str(&"─".repeat(width));
        out.push_str("╯\x1b[0m\r\n");
        let spinner = ["✻", "✶", "✳", "✢", "·"][frame % 5];
        out.push_str(&format!(
            "\x1b[38;5;174m{spinner} Pondering… \x1b[2m({frame}s · ↑ {} tokens · esc to interrupt)\x1b[0m\x1b[K",
            frame * 37
        ));
        out.push_str("\x1b8\x1b[?2026l");
    }
    out.into_bytes()
}

struct Options {
    passes: usize,
    sample: Option<String>,
    chunk: usize,
    profile: Option<String>,
    scrollback: usize,
    threads: usize,
    pty: bool,
    ls: bool,
    feed: bool,
}

fn options() -> Options {
    let mut options = Options {
        passes: 40,
        sample: None,
        chunk: 8192,
        profile: None,
        scrollback: SCROLLBACK,
        threads: 1,
        pty: false,
        ls: false,
        feed: true,
    };
    let mut args = std::env::args().skip(1);
    let number = |args: &mut std::iter::Skip<std::env::Args>, default: usize| {
        args.next().and_then(|n| n.parse().ok()).unwrap_or(default)
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--passes" => options.passes = number(&mut args, 40),
            "--sample" => options.sample = args.next(),
            "--chunk" => options.chunk = number(&mut args, 8192),
            "--profile" => options.profile = args.next(),
            "--scrollback" => options.scrollback = number(&mut args, SCROLLBACK),
            "--threads" => options.threads = number(&mut args, 1).max(1),
            "--pty" => options.pty = true,
            "--ls" => options.ls = true,
            "--no-feed" => options.feed = false,
            // `cargo bench` passes `--bench`; anything else is ignored too.
            _ => {}
        }
    }
    options
}

fn terminal(options: &Options) -> Terminal {
    let config = Config {
        scrollback: options.scrollback,
        ..Config::default()
    };
    Terminal::new(config, COLUMNS, ROWS)
}

/// How long a run took, by the clock and in CPU time.
#[derive(Clone, Copy, Default)]
struct Timing {
    wall: Duration,
    cpu: Duration,
}

/// `--threads` terminals fed `sample` at once: the time until the last one
/// is done, and the CPU time of them all.
fn run(sample: &[u8], options: &Options) -> Timing {
    let ready = std::sync::Barrier::new(options.threads);
    std::thread::scope(|scope| {
        let runs: Vec<_> = (0..options.threads)
            .map(|_| scope.spawn(|| run_terminal(sample, options, &ready)))
            .collect();
        runs.into_iter()
            .map(|run| run.join().expect("a feeding thread"))
            .fold(Timing::default(), |all, one| Timing {
                wall: all.wall.max(one.wall),
                cpu: all.cpu + one.cpu,
            })
    })
}

/// One terminal fed `--passes` times, `--chunk` bytes per call, as the
/// reader thread would, replies and all; timed from when every thread is
/// ready.
fn run_terminal(sample: &[u8], options: &Options, ready: &std::sync::Barrier) -> Timing {
    let mut terminal = terminal(options);
    let chunk = match options.chunk {
        0 => sample.len(),
        chunk => chunk,
    };
    // The scrollback filled first, as a busy pane's soon is: from then on,
    // every new line recycles the oldest one.
    let lines = sample.iter().filter(|&&b| b == b'\n').count().max(1);
    for _ in 0..options.scrollback.div_ceil(lines) + 1 {
        terminal.feed(sample);
    }
    ready.wait();
    let start = Instant::now();
    let cpu = thread_cpu();
    for _ in 0..options.passes {
        for piece in sample.chunks(chunk) {
            std::hint::black_box(terminal.feed(piece));
        }
    }
    let timing = Timing {
        wall: start.elapsed(),
        cpu: thread_cpu() - cpu,
    };
    std::hint::black_box(&terminal);
    timing
}

/// The CPU time of the calling thread: what a busy machine distorts less
/// than the time by the clock.
#[cfg(unix)]
fn thread_cpu() -> Duration {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: a valid pointer to a timespec, and a clock every unix has.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut now) };
    Duration::new(now.tv_sec as u64, now.tv_nsec as u32)
}

/// Not measured here: reported as zero.
#[cfg(not(unix))]
fn thread_cpu() -> Duration {
    Duration::ZERO
}

/// What the reader thread of a PTY `cat`ting a sample did.
#[cfg(unix)]
struct PtyRun {
    wall: Duration,
    /// The reader thread's CPU time, as it stood when the shell ended.
    cpu: Duration,
    bytes: usize,
    chunks: usize,
}

/// `cat`s `sample` `--passes` times into a PTY of core's, the terminal fed
/// from its sink on the reader thread, as a pane's is.
#[cfg(unix)]
fn run_pty(name: &str, sample: &[u8], options: &Options) -> PtyRun {
    use std::sync::{Arc, Mutex, mpsc};

    use hangar_core::pty::{PtyEvent, PtyManager, pty_kill, pty_spawn};

    // The terminal driver turns `\n` into `\r\n` itself.
    let lf: Vec<u8> = sample.iter().copied().filter(|&b| b != b'\r').collect();
    let dir = std::env::temp_dir().join(format!("hangar-feed-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a temporary folder");
    let file = dir.join(format!("{name}.txt"));
    std::fs::write(&file, &lf).expect("the sample's file");

    let terminal = Arc::new(Mutex::new(terminal(options)));
    let manager = Arc::new(PtyManager::default());
    let (done, finished) = mpsc::channel();
    let fed = terminal.clone();
    let feed = options.feed;
    let (mut bytes, mut chunks) = (0usize, 0usize);
    let writer = match options.ls {
        true => "command ls --color=always -la /usr/bin".to_string(),
        false => format!("cat '{}'", file.display()),
    };
    let script = format!(
        "i=0; while [ $i -lt {} ]; do {writer}; i=$((i+1)); done",
        options.passes,
    );
    let shell = hangar_core::shells::ShellInfo {
        id: "sh".into(),
        label: "sh".into(),
        program: "/bin/sh".into(),
        args: vec!["-c".into(), script],
    };
    let id = format!("bench-{name}");
    let start = Instant::now();
    pty_spawn(
        &manager,
        Box::new(move |event| match event {
            PtyEvent::Output(output) => {
                bytes += output.data.len();
                chunks += 1;
                if feed {
                    let events = fed
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .feed(output.data.as_bytes());
                    std::hint::black_box(events);
                }
            }
            PtyEvent::Exit(_) => {
                let _ = done.send((thread_cpu(), bytes, chunks));
            }
        }),
        id.clone(),
        dir.to_string_lossy().into_owned(),
        Some(shell),
        COLUMNS,
        ROWS,
        None,
        Some(false),
        None,
        None,
        None,
        None,
        None,
    )
    .expect("the PTY starts");
    let (cpu, bytes, chunks) = finished
        .recv_timeout(Duration::from_secs(300))
        .expect("the shell ends");
    let wall = start.elapsed();
    let _ = pty_kill(&manager, id, None);
    let _ = std::fs::remove_dir_all(&dir);
    std::hint::black_box(&terminal);
    PtyRun {
        wall,
        cpu,
        bytes,
        chunks,
    }
}

#[cfg(unix)]
fn report_pty(name: &str, sample: &[u8], options: &Options) {
    let go = || run_pty(name, sample, options);
    let run = match &options.profile {
        Some(dir) => profiled(dir, &format!("pty-{name}"), go),
        None => go(),
    };
    let mb = run.bytes as f64 / 1e6;
    println!(
        "{:>11}: {:>8.1} MB in {:>7.1} ms = {:>6.1} MB/s; reader thread {:>7.1} ms CPU = {:>6.1} MB per CPU-second; {} chunks of {:.0} bytes",
        format!("pty-{name}"),
        mb,
        run.wall.as_secs_f64() * 1e3,
        mb / run.wall.as_secs_f64(),
        run.cpu.as_secs_f64() * 1e3,
        mb / run.cpu.as_secs_f64(),
        run.chunks,
        run.bytes as f64 / run.chunks.max(1) as f64,
    );
}

#[cfg(not(unix))]
fn report_pty(_name: &str, _sample: &[u8], _options: &Options) {
    eprintln!("    --pty is unix only");
}

#[cfg(unix)]
fn profiled<T>(dir: &str, name: &str, run: impl FnOnce() -> T) -> T {
    let guard = pprof::ProfilerGuardBuilder::default()
        .frequency(997)
        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
        .build()
        .expect("the profiler starts");
    let result = run();
    let report = guard.report().build().expect("a profile");
    std::fs::create_dir_all(dir).expect("the profile's folder");
    let path = format!("{dir}/feed-{name}.svg");
    let file = std::fs::File::create(&path).expect("the flamegraph's file");
    report.flamegraph(file).expect("the flamegraph");
    eprintln!("    flamegraph: {path}");
    result
}

#[cfg(not(unix))]
fn profiled<T>(_dir: &str, _name: &str, run: impl FnOnce() -> T) -> T {
    eprintln!("    --profile needs pprof, which is unix only");
    run()
}

fn main() {
    let options = options();
    let claude = claude_sample();
    let samples: [(&str, &[u8]); 2] = [("ls", LS), ("claude", &claude)];
    println!(
        "feeding {} terminal(s) of {COLUMNS}x{ROWS} ({} lines of scrollback, full), {} passes, {} per feed",
        options.threads,
        options.scrollback,
        options.passes,
        match options.chunk {
            0 => "a whole pass".to_string(),
            chunk => format!("{chunk} bytes"),
        }
    );
    for (name, sample) in samples {
        if options.sample.as_deref().is_some_and(|only| only != name) {
            continue;
        }
        let go = || run(sample, &options);
        let timing = match &options.profile {
            Some(dir) => profiled(dir, name, go),
            None => go(),
        };
        let mb = (sample.len() * options.passes * options.threads) as f64 / 1e6;
        println!(
            "{name:>11}: {:>7} bytes x {} x {} in {:>8.1} ms = {:>7.1} MB/s; {:>7.1} MB per CPU-second",
            sample.len(),
            options.passes,
            options.threads,
            timing.wall.as_secs_f64() * 1e3,
            mb / timing.wall.as_secs_f64(),
            mb / timing.cpu.as_secs_f64(),
        );
        if options.pty {
            report_pty(name, sample, &options);
        }
    }
}
