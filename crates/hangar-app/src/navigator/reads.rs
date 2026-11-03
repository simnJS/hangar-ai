//! What the chrome reads from other packages' read-models, in one place.
//!
//! The navigator, the toolbar, the status bar and Needs-you all say what the
//! agents are doing (`useWorkspaceActivity`, `useAgentsIn`), and every one of
//! those reads goes through here, so that the day P4's `Live` holds activity
//! as a table of its own, one function changes and not four views.
//!
//! TODO(P4): `panes_logic::Live` is an empty struct today and its stub answers
//! only `state_glyph_state`. A pane's activity is read from that glyph: a pane
//! whose shell is starting or has exited counts as idle here, where the TS
//! counted whatever activity the pane last reported. The two differ only for a
//! pane that dies mid-turn, which the glyph is more right about. When P4 lands
//! a `Live::activity(pane)`, read it here instead.

use crate::panes_logic;
use crate::state::types::Workspace;
use crate::state::{AgentActivity, GlyphState, Model, PaneId};

use super::model::reading_order;

/// A pane's activity: `working`, `waiting`, `yours` or `idle`.
pub fn activity(m: &Model, pane: &PaneId) -> AgentActivity {
    match panes_logic::state_glyph_state(m, pane) {
        GlyphState::Working => AgentActivity::Working,
        GlyphState::Waiting => AgentActivity::Waiting,
        GlyphState::Yours => AgentActivity::Yours,
        GlyphState::Starting | GlyphState::Exited | GlyphState::Idle => AgentActivity::Idle,
    }
}

/// The glyph a pane wears: its shell's life until it runs, then its agent's
/// activity (`paneGlyphState`).
pub fn glyph(m: &Model, pane: &PaneId) -> hangar_ui::glyph::GlyphState {
    use hangar_ui::glyph::GlyphState as Ui;
    match panes_logic::state_glyph_state(m, pane) {
        GlyphState::Starting => Ui::Starting,
        GlyphState::Exited => Ui::Exited,
        GlyphState::Working => Ui::Working,
        GlyphState::Waiting => Ui::Waiting,
        GlyphState::Yours => Ui::Yours,
        GlyphState::Idle => Ui::Idle,
    }
}

/// The glyph of an agent's activity alone, for a count of panes in that state.
pub fn activity_glyph(activity: AgentActivity) -> hangar_ui::glyph::GlyphState {
    hangar_ui::glyph::GlyphState::from(hangar_theme::AgentState::from(activity))
}

/// How many of a workspace's agents are working, waiting, or done
/// (`WorkspaceActivity`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub working: usize,
    pub waiting: usize,
    pub yours: usize,
}

pub fn counts(m: &Model, ws: &Workspace) -> Counts {
    let mut counts = Counts::default();
    for pane in &ws.panes {
        match activity(m, &pane.id) {
            AgentActivity::Working => counts.working += 1,
            AgentActivity::Waiting => counts.waiting += 1,
            AgentActivity::Yours => counts.yours += 1,
            AgentActivity::Idle => {}
        }
    }
    counts
}

/// The states a workspace's summary counts, the one that needs you first
/// (`BUSY`).
pub const BUSY: [AgentActivity; 3] = [
    AgentActivity::Waiting,
    AgentActivity::Yours,
    AgentActivity::Working,
];

/// The agents of these workspaces in `state` now, in the order they sit on
/// screen: workspace by workspace, each one's panes in reading order
/// (`agentsIn`). Read when asked, for a menu or a click or a frame, rather
/// than kept up to date.
pub fn agents_in<'a>(
    m: &Model,
    workspaces: &[&'a Workspace],
    state: AgentActivity,
) -> Vec<(&'a Workspace, &'a crate::state::Pane)> {
    workspaces
        .iter()
        .flat_map(|ws| reading_order(ws).into_iter().map(move |pane| (*ws, pane)))
        .filter(|(_, pane)| activity(m, &pane.id) == state)
        .collect()
}

/// Whether a pane wants the person now: waiting for an answer, or done and
/// waiting for them (`wanting.waiting` / `wanting.yours`).
pub fn wants_you(m: &Model, pane: &PaneId) -> bool {
    matches!(
        activity(m, pane),
        AgentActivity::Waiting | AgentActivity::Yours
    )
}

/// The shortcut of a command as the settings store it (`"Ctrl+Shift+P"`), for
/// a tooltip or a menu line (`useShortcutLabel`). Empty when it has none, and
/// the widgets that show it leave it out then.
///
/// TODO(P7): `KeymapIndex` is a unit struct until P7 defines it, so no
/// binding is known and none is shown. It answers here when it can: this is
/// the one function the chrome asks.
pub fn binding(ctx: &crate::ctx::Ctx<'_>, id: crate::commands::CommandId) -> String {
    let _ = (ctx.keymap, id);
    String::new()
}

/// A tab of an enabled kit, for the toolbar's Kits menu (`KitMenuEntry`).
#[derive(Debug, Clone)]
pub struct KitTab {
    pub kit_id: String,
    pub tab_id: String,
    /// As the kit's manifest names the tab.
    pub title: String,
    /// The kit's name.
    pub kit: String,
}

/// The tabs of the workspace's kits, and whether the installed kits are still
/// being read; `None` when no kit is enabled on it, and the button is not
/// there (`kitTabs`, `kitsLoading`).
///
/// TODO(P14, P16): the installed kits live in `kits::Live` and the tabs a
/// workspace may open come from `kit_tabs`; both are empty stubs, so the
/// Kits button stays out of the bar until they answer.
pub fn kit_tabs(m: &Model, ws: &Workspace) -> Option<(Vec<KitTab>, bool)> {
    let _ = (m, ws);
    None
}

/// What the toolbar's 3D button needs of the preview: some model is open,
/// the panel shows it, and something was opened or rewritten since the panel
/// was last on screen (`modelsOpen`, `modelShown`, `modelsUnseen`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Models {
    pub open: bool,
    pub shown: bool,
    pub unseen: bool,
}

/// TODO(P15): the preview store is P15's and `preview::Live` holds nothing
/// yet, so the 3D button is not drawn.
pub fn models(m: &Model, ws: &Workspace) -> Models {
    let _ = (m, ws);
    Models::default()
}

/// When a workspace fell asleep (`useAsleep`), in ms since the epoch.
///
/// TODO(P4): hibernation is P4's (`hibernateStore.ts`) and its `Live` has
/// nothing to ask yet, so no workspace is ever shown asleep. The moon and its
/// tooltip are drawn and waiting for this one function.
pub fn asleep_at(m: &Model, ws: &Workspace) -> Option<i64> {
    let _ = (m, ws);
    None
}
