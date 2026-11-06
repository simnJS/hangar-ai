//! What the board costs the agents, measured: the work behind each board
//! route of the API (server.rs) on boards of 10, 500 and 5000 tasks, one
//! caller at a time and then 16 at once.
//!
//! Ignored by default — it takes a while and only means something in a
//! release build:
//!
//! ```text
//! cargo test --release -p hangar-core --lib board::bench -- --ignored --nocapture
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;

/// One latency figure per call, summarized as the audit reports them.
pub(crate) struct Latencies(pub Vec<Duration>);

impl Latencies {
    fn at(&self, quantile: f64) -> Duration {
        let mut sorted = self.0.clone();
        sorted.sort();
        let index = ((sorted.len() as f64 - 1.0) * quantile).round() as usize;
        sorted[index]
    }

    pub(crate) fn line(&self, label: &str) -> String {
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        format!(
            "{label:<34} n={:<5} median {:>9.3} ms  p95 {:>9.3} ms  p99 {:>9.3} ms",
            self.0.len(),
            ms(self.at(0.5)),
            ms(self.at(0.95)),
            ms(self.at(0.99)),
        )
    }
}

pub(crate) fn timed<T>(samples: &mut Vec<Duration>, work: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let out = work();
    samples.push(start.elapsed());
    out
}

/// A task the size agents write them: a brief, a couple of comments.
fn realistic(i: usize) -> Task {
    let stamp = 1_760_000_000_000 + i as u64;
    Task {
        id: format!("bench-{i:06}"),
        title: format!("Task {i}: make the thing do what the person asked"),
        description: "Goal: what the person asked, in a paragraph. ".repeat(14),
        column: COLUMNS[i % 4].to_string(),
        priority: (i % 3) as u8,
        assignee: (i % 4 == 1).then(|| format!("worker-{}", i % 16)),
        labels: vec!["bench".into(), format!("area-{}", i % 7)],
        comments: (0..2)
            .map(|c| Comment {
                id: format!("c-{i}-{c}"),
                author: "worker".into(),
                text: "What changed, the commands run and their results. ".repeat(6),
                created_at: stamp,
            })
            .collect(),
        depends_on: vec![],
        created_at: stamp,
        updated_at: stamp,
        order: i as f64,
        doing_since: None,
        owns: vec![],
        attempts: 0,
        blocked: None,
        progress: None,
    }
}

fn seeded(tasks: usize) -> (PathBuf, usize) {
    let root = crate::testdir::scratch("board-bench");
    let board = Board {
        tasks: (0..tasks).map(realistic).collect(),
    };
    write_board(root.to_str().unwrap(), &board).unwrap();
    let bytes = fs::metadata(board_path(root.to_str().unwrap()))
        .unwrap()
        .len() as usize;
    (root, bytes)
}

fn fresh(i: usize) -> NewTask {
    NewTask {
        title: format!("Created by the bench {i}"),
        description: "A new task. ".repeat(20),
        column: None,
        priority: None,
        labels: vec![],
        depends_on: vec![],
        owns: vec![],
    }
}

const ROUNDS: usize = 60;
const AGENTS: usize = 16;
const CALLS_PER_AGENT: usize = 20;

fn single_caller(tasks: usize) {
    let (root, bytes) = seeded(tasks);
    let cwd = root.to_str().unwrap().to_string();
    let store = BoardStore::default();
    println!(
        "\n== board of {tasks} tasks ({} KiB on disk) ==",
        bytes / 1024
    );
    // Warm: the cold `shared_root` starts git once per folder.
    store.read(&cwd);

    let mut reads = Vec::new();
    let mut creates = Vec::new();
    let mut patches = Vec::new();
    let mut claims = Vec::new();
    for round in 0..ROUNDS {
        timed(&mut reads, || store.read(&cwd));
        timed(&mut creates, || {
            store
                .update(&cwd, |board| create_task(board, fresh(round)))
                .unwrap()
        });
        let id = format!("bench-{:06}", (round * 7919) % tasks);
        timed(&mut patches, || {
            store
                .update(&cwd, |board| {
                    patch_task(
                        board,
                        &id,
                        TaskPatch {
                            title: Some(format!("Renamed {round}")),
                            ..TaskPatch::default()
                        },
                    )
                })
                .unwrap()
        });
        let todo = store.update(&cwd, |board| create_task(board, fresh(round)));
        let todo = todo.unwrap().id;
        timed(&mut claims, || {
            store
                .update(&cwd, |board| claim_task(board, &todo, "bench"))
                .unwrap()
        });
    }
    println!("{}", Latencies(reads).line("board_list_tasks (read)"));
    println!("{}", Latencies(creates).line("board_create_task"));
    println!("{}", Latencies(patches).line("board_update_task"));
    println!("{}", Latencies(claims).line("board_claim_task"));
}

/// 16 agents on the one board, each listing, updating and creating in turn.
fn sixteen_agents(tasks: usize) {
    let (root, _) = seeded(tasks);
    let cwd = Arc::new(root.to_str().unwrap().to_string());
    let store: Arc<BoardStore> = Arc::default();
    store.read(&cwd);
    let started = Instant::now();
    let handles: Vec<_> = (0..AGENTS)
        .map(|agent| {
            let (cwd, store) = (cwd.clone(), store.clone());
            std::thread::spawn(move || {
                let (mut reads, mut writes) = (Vec::new(), Vec::new());
                for call in 0..CALLS_PER_AGENT {
                    match call % 4 {
                        0 | 2 => {
                            timed(&mut reads, || store.read(&cwd));
                        }
                        1 => {
                            let id = format!("bench-{:06}", (agent * 31 + call) % tasks);
                            timed(&mut writes, || {
                                store.update(&cwd, |board| {
                                    patch_task(
                                        board,
                                        &id,
                                        TaskPatch {
                                            priority: Some((call % 3) as u8),
                                            ..TaskPatch::default()
                                        },
                                    )
                                })
                            })
                            .unwrap();
                        }
                        _ => {
                            timed(&mut writes, || {
                                store.update(&cwd, |board| create_task(board, fresh(call)))
                            })
                            .unwrap();
                        }
                    }
                }
                (reads, writes)
            })
        })
        .collect();
    let (mut reads, mut writes) = (Vec::new(), Vec::new());
    for handle in handles {
        let (r, w) = handle.join().unwrap();
        reads.extend(r);
        writes.extend(w);
    }
    let wall = started.elapsed();
    println!(
        "-- 16 agents on {tasks} tasks: {} calls in {:.0} ms ({:.0} calls/s)",
        AGENTS * CALLS_PER_AGENT,
        wall.as_secs_f64() * 1000.0,
        (AGENTS * CALLS_PER_AGENT) as f64 / wall.as_secs_f64()
    );
    println!("{}", Latencies(reads).line("  list under load"));
    println!("{}", Latencies(writes).line("  create/update under load"));
}

#[test]
#[ignore = "a benchmark: run it in release with --ignored --nocapture"]
fn board_latency() {
    for tasks in [10, 500, 5000] {
        single_caller(tasks);
        sixteen_agents(tasks);
    }
}
