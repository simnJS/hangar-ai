//! What the chrome reads from other packages' read-models, in one place.
//!
//! The navigator, the toolbar, the status bar and Needs-you all say what the
//! agents are doing (`useWorkspaceActivity`, `useAgentsIn`), and every one of
//! those reads goes through here, so that a change of provider is one
//! function and not four views.
//!
//! Activity, the sleeping workspaces and the plan's limits are `panes_logic`'s
//! (`Model.live.panes_logic`), the shortcuts are the keymap's, the kits are
//! `kits::Live`'s. Nothing here keeps a copy.

use crate::commands::CommandId;
use crate::ctx::Ctx;
use crate::state::layout;
use crate::state::live::{PlanLimits, WorkspaceActivity};
use crate::state::types::Workspace;
use crate::state::{AgentActivity, Model, PaneId};

/// A pane's activity: `working`, `waiting`, `yours` or `idle`
/// (`getPaneActivity(id)?.activity ?? "idle"`).
///
/// Read from the pane logic's published activity, as the TS did, not from the
/// pane's glyph: a pane that reports none (a plain shell, an agent that never
/// started) is idle, whatever its shell is doing.
pub fn activity(m: &Model, pane: &PaneId) -> AgentActivity {
    m.live.panes_logic.activity_or_idle(pane)
}

/// The glyph of an agent's activity, for a state word or a count of panes in
/// that state.
pub fn activity_glyph(activity: AgentActivity) -> hangar_ui::glyph::GlyphState {
    hangar_ui::glyph::GlyphState::from(hangar_theme::AgentState::from(activity))
}

/// How many of a workspace's agents are working, waiting, or done
/// (`WorkspaceActivity`).
pub type Counts = WorkspaceActivity;

pub fn counts(m: &Model, ws: &Workspace) -> Counts {
    m.live.panes_logic.workspace_activity(&ws.id)
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
        .flat_map(|ws| {
            layout::reading_order(&ws.panes, ws.tree.as_ref())
                .into_iter()
                .map(move |pane| (*ws, pane))
        })
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

/// The plan's rate-limit windows, from whichever session reported last
/// (`usePlanLimits`).
pub fn plan(m: &Model) -> Option<&PlanLimits> {
    m.live.panes_logic.plan_limits().or_else(dev_plan)
}

/// The shortcut of a command as the settings store it (`"Ctrl+Shift+P"`), for
/// a tooltip or a menu line (`useShortcutLabel`). Empty when it has none, and
/// the widgets that show it leave it out then.
///
/// The binding is handed over as written, not as `KeymapIndex::label` spells
/// it for the eye: hangar-ui's buttons, tips and menus format it themselves
/// (`format_binding`), and a binding formatted twice reads differently on the
/// platforms that write keys as symbols.
pub fn binding(ctx: &Ctx<'_>, id: CommandId) -> String {
    ctx.keymap.first_binding(id).unwrap_or_default().to_owned()
}

/// A tab of an enabled kit, for the toolbar's Kits menu (`KitMenuEntry`).
#[derive(Debug, Clone, PartialEq, Eq)]
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
/// The kits are the workspace's own and the ones enabled everywhere
/// (`effectiveKits`); which of their tabs may open is the kit tabs package's
/// rule (`kit_tabs::choices`, `workspaceKitTabs`). Opening one is the kit tabs
/// package's, through `Intent::OpenKitTab`.
pub fn kit_tabs(m: &Model, ws: &Workspace) -> Option<(Vec<KitTab>, bool)> {
    if !crate::kit_tabs::has_kits(m, ws) {
        return None;
    }
    let tabs = crate::kit_tabs::choices(m, ws)
        .into_iter()
        .map(|choice| KitTab {
            kit_id: choice.kit_id,
            tab_id: choice.tab.id,
            title: choice.tab.title,
            kit: choice.kit_name,
        })
        .collect();
    Some((tabs, m.live.kits.loading()))
}

/// When a workspace fell asleep (`useAsleep`), in ms since the epoch.
pub fn asleep_at(m: &Model, ws: &Workspace) -> Option<i64> {
    m.live
        .panes_logic
        .asleep()
        .get(&ws.id)
        .and_then(|asleep| i64::try_from(asleep.at).ok())
}

// ---- Development aids -----------------------------------------------------------------------------

/// `HANGAR_DEV_PLAN=five_hour:72:2026-11-04T20:01:00Z,seven_day:35` shows the
/// plan block while no pane has reported one: the real report wins as soon as
/// there is one. Debug builds only.
fn dev_plan<'a>() -> Option<&'a PlanLimits> {
    use std::sync::OnceLock;
    static PLAN: OnceLock<Option<PlanLimits>> = OnceLock::new();
    if !cfg!(debug_assertions) {
        return None;
    }
    PLAN.get_or_init(|| {
        let spec = std::env::var("HANGAR_DEV_PLAN").ok()?;
        let limits: Vec<crate::state::live::RateLimit> = spec
            .split(',')
            .filter_map(|window| {
                let mut parts = window.splitn(3, ':');
                let kind = parts.next()?.trim().to_string();
                let percent_used = parts.next()?.trim().parse().ok()?;
                let resets_at = parts.next().map(|at| at.trim().to_string());
                Some(crate::state::live::RateLimit {
                    kind,
                    percent_used,
                    resets_at,
                })
            })
            .collect();
        super::plan::take(&limits)
    })
    .as_ref()
}

#[cfg(test)]
mod tests;
