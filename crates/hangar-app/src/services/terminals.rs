//! What the engines may do with a pane's terminal, wherever it is drawn
//! (inventory §8.2 and A6). The panes implement it over their `TerminalHub`
//! (`panes::terminal_access`, installed when the app boots); the trait's own
//! answers (a pane absent, nothing taken) are only what an implementation
//! leaves out.

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

    /// The rows of the pane's screen as the program drew them, one string per
    /// row of the grid and none of the history above it. It is for finding a
    /// question the program is asking now: one answered and scrolled off is in
    /// `read_lines` and not here, which is what the old terminal read
    /// (`buffer.viewportY` for as many rows as the terminal had). A pane
    /// scrolled back is read where the program draws all the same, since the
    /// question is not where the person is looking.
    fn screen_text(&self, pane: &PaneId) -> Option<Vec<String>> {
        let _ = pane;
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
