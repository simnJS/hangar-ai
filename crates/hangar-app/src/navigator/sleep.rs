//! The « Put to sleep » line of a workspace's menu (`sleepEntry` of
//! `Sidebar.tsx`): greyed, with the reason as its tooltip, when a rule says
//! the workspace stays awake.
//!
//! The rules are the pane logic's (`panes_logic::sleep_line`). One of them
//! asks the system what the workspace's shells run, which takes a moment: the
//! menu asks as it opens, the line stays live meanwhile, and it is read again
//! once the answer is in. The click asks everything again whatever the line
//! said (`Intent::Sleep`).

use std::collections::HashMap;

use hangar_i18n::Translator;

use crate::panes_logic::{Awake, Blocker, SleepLine, sleep_line, sleep_refusal};
use crate::state::{Model, PaneId, WsId};

/// What the system said of the shells of the workspace whose menu is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    ws: WsId,
    answer: Answer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Answer {
    /// For each shell asked, whether it runs something.
    Running(HashMap<PaneId, bool>),
    /// The system could not say.
    Failed,
}

impl Probe {
    /// The answer to a question about `asked`: the shells that run something
    /// are the ones the system names.
    pub fn answered(ws: WsId, asked: &[PaneId], answer: Result<Vec<PaneId>, String>) -> Probe {
        let answer = match answer {
            Ok(running) => Answer::Running(
                asked
                    .iter()
                    .map(|id| (id.clone(), running.contains(id)))
                    .collect(),
            ),
            Err(_) => Answer::Failed,
        };
        Probe { ws, answer }
    }

    pub fn ws(&self) -> &str {
        &self.ws
    }
}

/// The shells to ask the system about as the menu of `ws` opens, when the
/// rules leave only that to settle.
pub fn request(m: &Model, ws: &str) -> Option<Vec<PaneId>> {
    match sleep_line(m, ws, None)? {
        SleepLine::Ask(shells) => Some(shells),
        SleepLine::Ready | SleepLine::Refused(_) => None,
    }
}

/// What the line is now, with what the system said of the shells so far, if it
/// has. `None` when the workspace is gone, and the menu has no such line.
pub fn line(m: &Model, probe: Option<&Probe>, ws: &str) -> Option<SleepLine> {
    let line = sleep_line(m, ws, None)?;
    let SleepLine::Ask(_) = line else {
        return Some(line);
    };
    let Some(probe) = probe.filter(|probe| probe.ws == ws) else {
        // Not asked yet, or not answered: live until it is.
        return Some(line);
    };
    match &probe.answer {
        Answer::Running(known) => match sleep_line(m, ws, Some(known))? {
            // Shells that came while the others were being asked: nobody asked
            // them.
            SleepLine::Ask(_) => Some(unknown_processes(m)),
            answered => Some(answered),
        },
        Answer::Failed => Some(unknown_processes(m)),
    }
}

/// Not knowing what a shell runs is reason enough to leave it running.
fn unknown_processes(m: &Model) -> SleepLine {
    let awake = Awake {
        reason: Blocker::UnknownProcesses,
        pane_id: None,
    };
    SleepLine::Refused(sleep_refusal(
        Translator::new(m.locale()),
        &awake,
        &[],
        &|_| false,
    ))
}

#[cfg(test)]
mod tests;
