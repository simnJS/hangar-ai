//! What a pane's shell has started: the processes below it, found in the
//! system's process table.
//!
//! Two callers. Before a sleeping workspace's terminals are closed, its shells
//! are asked whether they still run something — a dev server, a watcher, a
//! build — that closing them would kill (`pty_has_children`). And a pane
//! closed for that reason takes everything below its shell with it, the agent
//! and whatever the agent started, so the memory it was put to sleep for is
//! really given back (`pty_kill` with `tree`).
//!
//! The table is read the platform's way; walking it is the same everywhere,
//! and is what the tests below exercise.

use std::collections::{HashMap, HashSet, VecDeque};

/// One row of the process table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
}

/// Every process below `root`, parents before their children.
///
/// `started` gives a process's creation time when the platform can tell it.
/// Windows never re-parents an orphan: it keeps the id of its dead parent,
/// and that id is handed out again to some later process — perhaps a pane's
/// shell, which would then seem to own a stranger. A process cannot be the
/// child of one created after it, so such a row is passed over. A time that
/// cannot be read rules nothing out: for the question it serves, "maybe" is
/// the answer that keeps a workspace awake.
pub fn descendants(table: &[Proc], root: u32, started: impl Fn(u32) -> Option<u64>) -> Vec<u32> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for proc in table {
        // The idle process is its own parent on Windows; nothing is its child.
        if proc.pid != proc.ppid {
            children.entry(proc.ppid).or_default().push(proc.pid);
        }
    }

    let mut found = Vec::new();
    let mut seen = HashSet::from([root]);
    let mut queue = VecDeque::from([(root, started(root))]);
    while let Some((parent, parent_started)) = queue.pop_front() {
        for &pid in children.get(&parent).map_or(&[][..], Vec::as_slice) {
            if seen.contains(&pid) {
                continue;
            }
            let born = started(pid);
            if let (Some(born), Some(parent_started)) = (born, parent_started) {
                if born < parent_started {
                    continue;
                }
            }
            seen.insert(pid);
            found.push(pid);
            queue.push_back((pid, born));
        }
    }
    found
}

/// The rows of `ps -A -o pid= -o ppid=`, which Linux and macOS both print.
#[cfg(any(unix, test))]
fn parse_ps(output: &str) -> Vec<Proc> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let ppid = fields.next()?.parse().ok()?;
            Some(Proc { pid, ppid })
        })
        .collect()
}

/// The process table, or `None` when it cannot be read — which callers must
/// not take for an empty one.
#[cfg(unix)]
pub fn table() -> Option<Vec<Proc>> {
    let out = std::process::Command::new("ps")
        .args(["-A", "-o", "pid=", "-o", "ppid="])
        .output()
        .ok()?;
    let rows = parse_ps(&String::from_utf8_lossy(&out.stdout));
    (out.status.success() && !rows.is_empty()).then_some(rows)
}

/// Unix re-parents orphans, so a parent id is never stale there.
#[cfg(unix)]
pub fn started(_pid: u32) -> Option<u64> {
    None
}

/// Ends each process outright, as `reap_process_group` ends the pane's group.
#[cfg(unix)]
pub fn terminate(pids: &[u32]) {
    for &pid in pids {
        if let Ok(pid) = libc::pid_t::try_from(pid) {
            if pid > 1 {
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
            }
        }
    }
}

#[cfg(windows)]
mod win {
    use super::Proc;
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_TERMINATE,
    };

    /// A handle closed on the way out, whatever path leaves.
    struct Owned(HANDLE);

    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    fn open(pid: u32, access: u32) -> Option<Owned> {
        let handle = unsafe { OpenProcess(access, 0, pid) };
        (!handle.is_null()).then_some(Owned(handle))
    }

    pub fn table() -> Option<Vec<Proc>> {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let snapshot = Owned(snapshot);
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut rows = Vec::new();
        let mut more = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
        while more {
            rows.push(Proc {
                pid: entry.th32ProcessID,
                ppid: entry.th32ParentProcessID,
            });
            more = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
        }
        (!rows.is_empty()).then_some(rows)
    }

    /// Creation time in 100 ns ticks, for telling a real child from an
    /// orphan whose dead parent's id was reused.
    pub fn started(pid: u32) -> Option<u64> {
        let process = open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        let ok = unsafe {
            GetProcessTimes(process.0, &mut created, &mut exited, &mut kernel, &mut user)
        };
        (ok != 0)
            .then(|| (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }

    pub fn terminate(pids: &[u32]) {
        for &pid in pids {
            if let Some(process) = open(pid, PROCESS_TERMINATE) {
                unsafe {
                    TerminateProcess(process.0, 1);
                }
            }
        }
    }
}

#[cfg(windows)]
pub use win::{started, table, terminate};

/// Whether the process `root` has started anything that is still running.
pub fn has_descendants(table: &[Proc], root: u32) -> bool {
    !descendants(table, root, started).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &[(u32, u32)]) -> Vec<Proc> {
        rows.iter().map(|&(pid, ppid)| Proc { pid, ppid }).collect()
    }

    const UNKNOWN: fn(u32) -> Option<u64> = |_| None;

    #[test]
    fn finds_the_whole_tree_below_a_shell_parents_first() {
        // 10: the shell, running the agent 11 (through a cmd wrapper 12) and
        // the agent's MCP server 13. 20 is another pane, 1 the system.
        let rows = table(&[
            (1, 0),
            (10, 1),
            (12, 10),
            (11, 12),
            (13, 11),
            (20, 1),
            (21, 20),
        ]);
        assert_eq!(descendants(&rows, 10, UNKNOWN), vec![12, 11, 13]);
        assert_eq!(descendants(&rows, 20, UNKNOWN), vec![21]);
        assert!(descendants(&rows, 21, UNKNOWN).is_empty());
    }

    #[test]
    fn a_shell_at_rest_has_nothing_below_it() {
        let rows = table(&[(1, 0), (10, 1), (20, 1), (21, 20)]);
        assert!(descendants(&rows, 10, UNKNOWN).is_empty());
        // Not in the table at all, a shell that already ended: nothing either.
        assert!(descendants(&rows, 99, UNKNOWN).is_empty());
    }

    #[test]
    fn passes_over_an_orphan_older_than_the_process_that_reused_its_parent_id() {
        // 30 was started by a process 10 that died; the shell now holding id
        // 10 started later, so 30 is not its child. 31 really is.
        let rows = table(&[(10, 1), (30, 10), (31, 10), (32, 30)]);
        let born = |pid: u32| match pid {
            10 => Some(500),
            30 => Some(100),
            31 => Some(600),
            32 => Some(700),
            _ => None,
        };
        assert_eq!(descendants(&rows, 10, born), vec![31]);
    }

    #[test]
    fn an_unreadable_start_time_rules_nothing_out() {
        let rows = table(&[(10, 1), (11, 10), (12, 11)]);
        let born = |pid: u32| (pid == 10).then_some(500);
        assert_eq!(descendants(&rows, 10, born), vec![11, 12]);
    }

    #[test]
    fn survives_cycles_and_self_parents() {
        // The idle process (0, parent 0), and ids that were reused into a loop.
        let rows = table(&[(0, 0), (10, 1), (11, 10), (12, 11), (10, 12)]);
        assert_eq!(descendants(&rows, 10, UNKNOWN), vec![11, 12]);
        assert!(descendants(&rows, 0, UNKNOWN).is_empty());
    }

    #[test]
    fn reads_ps_output_and_skips_what_is_not_a_row() {
        let rows = parse_ps("    1     0\n  412     1\n\n garbage\n 9000  412\n");
        assert_eq!(rows, table(&[(1, 0), (412, 1), (9000, 412)]));
    }
}
