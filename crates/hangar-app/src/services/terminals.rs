//! What the engines may do with a pane's terminal, wherever it is drawn
//! (inventory §8.2 and A6). P5 implements it over its `TerminalHub`; until it
//! does, every pane reads as absent.

use crate::state::PaneId;

pub trait TerminalAccess: Send + Sync {
    /// Whether the pane has a live terminal.
    fn has(&self, pane: &PaneId) -> bool {
        let _ = pane;
        false
    }

    /// The last `n` lines of the pane's screen and scrollback, with the
    /// semantics of `readLines` in `lib/windows.ts`.
    fn read_lines(&self, pane: &PaneId, n: usize) -> Option<Vec<String>> {
        let _ = (pane, n);
        None
    }

    /// Pastes `text` as the person would, bracketed when the program asked
    /// for it (DECSET 2004). Whether the pane took it.
    fn paste(&self, pane: &PaneId, text: &str) -> bool {
        let _ = (pane, text);
        false
    }

    /// Whether the program put the cursor keys in application mode (DECCKM),
    /// which changes what an arrow key sends.
    fn application_cursor(&self, pane: &PaneId) -> bool {
        let _ = pane;
        false
    }
}

/// No terminals: the default until P5 installs its hub.
#[derive(Debug, Default)]
pub struct NoTerminals;

impl TerminalAccess for NoTerminals {}
