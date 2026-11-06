//! What the stats page costs on months of history: the first scan, a scan
//! with nothing new, the answer from the cache alone, and the cache on disk a
//! launch reads back.
//!
//! ```text
//! cargo test --release -p hangar-core --lib stats::aggregate::bench -- --ignored --nocapture
//! ```

use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::stats::{load_disk, usage_stats_at, Roots, Window};

/// Six months, fifteen sessions a day, a hundred exchanges each.
const DAYS: usize = 180;
const SESSIONS_PER_DAY: usize = 15;
const EXCHANGES: usize = 100;

fn transcript(day: usize, session: usize) -> String {
    let mut text = String::new();
    let (month, dom) = (5 + day / 30, 1 + day % 28);
    for exchange in 0..EXCHANGES {
        let (hour, minute) = (8 + exchange / 20, (exchange * 3) % 60);
        let at = format!("2026-{month:02}-{dom:02}T{hour:02}:{minute:02}:00.000Z");
        let id = format!("msg-{day}-{session}-{exchange}");
        let _ = writeln!(
            text,
            r#"{{"type":"user","timestamp":"{at}","sessionId":"s-{day}-{session}","cwd":"/home/dev/app","isSidechain":false,"uuid":"u-{id}","message":{{"role":"user","content":"Make the board faster, exchange {exchange}"}}}}"#
        );
        let _ = writeln!(
            text,
            r#"{{"parentUuid":"p","isSidechain":false,"type":"assistant","timestamp":"{at}","sessionId":"s-{day}-{session}","cwd":"/home/dev/app","requestId":"req-{id}","message":{{"id":"{id}","model":"claude-opus-5-5","role":"assistant","content":[{{"type":"text","text":"{}"}}],"usage":{{"input_tokens":3,"output_tokens":420,"cache_creation_input_tokens":100,"cache_read_input_tokens":20000,"cache_creation":{{"ephemeral_5m_input_tokens":40,"ephemeral_1h_input_tokens":60}},"service_tier":"standard","speed":"standard"}}}}}}"#,
            "Done: the change and what it was checked with. ".repeat(4)
        );
    }
    text
}

fn seed(root: &Path) -> u64 {
    let mut bytes = 0;
    for day in 0..DAYS {
        let project = root.join(format!("-home-dev-app-{}", day % 12));
        fs::create_dir_all(&project).unwrap();
        for session in 0..SESSIONS_PER_DAY {
            let text = transcript(day, session);
            bytes += text.len() as u64;
            fs::write(project.join(format!("s-{day}-{session}.jsonl")), text).unwrap();
        }
    }
    bytes
}

fn resident_kib() -> u64 {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find(|line| line.starts_with("VmRSS:"))
                .and_then(|line| line.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

#[test]
#[ignore = "a benchmark: run it in release with --ignored --nocapture"]
fn stats_on_months_of_history() {
    let root = crate::testdir::scratch("stats-bench");
    let projects = root.join("projects");
    let bytes = seed(&projects);
    let disk = root.join("usage-stats-cache.json");
    let roots = Roots {
        claude: vec![projects],
    };
    let all = Window {
        from: None,
        to: None,
    };
    println!(
        "\n== {} transcripts, {} MiB ==",
        DAYS * SESSIONS_PER_DAY,
        bytes >> 20
    );
    let rss = resident_kib();

    let started = Instant::now();
    let first = usage_stats_at(&roots, Some(&disk), all, false);
    println!(
        "first scan (nothing cached)        {:>9.1} ms  parsed {}  +{} MiB resident",
        ms(started.elapsed()),
        first.scan.parsed,
        (resident_kib().saturating_sub(rss)) >> 10
    );

    for round in 0..3 {
        let started = Instant::now();
        let again = usage_stats_at(&roots, Some(&disk), all, false);
        println!(
            "scan, nothing new ({round})              {:>9.1} ms  reused {}",
            ms(started.elapsed()),
            again.scan.reused
        );
    }
    for round in 0..3 {
        let started = Instant::now();
        let cached = usage_stats_at(&roots, Some(&disk), all, true);
        println!(
            "answer from the cache alone ({round})    {:>9.1} ms  {} buckets",
            ms(started.elapsed()),
            cached.buckets.len()
        );
    }
    let size = fs::metadata(&disk).map(|meta| meta.len()).unwrap_or(0);
    let started = Instant::now();
    let loaded = load_disk(&disk);
    println!(
        "cache on disk: {} KiB, read back in {:.1} ms ({} files)",
        size >> 10,
        ms(started.elapsed()),
        loaded.len()
    );
    println!("resident at the end: {} MiB", resident_kib() >> 10);
}
