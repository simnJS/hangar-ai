//! A pane's processes in a systemd user scope of their own (the `pane-scopes`
//! experiment, Linux only).
//!
//! Started from a desktop, Hangar lives in a scope the session made for it
//! (`app-gnome-hangar\x2dai-<pid>.scope`), and everything its panes run is
//! born there too: agents, their builds, their tests. When memory runs out,
//! systemd-oomd picks a scope to kill, and that one holds the lot, so Hangar
//! and every agent went down for one pane's build. Moved to a scope of its
//! own, a pane is what gets picked when it is the one eating memory.
//!
//! The shell is spawned as before (same PTY, environment, folder, process
//! group), then its PID is handed to the user manager in a transient scope
//! over D-Bus (`StartTransientUnit`): no wrapper around the command, so what
//! the pane runs, and how `kill_group`/`kill_all` reach it, does not change.
//! What the shell forks afterwards is born in the new scope.
//!
//! The unit is named after the pane (`hangar-pane-<name>-<id>-<n>.scope`)
//! with its name in the description, so `systemctl --user status` says which
//! pane is which. No limit is set on it: it only gives oomd a smaller target.
//! A scope that cannot be made leaves the pane where it was, as without the
//! experiment, and says so once on stderr.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The slice the user manager keeps applications in, Hangar's own scope
/// included. Only the D-Bus request reads it, and that exists on Linux alone.
#[cfg(target_os = "linux")]
pub(crate) const SLICE: &str = "app.slice";

/// What a unit name may hold besides ASCII letters and digits
/// (systemd.unit(5)); `\` escapes are left out, a pane name is not a path.
fn unit_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':')
}

/// At most this many characters of the pane's name go into the unit's name,
/// which systemd caps at 255 characters in all.
const NAME_PART: usize = 32;

/// The scope's unit name for the pane `id` named `name`, its `n`-th: the
/// name lowercased with anything else than letters, digits, `_`, `.` and `:`
/// turned into single dashes, then the start of the id, so two panes of one
/// name stay apart. `n` keeps a restarted pane from asking for the name of a
/// scope systemd has not collected yet.
pub(crate) fn unit_name(id: &str, name: Option<&str>, n: u64) -> String {
    let mut slug = String::new();
    for c in name.unwrap_or("").chars().flat_map(char::to_lowercase) {
        if slug.chars().count() >= NAME_PART {
            break;
        }
        if unit_char(c) {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    let short: String = id.chars().filter(|c| unit_char(*c)).take(8).collect();
    let short = if short.is_empty() { "pane" } else { &short };
    if slug.is_empty() {
        format!("hangar-pane-{short}-{n}.scope")
    } else {
        format!("hangar-pane-{slug}-{short}-{n}.scope")
    }
}

/// The scope's description, what `systemctl --user status` shows beside it.
pub(crate) fn description(id: &str, name: Option<&str>) -> String {
    match name.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => format!("Hangar pane \"{name}\" ({id})"),
        None => format!("Hangar pane {id}"),
    }
}

/// Where a pane's shell ends up.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Placed {
    /// The experiment is off, or this is not Linux: nothing was asked.
    Untouched,
    /// In the scope of that name.
    Scoped(String),
    /// Left in Hangar's scope, as without the experiment: no user manager
    /// answered, or it refused the scope.
    Fallback(String),
}

/// Asks for the scope of the pane `id` holding `pid`, when `on`, through
/// `start` (the user manager, or a test's stand-in). The unit's number is
/// drawn only when the scope is asked for. A failure is reported once per
/// run of Hangar through `warn`, then the pane goes on where it is.
pub(crate) fn place(
    on: bool,
    pid: Option<u32>,
    id: &str,
    name: Option<&str>,
    start: impl FnOnce(&str, &str, u32) -> Result<(), String>,
    warn: impl FnOnce(&str),
) -> Placed {
    static COUNT: AtomicU64 = AtomicU64::new(0);
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !on || !cfg!(target_os = "linux") {
        return Placed::Untouched;
    }
    let outcome = match pid {
        None => Err("the shell has no process id".to_string()),
        Some(pid) => {
            let unit = unit_name(id, name, COUNT.fetch_add(1, Ordering::Relaxed));
            start(&unit, &description(id, name), pid).map(|()| unit)
        }
    };
    match outcome {
        Ok(unit) => Placed::Scoped(unit),
        Err(err) => {
            if !WARNED.swap(true, Ordering::Relaxed) {
                warn(&format!(
                    "pane scopes: the panes stay in Hangar's own scope ({err})"
                ));
            }
            Placed::Fallback(err)
        }
    }
}

/// The user manager's answer to `StartTransientUnit`: a scope named `unit`
/// in [`SLICE`], holding `pid`, removed once empty or failed.
///
/// The session bus is connected once, on the first pane asking: a machine
/// without one (no systemd, a session without a user manager) is told so
/// once and asks no more.
#[cfg(target_os = "linux")]
pub(crate) fn start_transient(unit: &str, description: &str, pid: u32) -> Result<(), String> {
    use std::sync::OnceLock;
    use zbus::zvariant::Value;

    static BUS: OnceLock<Result<zbus::blocking::Connection, String>> = OnceLock::new();
    let bus = BUS
        .get_or_init(|| zbus::blocking::Connection::session().map_err(|err| err.to_string()))
        .as_ref()
        .map_err(|err| format!("no session bus: {err}"))?;
    let properties: Vec<(&str, Value<'_>)> = vec![
        ("Description", Value::from(description)),
        ("Slice", Value::from(SLICE)),
        ("CollectMode", Value::from("inactive-or-failed")),
        ("PIDs", Value::from(vec![pid])),
    ];
    let aux: Vec<(&str, Vec<(&str, Value<'_>)>)> = Vec::new();
    bus.call_method(
        Some("org.freedesktop.systemd1"),
        "/org/freedesktop/systemd1",
        Some("org.freedesktop.systemd1.Manager"),
        "StartTransientUnit",
        &(unit, "fail", properties, aux),
    )
    .map(|_| ())
    .map_err(|err| format!("{unit}: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn the_unit_is_named_after_the_pane_and_its_id() {
        assert_eq!(
            unit_name("3f2a9c1e-77aa-4bb0", Some("Sam"), 0),
            "hangar-pane-sam-3f2a9c1e-0.scope"
        );
        assert_eq!(
            unit_name("abc", Some("  Leo / build (rust) "), 7),
            "hangar-pane-leo-build-rust-abc-7.scope"
        );
    }

    #[test]
    fn a_unit_name_holds_only_what_systemd_accepts() {
        let unit = unit_name("é/ü", Some("Été 🚀 x\\y"), 3);
        assert_eq!(unit, "hangar-pane-t-x-y-pane-3.scope");
        let long = "a".repeat(300);
        let unit = unit_name(&long, Some(&long), 1);
        assert!(unit.len() < 100, "{unit}");
        let body = unit.trim_end_matches(".scope");
        assert!(body
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':')));
    }

    #[test]
    fn a_pane_without_a_name_is_named_by_its_id() {
        assert_eq!(unit_name("p1", None, 2), "hangar-pane-p1-2.scope");
        assert_eq!(unit_name("p1", Some(" -- "), 2), "hangar-pane-p1-2.scope");
        assert_eq!(description("p1", Some(" ")), "Hangar pane p1");
        assert_eq!(description("p1", Some("Leo")), "Hangar pane \"Leo\" (p1)");
    }

    #[test]
    fn off_asks_nothing_of_systemd() {
        let asked = Cell::new(false);
        let placed = place(
            false,
            Some(42),
            "p",
            Some("x"),
            |_, _, _| {
                asked.set(true);
                Ok(())
            },
            |_| panic!("nothing to warn about"),
        );
        assert_eq!(placed, Placed::Untouched);
        assert!(!asked.get());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn on_hands_the_shell_s_pid_to_the_named_scope() {
        let placed = place(
            true,
            Some(42),
            "p9",
            Some("Sam"),
            |unit, description, pid| {
                assert!(unit.starts_with("hangar-pane-sam-p9-"), "{unit}");
                assert!(unit.ends_with(".scope"));
                assert_eq!(description, "Hangar pane \"Sam\" (p9)");
                assert_eq!(pid, 42);
                Ok(())
            },
            |_| panic!("nothing to warn about"),
        );
        assert!(matches!(placed, Placed::Scoped(unit) if unit.starts_with("hangar-pane-sam-p9-")));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_scope_that_cannot_be_made_falls_back_and_warns_once() {
        let warned = Cell::new(0);
        for _ in 0..3 {
            let placed = place(
                true,
                Some(42),
                "p",
                None,
                |_, _, _| Err("no session bus".into()),
                |message| {
                    assert!(message.contains("no session bus"));
                    warned.set(warned.get() + 1);
                },
            );
            assert_eq!(placed, Placed::Fallback("no session bus".into()));
        }
        // Once per run: another test of this binary may have warned first.
        assert!(warned.get() <= 1);
        let placed = place(
            true,
            None,
            "p",
            None,
            |_, _, _| panic!("no pid, nothing to ask"),
            |_| {},
        );
        assert!(matches!(placed, Placed::Fallback(_)));
    }
}
