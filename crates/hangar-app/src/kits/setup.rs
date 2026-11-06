//! Setup commands of a kit (docs/kits.md): shown verbatim, run in a visible
//! pane only when the person clicks Run, once per machine. `lib/kitSetup.ts`.
//!
//! WHEN A STEP COUNTS AS DONE. Hangar types the command into an interactive
//! shell pane (the way a saved command is run), so it never sees the exit
//! code: the shell outlives the command. A step is therefore marked done
//! (`kit_setup_mark`) only after the person confirms it worked, in the pane
//! they are watching, never on its own and never merely because Run was
//! clicked. The record is keyed by the command's hash, so an update that
//! changes a command makes `kit_setup_status` report the step as not done
//! again.

use std::future::Future;
use std::time::Duration;

#[cfg(test)]
use hangar_core::kits::StepStatus;

/// The steps still to run on this machine: never run, or run with another
/// command.
#[cfg(test)]
pub fn pending_steps(steps: &[StepStatus]) -> Vec<&StepStatus> {
    steps.iter().filter(|step| !step.done).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(test)]
pub struct SetupSummary {
    pub done: usize,
    pub total: usize,
    /// Every step is done (a kit with none counts as complete).
    pub complete: bool,
}

#[cfg(test)]
pub fn setup_summary(steps: &[StepStatus]) -> SetupSummary {
    let done = steps.iter().filter(|step| step.done).count();
    SetupSummary {
        done,
        total: steps.len(),
        complete: done == steps.len(),
    }
}

/// The bytes typed into the pane: the command exactly as the manifest writes
/// it and the Enter that runs it. Nothing is added, quoted or rewritten, so
/// what the person read is what the shell gets.
pub fn setup_keystrokes(command: &str) -> String {
    format!("{command}\r")
}

/// The operating system the commands are chosen for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KitOs {
    Windows,
    Macos,
    Linux,
}

impl KitOs {
    /// The name the manifest and core use for it.
    #[cfg(test)]
    pub fn name(self) -> &'static str {
        match self {
            KitOs::Windows => "windows",
            KitOs::Macos => "macos",
            KitOs::Linux => "linux",
        }
    }
}

/// Every unix that is not macOS counts as linux, as the Rust side does
/// (`manifest::current_os`).
pub fn current_os() -> KitOs {
    if cfg!(windows) {
        KitOs::Windows
    } else if cfg!(target_os = "macos") {
        KitOs::Macos
    } else {
        KitOs::Linux
    }
}

/// The same mapping from a platform string, which is how the web app learned
/// the system (`navigator.platform`). The app asks [`current_os`]; this one is
/// kept so the mapping has its test.
#[cfg(test)]
pub fn os_of_platform(platform: &str) -> KitOs {
    let platform = platform.to_lowercase();
    if platform.contains("win") {
        KitOs::Windows
    } else if platform.contains("mac") {
        KitOs::Macos
    } else {
        KitOs::Linux
    }
}

/// What typing into a pane needs: whether its terminal is up, a write, and a
/// pause.
pub trait PaneTyping: Send + Sync {
    /// Whether the pane's terminal has started.
    fn alive(&self, pane: &str) -> impl Future<Output = bool> + Send;
    /// Types into the pane.
    fn write(&self, pane: &str, data: &str) -> impl Future<Output = Result<(), String>> + Send;
    fn wait(&self, delay: Duration) -> impl Future<Output = ()> + Send;
    /// Closes the pane just opened, so a failed start leaves none behind.
    fn close_pane(&self, pane: &str);
    /// How many times to look for the pane's terminal, and the pause between
    /// looks.
    fn tries(&self) -> usize {
        80
    }
    fn delay(&self) -> Duration {
        Duration::from_millis(250)
    }
}

/// [`PaneTyping`] and the opening of the pane itself.
#[cfg(test)]
pub trait SetupPaneDeps: PaneTyping {
    /// Opens a shell pane in the kit's folder; `None` when the workspace is
    /// full.
    fn open_pane(&self) -> Option<String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupRunResult {
    Ok {
        pane: String,
    },
    NoPane,
    /// The terminal never started: nothing was typed.
    NotStarted,
    WriteFailed {
        detail: String,
    },
}

/// Waits for the pane's terminal, then types the command. Writes nothing if
/// the terminal never starts (a command typed into nowhere would look as if it
/// ran) and closes the pane it was given.
///
/// A workspace never opened this session has to mount before its pane can
/// start, which is why this waits for as long as it does.
pub async fn type_into_pane<D: PaneTyping>(pane: &str, command: &str, deps: &D) -> SetupRunResult {
    for _ in 0..deps.tries() {
        if deps.alive(pane).await {
            return match deps.write(pane, &setup_keystrokes(command)).await {
                Ok(()) => SetupRunResult::Ok {
                    pane: pane.to_owned(),
                },
                Err(detail) => {
                    deps.close_pane(pane);
                    SetupRunResult::WriteFailed { detail }
                }
            };
        }
        deps.wait(deps.delay()).await;
    }
    deps.close_pane(pane);
    SetupRunResult::NotStarted
}

/// Opens the pane, waits for its terminal, then types the command.
#[cfg(test)]
pub async fn run_setup_command<D: SetupPaneDeps>(command: &str, deps: &D) -> SetupRunResult {
    match deps.open_pane() {
        Some(pane) => type_into_pane(&pane, command, deps).await,
        None => SetupRunResult::NoPane,
    }
}

/// Makes the workspace the one on screen before a pane is added to it: the
/// shell only mounts the workspaces it has opened, and an unmounted pane never
/// starts its terminal. Selecting it opens it, the way the navigator wakes a
/// sleeping one. A sleeping workspace that is already the selected one is
/// selected again, from nothing, because selecting the same id changes
/// nothing.
///
/// The answer is the selections to make, in order: `None` selects no
/// workspace. Between the two of a wake the caller waits [`WAKE_PAUSE`].
pub fn show_workspace_selections(
    workspace: &str,
    active: Option<&str>,
    asleep: bool,
) -> Vec<Option<String>> {
    let mut selections = Vec::new();
    if asleep && active == Some(workspace) {
        selections.push(None);
    }
    selections.push(Some(workspace.to_owned()));
    selections
}

/// The pause between deselecting a sleeping workspace and selecting it again.
pub const WAKE_PAUSE: Duration = Duration::from_millis(50);

#[cfg(test)]
mod tests;
