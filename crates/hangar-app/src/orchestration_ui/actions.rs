//! Pinning, unpinning and creating a workspace's orchestrator, and opening
//! the project manager: the one Claude Code pane of a workspace that plans on
//! the board and starts its workers (`lib/orchestrationActions.ts`). The pane
//! menu, the palette and the orchestration panel all come through here, so
//! the rule (one per workspace, Claude Code only, never a worker) is held in
//! one place.
//!
//! The store is reached through [`OrchestrationStore`], which `Model`
//! implements with `apply` and the tests implement in memory, as the TS tests
//! faked the store. Everything here is synchronous: the one asynchronous step
//! of the TS (`hqDir`, the folder of the HQ) is cut out of `open_manager`
//! into its own message, see [`ManagerStep`].

use std::collections::HashMap;
use std::sync::Mutex;

use hangar_i18n::Locale;

use crate::state::live::AgentActivity;
use crate::state::roles::{hq_of, manager_of, orchestration_of, orchestrator_of};
use crate::state::store::{PanePatch, WorkspaceDraft, WorkspacePatch};
use crate::state::types::{AgentId, AppState, Pane, PaneId, PaneRole, SplitDir, Workspace, WsId};
use crate::state::{Model, Patch, StoreAction};

/// Why a pane cannot be pinned, or `None` when it can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinRefusal {
    /// No such pane in the workspace.
    Missing,
    /// It runs something else than Claude Code: the briefing is Claude Code's
    /// mod.
    NotClaude,
    /// It is already a worker, on a task of its own.
    Worker,
    /// It is the orchestrator already.
    Pinned,
    /// Another pane of the workspace is.
    Taken,
    /// It is the project manager: its role is its own, never an
    /// orchestrator's.
    Manager,
    /// The workspace is the HQ, where the manager lives and no project is
    /// planned.
    Hq,
}

pub fn pin_refusal(ws: &Workspace, pane_id: &str) -> Option<PinRefusal> {
    let Some(pane) = ws.panes.iter().find(|pane| pane.id == pane_id) else {
        return Some(PinRefusal::Missing);
    };
    if pane.role == Some(PaneRole::Manager) {
        return Some(PinRefusal::Manager);
    }
    if ws.hq == Some(true) {
        return Some(PinRefusal::Hq);
    }
    if pane.role == Some(PaneRole::Orchestrator) {
        return Some(PinRefusal::Pinned);
    }
    if pane.role == Some(PaneRole::Worker) {
        return Some(PinRefusal::Worker);
    }
    if pane.agent != AgentId::Claude {
        return Some(PinRefusal::NotClaude);
    }
    if orchestrator_of(ws).is_some() {
        return Some(PinRefusal::Taken);
    }
    None
}

/// Whether the workspace can have an orchestrator created in it.
pub fn can_create_orchestrator(ws: &Workspace) -> bool {
    ws.hq != Some(true) && orchestrator_of(ws).is_none()
}

/// A Claude Code pane that is given a role while it runs keeps the session it
/// started with, whose `--mcp-config` (Hangar's server: the board, memory and
/// `worker_*` tools) was only added at launch to panes that had a role then.
/// Hangar restarts it on the same conversation (`--resume`); the decision is
/// made here, from plain values, and acted on by the pane (P5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleRestart {
    /// Nothing to do: not Claude Code, no role, it has the server, or it
    /// started with the role.
    Skip,
    /// In the middle of a turn: wait for it to end.
    Wait,
    /// Restart now.
    Now,
    /// It lacks the server but has no conversation to resume: a restart would
    /// lose it.
    Unresumable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleRestartInput {
    pub agent: AgentId,
    /// The role the pane has now.
    pub role: Option<PaneRole>,
    /// The role it was launched with: a role from the start means the server
    /// was added.
    pub launched_role: Option<PaneRole>,
    /// `PaneMcp.hangar` of this launch; `None` when it was not recorded.
    pub hangar: Option<bool>,
    /// The pane is running a turn, or waiting for an answer inside one.
    pub mid_turn: bool,
    pub session_id: Option<String>,
}

/// Whether the pane is inside a turn, as its activity says.
pub fn is_mid_turn(activity: AgentActivity) -> bool {
    matches!(activity, AgentActivity::Working | AgentActivity::Waiting)
}

pub fn role_restart(input: &RoleRestartInput) -> RoleRestart {
    if input.agent != AgentId::Claude {
        return RoleRestart::Skip;
    }
    if input.role.is_none() || input.launched_role.is_some() {
        return RoleRestart::Skip;
    }
    // Only a launch known to have gone without the server: unknown is left
    // alone.
    if input.hangar != Some(false) {
        return RoleRestart::Skip;
    }
    if input.mid_turn {
        return RoleRestart::Wait;
    }
    if input.session_id.is_some() {
        RoleRestart::Now
    } else {
        RoleRestart::Unresumable
    }
}

/// The three lines a restart for a role can leave in the pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartText {
    Restarting,
    Failed,
    Unresumable,
}

/// What the pane's terminal says about the restart, in the person's language.
/// It is written into the terminal, not drawn by the interface, so it is not
/// in the dictionary (the TS kept it in `RESTART_TEXT` for the same reason).
pub fn restart_text(locale: Locale, which: RestartText) -> &'static str {
    match (locale, which) {
        (Locale::En, RestartText::Restarting) => {
            "Hangar is restarting this pane on the same conversation to give it Hangar's tools."
        }
        (Locale::En, RestartText::Failed) => {
            "Hangar could not restart this pane, so it has its role but not Hangar's tools: restart it yourself (it resumes the conversation)."
        }
        (Locale::En, RestartText::Unresumable) => {
            "This pane has no saved conversation to resume, so Hangar did not restart it: it has its role but not Hangar's tools until you restart it."
        }
        (Locale::Fr, RestartText::Restarting) => {
            "Hangar relance ce panneau sur la même conversation pour lui donner les outils Hangar."
        }
        (Locale::Fr, RestartText::Failed) => {
            "Hangar n'a pas pu relancer ce panneau : il a son rôle mais pas les outils Hangar. Relancez-le vous-même (il reprend la conversation)."
        }
        (Locale::Fr, RestartText::Unresumable) => {
            "Ce panneau n'a pas de conversation enregistrée à reprendre, Hangar ne l'a donc pas relancé : il a son rôle mais pas les outils Hangar tant que vous ne le relancez pas."
        }
    }
}

/// The line a restart leaves for the pane that replaces it: the restart gives
/// the pane a new id, so the old terminal's line is gone with it. Keyed by the
/// conversation, which the new pane resumes.
static RESTART_NOTES: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

pub fn queue_restart_note(session_id: &str, note: &str) {
    RESTART_NOTES
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(session_id.to_owned(), note.to_owned());
}

/// The note queued for this conversation, once: taking it removes it.
pub fn take_restart_note(session_id: Option<&str>) -> Option<String> {
    let session_id = session_id?;
    RESTART_NOTES
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .as_mut()?
        .remove(session_id)
}

/// What `open_manager` did, for the notice that follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenManager {
    /// The HQ and its manager did not exist: both are there now.
    Created { workspace: WsId, pane: PaneId },
    /// The HQ was there without its manager (closed): a new one is in it.
    Restored { workspace: WsId, pane: PaneId },
    /// The manager was there already: shown.
    Shown { workspace: WsId, pane: PaneId },
    /// The HQ has no room for a pane, or its folder could not be made.
    Failed { error: Option<String> },
}

/// What the actions read and do in the store. `Model` implements it with
/// `apply`; the tests, in memory.
pub trait OrchestrationStore {
    fn app_state(&self) -> &AppState;
    fn update_pane(&mut self, ws: &str, pane: &str, patch: PanePatch);
    /// A new Claude Code pane, with `init` applied before anything spawns it
    /// (the role it must launch with, inventory A6). `None` at `MAX_PANES`.
    fn add_claude_pane(
        &mut self,
        ws: &str,
        near: Option<PaneId>,
        dir: Option<SplitDir>,
        init: PanePatch,
    ) -> Option<PaneId>;
    fn add_workspace(&mut self, draft: WorkspaceDraft) -> Option<WsId>;
    fn update_workspace(&mut self, ws: &str, patch: WorkspacePatch);
    fn set_active_workspace(&mut self, ws: Option<WsId>);
}

impl OrchestrationStore for Model {
    fn app_state(&self) -> &AppState {
        self.state()
    }

    fn update_pane(&mut self, ws: &str, pane: &str, patch: PanePatch) {
        self.apply(StoreAction::UpdatePane {
            ws: ws.to_owned(),
            pane: pane.to_owned(),
            patch,
        });
    }

    fn add_claude_pane(
        &mut self,
        ws: &str,
        near: Option<PaneId>,
        dir: Option<SplitDir>,
        init: PanePatch,
    ) -> Option<PaneId> {
        self.apply(StoreAction::AddPane {
            ws: ws.to_owned(),
            agent: Some(AgentId::Claude),
            near,
            dir,
            cwd: None,
            name: None,
            init: Some(init),
        })
        .pane
    }

    fn add_workspace(&mut self, draft: WorkspaceDraft) -> Option<WsId> {
        self.apply(StoreAction::AddWorkspace(draft)).workspace
    }

    fn update_workspace(&mut self, ws: &str, patch: WorkspacePatch) {
        self.apply(StoreAction::UpdateWorkspace(ws.to_owned(), patch));
    }

    fn set_active_workspace(&mut self, ws: Option<WsId>) {
        self.apply(StoreAction::SetActiveWorkspace(ws));
    }
}

fn workspace<'a>(store: &'a impl OrchestrationStore, id: &str) -> Option<&'a Workspace> {
    store.app_state().workspaces.iter().find(|ws| ws.id == id)
}

/// Pins the pane; `false` when `pin_refusal` has a reason not to. The caller
/// tells the session of its new role (`Intent::AnnounceRole`): the pane may be
/// in the middle of a session, whose system prompt was settled when it
/// started, so it gets a one-time message from Hangar, written once the store
/// holds the role the briefing is made from.
pub fn pin(store: &mut impl OrchestrationStore, ws_id: &str, pane_id: &str) -> bool {
    match workspace(store, ws_id) {
        Some(ws) if pin_refusal(ws, pane_id).is_none() => {}
        _ => return false,
    }
    store.update_pane(
        ws_id,
        pane_id,
        Patch::new(|pane: &mut Pane| pane.role = Some(PaneRole::Orchestrator)),
    );
    true
}

/// The pane goes back to being a pane like any other; `false` when it was not
/// the orchestrator. Its system prompt keeps the role until the next
/// compaction: the caller tells the session once that it no longer
/// orchestrates.
pub fn unpin(store: &mut impl OrchestrationStore, ws_id: &str, pane_id: &str) -> bool {
    let is_orchestrator = workspace(store, ws_id)
        .and_then(|ws| ws.panes.iter().find(|pane| pane.id == pane_id))
        .is_some_and(|pane| pane.role == Some(PaneRole::Orchestrator));
    if !is_orchestrator {
        return false;
    }
    store.update_pane(
        ws_id,
        pane_id,
        Patch::new(|pane: &mut Pane| pane.role = None),
    );
    true
}

/// A new Claude Code pane beside `near`, pinned from the start and on the
/// orchestrator's model: its id, or `None` when the workspace already has an
/// orchestrator or no room for a pane.
///
/// The TS split along the longer side of `near`'s rectangle (`preferredDir`),
/// read from the DOM. The pane grid (P5) owns that rectangle here, and the
/// store's default, to the right, is what it did without one.
pub fn create(
    store: &mut impl OrchestrationStore,
    ws_id: &str,
    near: Option<PaneId>,
) -> Option<PaneId> {
    let ws = workspace(store, ws_id)?;
    if !can_create_orchestrator(ws) {
        return None;
    }
    let model = orchestration_of(ws).orchestrator_model;
    // The role goes in with the pane, before it mounts and launches, so it
    // starts as the orchestrator it was created to be.
    store.add_claude_pane(
        ws_id,
        near,
        None,
        Patch::new(move |pane: &mut Pane| {
            pane.role = Some(PaneRole::Orchestrator);
            pane.model = Some(model.clone());
        }),
    )
}

/// What opening the manager needs next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagerStep {
    /// Settled without waiting for anything.
    Done(OpenManager),
    /// There is no HQ yet: its folder is to be made (`hq_dir`), off the UI
    /// thread, and `finish_open_manager` called with the answer.
    NeedsFolder,
}

/// « Ouvrir le chef de projet »: the one way to the manager. Hangar has one
/// manager, in one HQ workspace: with neither, both are made (the pane a
/// Claude Code with the role from the start, on the orchestrator's model);
/// with the HQ only, a new manager is added; with both, they are shown. Never
/// a second manager.
///
/// The TS made the folder in the middle (`await deps.hqDir()`) and guarded the
/// whole with a single flight. Here the part before the folder and the part
/// after it are two calls; the caller makes the folder in between, and its
/// [`SingleFlight`] keeps a second request from starting while one waits.
pub fn begin_open_manager(store: &mut impl OrchestrationStore, hq_name: &str) -> ManagerStep {
    if let Some((ws, pane)) = manager_of(store.app_state()) {
        let (workspace, pane) = (ws.id.clone(), pane.id.clone());
        store.set_active_workspace(Some(workspace.clone()));
        return ManagerStep::Done(OpenManager::Shown { workspace, pane });
    }
    if hq_of(store.app_state()).is_none() {
        return ManagerStep::NeedsFolder;
    }
    ManagerStep::Done(place_manager(store, hq_name, None))
}

/// The second half: the HQ's folder (or why it could not be made).
pub fn finish_open_manager(
    store: &mut impl OrchestrationStore,
    hq_name: &str,
    folder: Result<String, String>,
) -> OpenManager {
    match folder {
        Ok(cwd) => place_manager(store, hq_name, Some(cwd)),
        Err(error) => OpenManager::Failed { error: Some(error) },
    }
}

/// The HQ, made if it has to be, and a manager in it.
fn place_manager(
    store: &mut impl OrchestrationStore,
    hq_name: &str,
    folder: Option<String>,
) -> OpenManager {
    let mut created = false;
    // Another request may have made the HQ while its folder was.
    let mut hq_id = hq_of(store.app_state()).map(|ws| ws.id.clone());
    if hq_id.is_none() {
        let Some(cwd) = folder else {
            return OpenManager::Failed { error: None };
        };
        let Some(id) = store.add_workspace(WorkspaceDraft {
            name: hq_name.to_owned(),
            cwd,
            layout: 1,
            agents: vec![AgentId::Claude],
            ..WorkspaceDraft::default()
        }) else {
            return OpenManager::Failed { error: None };
        };
        store.update_workspace(&id, Patch::new(|ws: &mut Workspace| ws.hq = Some(true)));
        hq_id = Some(id);
        created = true;
    }
    let Some(hq) = hq_id.and_then(|id| workspace(store, &id)) else {
        return OpenManager::Failed { error: None };
    };
    let hq_id = hq.id.clone();
    let model = orchestration_of(hq).orchestrator_model;
    let first_pane = hq.panes.first().map(|pane| pane.id.clone());

    let as_manager = {
        let model = model.clone();
        Patch::new(move |pane: &mut Pane| {
            pane.role = Some(PaneRole::Manager);
            pane.model = Some(model.clone());
        })
    };
    // The first pane of a new HQ is the Claude Code it was made with; an HQ
    // whose manager was closed gets one again.
    let pane = if created {
        let Some(pane) = first_pane else {
            return OpenManager::Failed { error: None };
        };
        // In the same update as the creation: the pane mounts, and launches,
        // as the manager.
        store.update_pane(&hq_id, &pane, as_manager);
        pane
    } else {
        match store.add_claude_pane(&hq_id, None, Some(SplitDir::Row), as_manager) {
            Some(pane) => pane,
            None => return OpenManager::Failed { error: None },
        }
    };
    store.set_active_workspace(Some(hq_id.clone()));
    if created {
        OpenManager::Created {
            workspace: hq_id,
            pane,
        }
    } else {
        OpenManager::Restored {
            workspace: hq_id,
            pane,
        }
    }
}

/// The guard that keeps two requests for the manager from both going to make
/// the HQ's folder: the second is ignored while the first waits for it.
#[derive(Debug, Default)]
pub struct SingleFlight(bool);

impl SingleFlight {
    /// `true` when the caller may go ahead, and must call `end` after.
    pub fn begin(&mut self) -> bool {
        !std::mem::replace(&mut self.0, true)
    }

    pub fn end(&mut self) {
        self.0 = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(id: &str, agent: AgentId, role: Option<PaneRole>) -> Pane {
        Pane {
            id: id.into(),
            name: id.into(),
            agent,
            role,
            ..Pane::default()
        }
    }

    fn claude(id: &str) -> Pane {
        pane(id, AgentId::Claude, None)
    }

    fn with_role(id: &str, role: PaneRole) -> Pane {
        pane(id, AgentId::Claude, Some(role))
    }

    fn ws_of(panes: Vec<Pane>) -> Workspace {
        Workspace {
            panes,
            ..Workspace::default()
        }
    }

    #[test]
    fn pins_a_claude_code_pane_in_a_workspace_that_has_no_orchestrator() {
        let ws = ws_of(vec![claude("a"), pane("b", AgentId::Codex, None)]);
        assert_eq!(pin_refusal(&ws, "a"), None);
    }

    #[test]
    fn keeps_to_one_orchestrator_per_workspace() {
        let ws = ws_of(vec![with_role("a", PaneRole::Orchestrator), claude("b")]);
        assert_eq!(pin_refusal(&ws, "b"), Some(PinRefusal::Taken));
        assert_eq!(pin_refusal(&ws, "a"), Some(PinRefusal::Pinned));
    }

    #[test]
    fn pins_claude_code_only_the_role_reaches_the_agent_through_its_mod() {
        for agent in [
            AgentId::Shell,
            AgentId::Codex,
            AgentId::Gemini,
            AgentId::Opencode,
        ] {
            let ws = ws_of(vec![pane("x", agent, None)]);
            assert_eq!(
                pin_refusal(&ws, "x"),
                Some(PinRefusal::NotClaude),
                "{agent:?}"
            );
        }
    }

    #[test]
    fn never_turns_a_worker_into_the_orchestrator() {
        let mut worker = with_role("w", PaneRole::Worker);
        worker.task_id = Some("1".into());
        let ws = ws_of(vec![worker]);
        assert_eq!(pin_refusal(&ws, "w"), Some(PinRefusal::Worker));
    }

    #[test]
    fn says_so_when_the_pane_is_gone() {
        let ws = ws_of(vec![claude("a")]);
        assert_eq!(pin_refusal(&ws, "zz"), Some(PinRefusal::Missing));
    }

    #[test]
    fn creates_an_orchestrator_only_where_there_is_none_workers_or_not() {
        let with_worker = ws_of(vec![claude("a"), with_role("w", PaneRole::Worker)]);
        assert!(can_create_orchestrator(&with_worker));
        let with_one = ws_of(vec![with_role("a", PaneRole::Orchestrator)]);
        assert!(!can_create_orchestrator(&with_one));
    }

    #[test]
    fn refuses_an_orchestrator_in_the_hq_and_a_manager_pane_as_one() {
        let hq = Workspace {
            hq: Some(true),
            panes: vec![claude("a")],
            ..Workspace::default()
        };
        assert_eq!(pin_refusal(&hq, "a"), Some(PinRefusal::Hq));
        assert!(!can_create_orchestrator(&Workspace {
            hq: Some(true),
            ..Workspace::default()
        }));
        let managed = ws_of(vec![with_role("m", PaneRole::Manager)]);
        assert_eq!(pin_refusal(&managed, "m"), Some(PinRefusal::Manager));
        // Wherever it sits, never a worker's or an orchestrator's.
        let in_hq = Workspace {
            hq: Some(true),
            panes: vec![with_role("m", PaneRole::Manager)],
            ..Workspace::default()
        };
        assert_eq!(pin_refusal(&in_hq, "m"), Some(PinRefusal::Manager));
    }

    /// A tiny store: what the actions read and do, in memory.
    #[derive(Default)]
    struct FakeStore {
        state: AppState,
        n: u32,
    }

    impl FakeStore {
        fn new(workspaces: Vec<Workspace>) -> Self {
            FakeStore {
                state: AppState {
                    workspaces,
                    ..AppState::default()
                },
                n: 0,
            }
        }

        fn id(&mut self, prefix: &str) -> String {
            self.n += 1;
            format!("{prefix}{}", self.n)
        }

        fn managers(&self) -> Vec<&Pane> {
            self.state
                .workspaces
                .iter()
                .flat_map(|ws| ws.panes.iter())
                .filter(|pane| pane.role == Some(PaneRole::Manager))
                .collect()
        }
    }

    impl OrchestrationStore for FakeStore {
        fn app_state(&self) -> &AppState {
            &self.state
        }

        fn update_pane(&mut self, ws: &str, pane: &str, patch: PanePatch) {
            for entry in &mut self.state.workspaces {
                if entry.id == ws {
                    for p in &mut entry.panes {
                        if p.id == pane {
                            patch.apply(p);
                        }
                    }
                }
            }
        }

        fn add_claude_pane(
            &mut self,
            ws: &str,
            _near: Option<PaneId>,
            _dir: Option<SplitDir>,
            init: PanePatch,
        ) -> Option<PaneId> {
            let id = self.id("p");
            let mut fresh = claude(&id);
            init.apply(&mut fresh);
            let entry = self.state.workspaces.iter_mut().find(|w| w.id == ws)?;
            entry.panes.push(fresh);
            Some(id)
        }

        fn add_workspace(&mut self, draft: WorkspaceDraft) -> Option<WsId> {
            let id = self.id("ws");
            let first = self.id("p");
            self.state.workspaces.push(Workspace {
                id: id.clone(),
                name: draft.name,
                cwd: draft.cwd,
                panes: vec![claude(&first)],
                ..Workspace::default()
            });
            self.state.active_workspace_id = Some(id.clone());
            Some(id)
        }

        fn update_workspace(&mut self, ws: &str, patch: WorkspacePatch) {
            if let Some(entry) = self.state.workspaces.iter_mut().find(|w| w.id == ws) {
                patch.apply(entry);
            }
        }

        fn set_active_workspace(&mut self, ws: Option<WsId>) {
            self.state.active_workspace_id = ws;
        }
    }

    fn open(store: &mut FakeStore, folder: &str) -> OpenManager {
        match begin_open_manager(store, "HQ") {
            ManagerStep::Done(done) => done,
            ManagerStep::NeedsFolder => finish_open_manager(store, "HQ", Ok(folder.to_owned())),
        }
    }

    #[test]
    fn creates_the_hq_once_a_claude_code_with_the_role_on_the_orchestrators_model() {
        let mut store = FakeStore::new(Vec::new());
        assert_eq!(
            begin_open_manager(&mut store, "HQ"),
            ManagerStep::NeedsFolder
        );
        let first = finish_open_manager(&mut store, "HQ", Ok("/data/hq".into()));
        assert!(matches!(first, OpenManager::Created { .. }));

        let hq = &store.state.workspaces[0];
        assert_eq!(hq.hq, Some(true));
        assert_eq!(hq.name, "HQ");
        assert_eq!(hq.cwd, "/data/hq");
        assert_eq!(hq.panes.len(), 1);
        assert_eq!(hq.panes[0].agent, AgentId::Claude);
        assert_eq!(hq.panes[0].role, Some(PaneRole::Manager));
        assert_eq!(hq.panes[0].model.as_deref(), Some("opus"));
        assert_eq!(store.state.active_workspace_id.as_ref(), Some(&hq.id));

        // A second call only switches to it, and asks for no folder.
        let (hq_id, pane_id) = (hq.id.clone(), hq.panes[0].id.clone());
        store.state.active_workspace_id = Some("elsewhere".into());
        assert_eq!(
            begin_open_manager(&mut store, "HQ"),
            ManagerStep::Done(OpenManager::Shown {
                workspace: hq_id.clone(),
                pane: pane_id
            })
        );
        assert_eq!(store.state.active_workspace_id, Some(hq_id));
        assert_eq!(store.state.workspaces.len(), 1);
        assert_eq!(store.managers().len(), 1);
    }

    #[test]
    fn adds_a_manager_again_to_an_hq_whose_manager_was_closed() {
        let mut store = FakeStore::new(vec![Workspace {
            id: "hq".into(),
            name: "HQ".into(),
            cwd: "/data/hq".into(),
            hq: Some(true),
            ..Workspace::default()
        }]);
        let result = open(&mut store, "/data/hq");
        assert!(matches!(result, OpenManager::Restored { .. }));
        assert_eq!(store.managers().len(), 1);
        let manager = store.managers()[0];
        assert_eq!(manager.role, Some(PaneRole::Manager));
        assert_eq!(manager.agent, AgentId::Claude);
        assert_eq!(manager.model.as_deref(), Some("opus"));
        assert_eq!(store.state.active_workspace_id.as_deref(), Some("hq"));
    }

    #[test]
    fn never_makes_two_managers_even_when_asked_twice_at_once() {
        let mut store = FakeStore::new(Vec::new());
        let mut flight = SingleFlight::default();
        // The first asks and waits for the folder; the second comes in while
        // it waits and is turned away.
        assert!(flight.begin());
        assert_eq!(
            begin_open_manager(&mut store, "HQ"),
            ManagerStep::NeedsFolder
        );
        assert!(!flight.begin());
        let result = finish_open_manager(&mut store, "HQ", Ok("/data/hq".into()));
        flight.end();
        assert!(matches!(result, OpenManager::Created { .. }));
        assert_eq!(store.state.workspaces.len(), 1);
        assert_eq!(store.managers().len(), 1);
        // Asked again once it is over, it shows what is there.
        assert!(flight.begin());
        assert!(matches!(
            begin_open_manager(&mut store, "HQ"),
            ManagerStep::Done(OpenManager::Shown { .. })
        ));
        assert_eq!(store.managers().len(), 1);
    }

    #[test]
    fn says_so_when_the_folder_cannot_be_made_and_creates_nothing() {
        let mut store = FakeStore::new(Vec::new());
        assert_eq!(
            begin_open_manager(&mut store, "HQ"),
            ManagerStep::NeedsFolder
        );
        let result = finish_open_manager(&mut store, "HQ", Err("denied".into()));
        assert_eq!(
            result,
            OpenManager::Failed {
                error: Some("denied".into())
            }
        );
        assert!(store.state.workspaces.is_empty());
    }

    #[test]
    fn pins_and_unpins_through_the_store() {
        let mut store = FakeStore::new(vec![Workspace {
            id: "w".into(),
            panes: vec![claude("a"), claude("b")],
            ..Workspace::default()
        }]);
        assert!(pin(&mut store, "w", "a"));
        assert_eq!(
            store.state.workspaces[0].panes[0].role,
            Some(PaneRole::Orchestrator)
        );
        // One per workspace, and nothing for a pane that is not there.
        assert!(!pin(&mut store, "w", "b"));
        assert!(!pin(&mut store, "w", "zz"));
        assert!(!unpin(&mut store, "w", "b"));
        assert!(unpin(&mut store, "w", "a"));
        assert_eq!(store.state.workspaces[0].panes[0].role, None);
    }

    #[test]
    fn creates_a_pinned_pane_on_the_orchestrators_model_only_where_there_is_none() {
        let mut store = FakeStore::new(vec![Workspace {
            id: "w".into(),
            orchestration: Some(crate::state::types::OrchestrationSettings {
                orchestrator_model: "claude-opus-4-1".into(),
                ..Default::default()
            }),
            panes: vec![claude("a")],
            ..Workspace::default()
        }]);
        let id = create(&mut store, "w", Some("a".into())).expect("a new pane");
        let made = store.state.workspaces[0]
            .panes
            .iter()
            .find(|pane| pane.id == id)
            .unwrap();
        assert_eq!(made.role, Some(PaneRole::Orchestrator));
        assert_eq!(made.model.as_deref(), Some("claude-opus-4-1"));
        assert_eq!(made.agent, AgentId::Claude);
        assert_eq!(create(&mut store, "w", None), None);
        assert_eq!(create(&mut store, "nowhere", None), None);
    }

    fn base() -> RoleRestartInput {
        RoleRestartInput {
            agent: AgentId::Claude,
            role: Some(PaneRole::Orchestrator),
            launched_role: None,
            hangar: Some(false),
            mid_turn: false,
            session_id: Some("abc-123".into()),
        }
    }

    #[test]
    fn restarts_at_once_a_pinned_session_that_started_without_the_server() {
        assert_eq!(role_restart(&base()), RoleRestart::Now);
    }

    #[test]
    fn waits_for_the_end_of_the_turn_when_it_is_mid_turn() {
        let input = RoleRestartInput {
            mid_turn: true,
            ..base()
        };
        assert_eq!(role_restart(&input), RoleRestart::Wait);
    }

    #[test]
    fn leaves_a_pane_that_already_holds_the_server_alone() {
        let input = RoleRestartInput {
            hangar: Some(true),
            ..base()
        };
        assert_eq!(role_restart(&input), RoleRestart::Skip);
    }

    #[test]
    fn leaves_a_pane_alone_when_the_launch_did_not_say() {
        let input = RoleRestartInput {
            hangar: None,
            ..base()
        };
        assert_eq!(role_restart(&input), RoleRestart::Skip);
    }

    #[test]
    fn leaves_a_pane_that_was_launched_with_its_role_alone() {
        let input = RoleRestartInput {
            launched_role: Some(PaneRole::Orchestrator),
            ..base()
        };
        assert_eq!(role_restart(&input), RoleRestart::Skip);
    }

    #[test]
    fn does_nothing_for_other_agents_or_without_a_role() {
        for agent in [AgentId::Codex, AgentId::Shell] {
            let input = RoleRestartInput { agent, ..base() };
            assert_eq!(role_restart(&input), RoleRestart::Skip);
            let mid = RoleRestartInput {
                agent,
                mid_turn: true,
                ..base()
            };
            assert_eq!(role_restart(&mid), RoleRestart::Skip);
        }
        let input = RoleRestartInput {
            role: None,
            ..base()
        };
        assert_eq!(role_restart(&input), RoleRestart::Skip);
    }

    #[test]
    fn refuses_to_lose_a_conversation_that_cannot_be_resumed() {
        let input = RoleRestartInput {
            session_id: None,
            ..base()
        };
        assert_eq!(role_restart(&input), RoleRestart::Unresumable);
    }

    #[test]
    fn tells_a_turn_from_a_pause() {
        assert!(is_mid_turn(AgentActivity::Working));
        assert!(is_mid_turn(AgentActivity::Waiting));
        assert!(!is_mid_turn(AgentActivity::Idle));
        assert!(!is_mid_turn(AgentActivity::Yours));
    }

    #[test]
    fn says_it_in_english_and_french() {
        for which in [
            RestartText::Restarting,
            RestartText::Failed,
            RestartText::Unresumable,
        ] {
            assert_ne!(
                restart_text(Locale::En, which),
                restart_text(Locale::Fr, which)
            );
        }
    }

    #[test]
    fn hands_a_note_to_the_replacement_pane_once() {
        queue_restart_note("s1", "note");
        assert_eq!(take_restart_note(Some("s1")).as_deref(), Some("note"));
        assert_eq!(take_restart_note(Some("s1")), None);
        assert_eq!(take_restart_note(None), None);
    }
}
