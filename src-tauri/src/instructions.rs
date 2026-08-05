//! Writes the agent playbook into the files agents actually read on startup.
//!
//! A doc parked in `.iabench/` would be loaded by nobody. `AGENTS.md` is the
//! cross-tool convention (Codex, Cursor, recent Claude Code) and `CLAUDE.md`
//! is always read by Claude Code, so the block goes in both.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

const START: &str = "<!-- IABENCH:START - generated, do not edit by hand -->";
const END: &str = "<!-- IABENCH:END -->";

const TARGET_FILES: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];

#[derive(Debug, Serialize)]
pub struct InstructionReport {
    pub path: String,
    pub ok: bool,
    pub message: String,
}

fn playbook() -> String {
    format!(
        r#"{START}

## Shared task board (Hangar.IA)

This project is driven from Hangar.IA. Several agents work **in parallel** in separate
terminals on this same directory. A shared kanban board is the source of truth for
who is doing what. Reach it through the MCP tools prefixed `board_`.

### Hard rule

**Never start work on a task without claiming it first.** Another agent may already
be on it. `board_claim_task` fails when the task is taken — when that happens, do not
retry it, pick a different one.

### Expected loop

1. **On startup**, call `board_list_tasks` to see the current state of the work.
2. **To pick work**, call `board_next_task`. It returns the highest-priority free task
   whose dependencies are all done. Do not pick arbitrarily from the todo column, or
   you may start work that is still blocked.
3. **Claim it** with `board_claim_task`. It moves to `doing` automatically.
   - If the call fails, go back to step 2.
4. **Do the work.**
5. **Report** with `board_comment_task`: decisions made, files touched, problems hit.
   This is the only channel other agents have to learn what you did.
6. **Finish** with `board_update_task`, setting `column` to `review` if the work needs
   a second pair of eyes, or `done` otherwise.
   - If you give up, call `board_update_task` with `release: true` so the task returns
     to the pool instead of sitting blocked under your name.

### Splitting work

When you find work beyond your current task, do not silently pile it into the same
change: create tasks with `board_create_task` so other agents can pick them up.

- Use `depends_on` with the ids of prerequisite tasks. `board_next_task` relies on it
  to never hand out work that cannot start yet.
- Use `priority` (integer, higher is more urgent) for work that unblocks others.

### Columns

| Column   | Meaning |
|----------|---------|
| `todo`   | ready to be picked up |
| `doing`  | someone is working on it |
| `review` | finished, waiting for a review |
| `done`   | accepted - unblocks tasks that depend on it |

### Staying out of each other's way

Before editing a central file, check with `board_list_tasks` that no other agent holds
a `doing` task covering the same area. If one does, leave a comment on their task
rather than editing the same files at the same time.

{END}"#
    )
}

/// Replaces our block if present, otherwise appends it, leaving the rest of the
/// file untouched.
fn upsert_block(existing: &str, block: &str) -> String {
    if let (Some(start), Some(end)) = (existing.find(START), existing.find(END)) {
        if start < end {
            let mut out = String::with_capacity(existing.len());
            out.push_str(&existing[..start]);
            out.push_str(block);
            out.push_str(&existing[end + END.len()..]);
            return out;
        }
    }

    if existing.trim().is_empty() {
        return format!("{block}\n");
    }
    format!("{}\n\n{}\n", existing.trim_end(), block)
}

fn write_playbook(root: &Path, file: &str) -> InstructionReport {
    let path = root.join(file);
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let updated = upsert_block(&existing, &playbook());

    match fs::write(&path, updated) {
        Ok(()) => InstructionReport {
            path: path.to_string_lossy().into_owned(),
            ok: true,
            message: if existing.is_empty() {
                "créé".into()
            } else {
                "bloc mis à jour".into()
            },
        },
        Err(err) => InstructionReport {
            path: path.to_string_lossy().into_owned(),
            ok: false,
            message: err.to_string(),
        },
    }
}

/// Appends the ignore rule only when no existing line already covers it, so
/// running this repeatedly does not pile up duplicates.
fn update_gitignore(root: &Path) -> InstructionReport {
    let path = root.join(".gitignore");
    let existing = fs::read_to_string(&path).unwrap_or_default();

    let already = existing.lines().any(|line| {
        let trimmed = line.trim().trim_end_matches('/');
        trimmed == ".iabench" || trimmed == "/.iabench"
    });

    if already {
        return InstructionReport {
            path: path.to_string_lossy().into_owned(),
            ok: true,
            message: "déjà présent".into(),
        };
    }

    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    if !updated.is_empty() {
        updated.push('\n');
    }
    updated.push_str("# Hangar.IA — tableau de tâches local\n.iabench/\n");

    match fs::write(&path, updated) {
        Ok(()) => InstructionReport {
            path: path.to_string_lossy().into_owned(),
            ok: true,
            message: "règle ajoutée".into(),
        },
        Err(err) => InstructionReport {
            path: path.to_string_lossy().into_owned(),
            ok: false,
            message: err.to_string(),
        },
    }
}

#[tauri::command]
pub fn write_agent_instructions(workspace_cwd: String) -> Vec<InstructionReport> {
    let root = PathBuf::from(&workspace_cwd);
    if !root.is_dir() {
        return vec![InstructionReport {
            path: workspace_cwd,
            ok: false,
            message: "dossier introuvable".into(),
        }];
    }

    let mut reports: Vec<InstructionReport> = TARGET_FILES
        .iter()
        .map(|file| write_playbook(&root, file))
        .collect();

    reports.push(update_gitignore(&root));
    reports
}

/// Lets the UI show whether the playbook is already in place.
#[tauri::command]
pub fn agent_instructions_status(workspace_cwd: String) -> bool {
    let root = PathBuf::from(workspace_cwd);
    TARGET_FILES.iter().any(|file| {
        fs::read_to_string(root.join(file))
            .map(|content| content.contains(START))
            .unwrap_or(false)
    })
}
