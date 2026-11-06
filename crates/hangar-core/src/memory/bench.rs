//! What the memory costs the agents, measured: the work behind
//! `memory_write`, `memory_search` and `memory_list` (server.rs) on a full
//! memory, one caller at a time and then 16 at once.
//!
//! ```text
//! cargo test --release -p hangar-core --lib memory::bench -- --ignored --nocapture
//! ```

use std::sync::Arc;
use std::time::Instant;

use super::*;
use crate::board::bench::{timed, Latencies};

/// A fact the size agents write them: a few paragraphs.
fn entry(i: usize) -> MemoryEntry {
    MemoryEntry {
        id: format!("m-{i:05}"),
        title: format!("Fact {i} about the build and where its cache lives"),
        content: format!(
            "{} The cargo target folder {i} is shared between worktrees. ",
            "How the project builds, which flags matter, what broke last time. ".repeat(20)
        ),
        tags: vec!["build".into(), format!("area-{}", i % 9)],
        workspace: format!("/home/dev/project-{}", i % 5),
        agent: "worker".into(),
        created_at: 1_760_000_000_000 + i as u64,
        updated_at: 1_760_000_000_000 + i as u64,
    }
}

fn seeded(entries: usize) -> (PathBuf, usize) {
    let dir = crate::testdir::scratch("memory-bench");
    let memory = Memory {
        entries: (0..entries).map(entry).collect(),
    };
    write_memory(&dir, &memory).unwrap();
    let bytes = fs::metadata(memory_path(&dir)).unwrap().len() as usize;
    (dir, bytes)
}

fn query(q: &str) -> SearchQuery {
    SearchQuery {
        q: q.into(),
        ..SearchQuery::default()
    }
}

const ROUNDS: usize = 60;

fn single_caller(entries: usize) {
    let (dir, bytes) = seeded(entries);
    let store = MemoryStore::default();
    println!(
        "\n== memory of {entries} entries ({} KiB on disk) ==",
        bytes / 1024
    );
    let (mut lists, mut searches, mut writes) = (Vec::new(), Vec::new(), Vec::new());
    for round in 0..ROUNDS {
        timed(&mut lists, || list(&store.read(&dir).unwrap(), None, None));
        timed(&mut searches, || {
            search(&store.read(&dir).unwrap(), &query("cargo worktrees"))
        });
        // An upsert on a title already there: the memory is full.
        timed(&mut writes, || {
            store
                .update(&dir, |memory| {
                    upsert(
                        memory,
                        NewMemory {
                            title: format!(
                                "Fact {} about the build and where its cache lives",
                                round % entries
                            ),
                            content: format!("Rewritten in round {round}."),
                            ..NewMemory::default()
                        },
                    )
                })
                .unwrap()
        });
    }
    println!("{}", Latencies(lists).line("memory_list"));
    println!("{}", Latencies(searches).line("memory_search"));
    println!("{}", Latencies(writes).line("memory_write (upsert)"));
}

fn sixteen_agents(entries: usize) {
    let (dir, _) = seeded(entries);
    let dir = Arc::new(dir);
    let store: Arc<MemoryStore> = Arc::default();
    let started = Instant::now();
    let handles: Vec<_> = (0..16)
        .map(|agent| {
            let (dir, store) = (dir.clone(), store.clone());
            std::thread::spawn(move || {
                let (mut searches, mut writes) = (Vec::new(), Vec::new());
                for call in 0..20 {
                    if call % 4 == 3 {
                        timed(&mut writes, || {
                            store.update(&dir, |memory| {
                                upsert(
                                    memory,
                                    NewMemory {
                                        title: format!(
                                            "Fact {} about the build and where its cache lives",
                                            (agent * 20 + call) % entries
                                        ),
                                        content: "Rewritten under load.".into(),
                                        ..NewMemory::default()
                                    },
                                )
                            })
                        })
                        .unwrap();
                    } else {
                        timed(&mut searches, || {
                            search(&store.read(&dir).unwrap(), &query("cargo"))
                        });
                    }
                }
                (searches, writes)
            })
        })
        .collect();
    let (mut searches, mut writes) = (Vec::new(), Vec::new());
    for handle in handles {
        let (s, w) = handle.join().unwrap();
        searches.extend(s);
        writes.extend(w);
    }
    let wall = started.elapsed();
    println!(
        "-- 16 agents: 320 calls in {:.0} ms ({:.0} calls/s)",
        wall.as_secs_f64() * 1000.0,
        320.0 / wall.as_secs_f64()
    );
    println!("{}", Latencies(searches).line("  memory_search under load"));
    println!("{}", Latencies(writes).line("  memory_write under load"));
}

#[test]
#[ignore = "a benchmark: run it in release with --ignored --nocapture"]
fn memory_latency() {
    // 1000 is the ceiling (MAX_ENTRIES); 2000 is a file left by a hand edit or
    // an older build, which still has to read.
    for entries in [1000, 2000] {
        single_caller(entries);
        sixteen_agents(entries);
    }
}
