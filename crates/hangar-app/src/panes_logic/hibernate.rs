//! Whether a workspace can be put to sleep (`lib/hibernate.ts`): its
//! terminals closed to give the memory its agents hold back, and reopened,
//! every agent on the session it was in, the next time anyone looks at it.
//! Either because nobody has used it for a while (`hibernation_verdict`), or
//! because the person asked for it from the navigator or the palette
//! (`forced_sleep_verdict`).
//!
//! Nothing here touches the app. The window gathers what it knows about each
//! pane (`hibernation`, the panes' own state) and asks; a test hands it
//! whatever it likes. The rules lean one way throughout: anything that cannot
//! be shown to be at rest keeps the workspace awake, because a conversation
//! lost to save a few hundred megabytes is not a trade anyone asked for, and
//! asking for the sleep by hand changes how long to wait, never that.

use std::collections::HashMap;

use hangar_i18n::{Key, Translator, args};

use super::agents::is_resumable;
use crate::state::live::EpochMs;
use crate::state::types::{AgentId, Pane, PaneId, PaneRole, WsId};

/// What the window knows about one pane of the workspace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaneRest {
    /// When it last printed anything, or when it was mounted if it never did.
    pub last_output_at: EpochMs,
    /// Its agent is working or waiting for an answer right now.
    pub busy: bool,
    /// When its agent last stopped working or waiting, or when it was mounted.
    pub resting_since: EpochMs,
    /// Prompts held for it until its agent is free (the hangar-bridge queue).
    pub queued: usize,
    /// The agent quit back to its shell: what the pane runs now is anyone's
    /// guess.
    pub agent_exited: bool,
    /// Whether its shell has started a process that is still running: a dev
    /// server, a watcher, a build. `None` until the system has been asked,
    /// which is only worth doing once everything else says yes.
    pub has_children: Option<bool>,
}

pub struct HibernateInput<'a> {
    pub now: EpochMs,
    /// How long a workspace has to be left alone, in ms.
    pub delay_ms: u64,
    pub panes: &'a [Pane],
    /// Its terminals are mounted in this window.
    pub mounted: bool,
    /// It is the workspace the main window shows.
    pub on_screen: bool,
    /// It is shown in a window of its own.
    pub detached: bool,
    /// One of its deliveries is being checked, reviewed or landed right now:
    /// stopping it halfway would leave a merge half done.
    pub delivering: bool,
    /// When someone last typed into it, clicked it, or sent it anything.
    pub last_interaction: EpochMs,
    /// By pane id; a pane with no entry has not been seen and counts as busy.
    pub rest: &'a HashMap<PaneId, PaneRest>,
}

/// What a sleep asked for by hand is decided on: no clock, the same
/// protections.
pub struct ForcedSleepInput<'a> {
    pub panes: &'a [Pane],
    /// Asleep already: there is nothing left to stop.
    pub asleep: bool,
    pub mounted: bool,
    /// It is the workspace the main window shows: it moves on to another
    /// first.
    pub on_screen: bool,
    /// The main window has another workspace to show in its place
    /// (`step_aside`).
    pub can_step_aside: bool,
    pub detached: bool,
    pub delivering: bool,
    pub rest: &'a HashMap<PaneId, PaneRest>,
}

/// Why a workspace stays awake: the first rule it fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Blocker {
    Asleep,
    NotMounted,
    OnScreen,
    /// On screen, with nothing else for the main window to show instead.
    Alone,
    Detached,
    Worker,
    /// The project manager lives here: it must hear events, so the HQ never
    /// sleeps.
    Manager,
    Delivering,
    RecentInteraction,
    NoAgents,
    UnknownPane,
    NotResumable,
    NoSession,
    Busy,
    RecentlyBusy,
    Queued,
    ShellOutput,
    ShellChildren,
    /// Never a rule's answer: the system could not say what the shells run,
    /// or the workspace changed while it was being asked. Either way, awake.
    UnknownProcesses,
}

/// A workspace that stays awake, and the pane that said no when it was one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Awake {
    pub reason: Blocker,
    pub pane_id: Option<PaneId>,
}

impl Awake {
    fn because(reason: Blocker) -> Awake {
        Awake {
            reason,
            pane_id: None,
        }
    }

    fn of_pane(reason: Blocker, pane: &Pane) -> Awake {
        Awake {
            reason,
            pane_id: Some(pane.id.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Awake(Awake),
    /// Every rule holds but whether these shells still run something.
    Probe {
        pane_ids: Vec<PaneId>,
    },
    /// Put it to sleep: `agents` panes running an agent will be stopped.
    Sleep {
        agents: usize,
    },
}

impl Verdict {
    fn awake(reason: Blocker) -> Verdict {
        Verdict::Awake(Awake::because(reason))
    }
}

/// "At least the delay": a pane quiet for exactly that long has waited enough.
fn waited(since: EpochMs, now: EpochMs, delay_ms: u64) -> bool {
    now.checked_sub(since)
        .is_some_and(|quiet| quiet >= delay_ms)
}

/// The rules, in the order that answers cheapest:
///
/// 1. Mounted here, not the workspace on screen, not in a window of its own.
///    Never the HQ (the manager must hear events). No worker of an
///    orchestrator in it, and no delivery running: work handed out goes on
///    with nobody looking (that is the point of handing it out), and a worker
///    stopped mid-task, or a delivery mid-merge, is work lost rather than
///    memory saved.
/// 2. Nobody has touched it for the delay.
/// 3. At least one pane runs an agent: freeing agents is the point, and a
///    workspace of shells holds little memory and all its value on screen.
/// 4. Every agent pane can come back as it was: Claude Code or Codex, with a
///    session id captured. Gemini and OpenCode cannot resume, and a pane
///    without an id would come back on a blank conversation.
/// 5. Every agent has been idle or handed back for the delay (not working,
///    not waiting for an answer) with nothing queued for it.
/// 6. Every shell has printed nothing for the delay and runs no process of
///    its own. An agent pane whose agent quit is a shell again and is held to
///    the same. Only then are the shells' processes asked about (`Probe`).
pub fn hibernation_verdict(input: &HibernateInput) -> Verdict {
    let HibernateInput {
        now,
        delay_ms,
        panes,
        ..
    } = *input;
    if !input.mounted {
        return Verdict::awake(Blocker::NotMounted);
    }
    if input.on_screen {
        return Verdict::awake(Blocker::OnScreen);
    }
    if input.detached {
        return Verdict::awake(Blocker::Detached);
    }
    if let Some(handed_out) = work_in_flight(panes, input.delivering) {
        return Verdict::Awake(handed_out);
    }
    if !waited(input.last_interaction, now, delay_ms) {
        return Verdict::awake(Blocker::RecentInteraction);
    }
    if !panes.iter().any(|pane| pane.agent != AgentId::Shell) {
        return Verdict::awake(Blocker::NoAgents);
    }
    panes_at_rest(panes, input.rest, &|since| waited(since, now, delay_ms))
}

/// The same rules for a sleep the person asked for, minus the clock: nobody
/// has to have left it alone, nor an agent to have rested, nor a shell to have
/// been quiet, for any length of time. Every protection stays (an agent at
/// work or waiting, prompts queued, a process in a shell, a session that
/// could not come back, a worker, a delivery) since asking for it by hand is
/// no reason to lose what those protect.
///
/// Two rules read differently. The workspace on screen can go, once the main
/// window has another to show in its place; a window of its own has nothing
/// else to show, so a workspace there stays. And a workspace of shells alone
/// can go too: there is little memory to free, but nothing to lose either,
/// and the person asked.
pub fn forced_sleep_verdict(input: &ForcedSleepInput) -> Verdict {
    if input.asleep {
        return Verdict::awake(Blocker::Asleep);
    }
    if input.detached {
        return Verdict::awake(Blocker::Detached);
    }
    if !input.mounted {
        return Verdict::awake(Blocker::NotMounted);
    }
    if input.on_screen && !input.can_step_aside {
        return Verdict::awake(Blocker::Alone);
    }
    match work_in_flight(input.panes, input.delivering) {
        Some(awake) => Verdict::Awake(awake),
        None => panes_at_rest(input.panes, input.rest, &|_| true),
    }
}

/// Rule 1's second half: work an orchestrator handed out goes on with nobody
/// looking, and a worker stopped mid-task, or a delivery mid-merge, is work
/// lost rather than memory saved.
fn work_in_flight(panes: &[Pane], delivering: bool) -> Option<Awake> {
    if let Some(manager) = panes
        .iter()
        .find(|pane| pane.role == Some(PaneRole::Manager))
    {
        return Some(Awake::of_pane(Blocker::Manager, manager));
    }
    if let Some(worker) = panes
        .iter()
        .find(|pane| pane.role == Some(PaneRole::Worker))
    {
        return Some(Awake::of_pane(Blocker::Worker, worker));
    }
    delivering.then(|| Awake::because(Blocker::Delivering))
}

/// Rules 4 to 6, pane by pane. `rested` says whether a pane has been quiet
/// long enough since a moment: the delay for a workspace left alone, any time
/// at all for one put to sleep by hand.
fn panes_at_rest(
    panes: &[Pane],
    rest: &HashMap<PaneId, PaneRest>,
    rested: &dyn Fn(EpochMs) -> bool,
) -> Verdict {
    let mut shells: Vec<PaneId> = Vec::new();
    for pane in panes {
        let awake = |reason: Blocker| Verdict::Awake(Awake::of_pane(reason, pane));
        let Some(state) = rest.get(&pane.id) else {
            return awake(Blocker::UnknownPane);
        };

        let mut shell = pane.agent == AgentId::Shell;
        if !shell {
            if !is_resumable(pane.agent) {
                return awake(Blocker::NotResumable);
            }
            if pane.session_id.as_deref().is_none_or(str::is_empty) {
                return awake(Blocker::NoSession);
            }
            if state.busy {
                return awake(Blocker::Busy);
            }
            if !rested(state.resting_since) {
                return awake(Blocker::RecentlyBusy);
            }
            if state.queued > 0 {
                return awake(Blocker::Queued);
            }
            shell = state.agent_exited;
        }
        if !shell {
            continue;
        }

        if !rested(state.last_output_at) {
            return awake(Blocker::ShellOutput);
        }
        match state.has_children {
            Some(true) => return awake(Blocker::ShellChildren),
            None => shells.push(pane.id.clone()),
            Some(false) => {}
        }
    }

    if !shells.is_empty() {
        return Verdict::Probe { pane_ids: shells };
    }
    Verdict::Sleep {
        agents: panes
            .iter()
            .filter(|pane| pane.agent != AgentId::Shell)
            .count(),
    }
}

/// A workspace the main window could step aside to (`Pick<Workspace, "id" |
/// "detached">`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: WsId,
    pub detached: bool,
}

pub struct StepAsideInput<'a> {
    /// The workspace leaving the screen.
    pub current: &'a str,
    /// Every workspace, in the navigator's order.
    pub workspaces: &'a [Candidate],
    /// The workspaces the main window showed, the latest first.
    pub recent: &'a [WsId],
    /// The workspaces whose terminals are mounted in the main window.
    pub mounted: &'a [WsId],
}

/// Where the main window goes when the workspace it shows is put to sleep:
/// the one used last before it among those still awake (so that putting
/// several to sleep in a row never wakes the one put to sleep just before)
/// or, none of those shown yet, the first awake in the navigator. With none
/// awake, the same among the others, which being shown wakes: a start, but
/// nothing lost. Never one in a window of its own, of which the main window
/// would only say that it is elsewhere. `None` when there is nowhere to go.
pub fn step_aside(input: &StepAsideInput) -> Option<WsId> {
    let candidates: Vec<&Candidate> = input
        .workspaces
        .iter()
        .filter(|ws| ws.id != input.current && !ws.detached)
        .collect();
    let rank = |id: &str| {
        input
            .recent
            .iter()
            .position(|recent| recent == id)
            .unwrap_or(usize::MAX)
    };
    // Stable, so ties keep the navigator's order.
    let first = |list: Vec<&Candidate>| -> Option<WsId> {
        list.into_iter()
            .min_by_key(|ws| rank(&ws.id))
            .map(|ws| ws.id.clone())
    };
    let awake: Vec<&Candidate> = candidates
        .iter()
        .copied()
        .filter(|ws| input.mounted.contains(&ws.id))
        .collect();
    first(awake).or_else(|| first(candidates))
}

/// The delays the settings offer, in minutes.
pub const HIBERNATE_DELAYS: [u32; 5] = [5, 10, 20, 30, 60];

/// How often the window looks for a workspace to put to sleep.
pub const HIBERNATE_TICK_MS: u64 = 30_000;

/// Why a workspace the person asked to put to sleep stays awake, in a
/// sentence: the tooltip of the navigator's entry, what a screen reader reads
/// with it, and the notice when the answer changes between the menu and the
/// click. About the pane that said no, by its name. `waiting` tells an agent
/// waiting for an answer from one at work, which the rules count alike.
pub fn sleep_refusal(
    t: Translator,
    verdict: &Awake,
    panes: &[Pane],
    waiting: &dyn Fn(&str) -> bool,
) -> String {
    let pane = verdict
        .pane_id
        .as_deref()
        .and_then(|id| panes.iter().find(|entry| entry.id == id));
    let name = pane.map_or("", |pane| pane.name.as_str());
    let named = |key: Key| t.t_with(key, &args!("name" => name)).into_owned();
    match verdict.reason {
        Blocker::Asleep => t.t(Key::SleepAsleep).into_owned(),
        Blocker::Detached => t.t(Key::SleepDetached).into_owned(),
        Blocker::NotMounted => t.t(Key::SleepNotMounted).into_owned(),
        Blocker::Alone => t.t(Key::SleepAlone).into_owned(),
        Blocker::Worker => named(Key::SleepWorker),
        Blocker::Manager => named(Key::SleepManager),
        Blocker::Delivering => t.t(Key::SleepDelivering).into_owned(),
        Blocker::NoAgents => t.t(Key::SleepNoAgents).into_owned(),
        Blocker::UnknownPane => named(Key::SleepStarting),
        Blocker::NotResumable => {
            let agent = pane.map_or("", |pane| pane.agent.info().label);
            t.t_with(
                Key::SleepNotResumable,
                &args!("name" => name, "agent" => agent),
            )
            .into_owned()
        }
        Blocker::NoSession => named(Key::SleepNoSession),
        Blocker::Busy | Blocker::RecentlyBusy => {
            if pane.is_some_and(|pane| waiting(&pane.id)) {
                named(Key::SleepWaiting)
            } else {
                named(Key::SleepWorking)
            }
        }
        Blocker::Queued => named(Key::SleepQueued),
        Blocker::ShellChildren => named(Key::SleepShellChildren),
        Blocker::UnknownProcesses => t.t(Key::SleepUnknownProcesses).into_owned(),
        // Only the clock of a workspace left alone says these, and a sleep
        // asked for by hand has none.
        Blocker::OnScreen | Blocker::RecentInteraction | Blocker::ShellOutput => {
            t.t(Key::SleepInUse).into_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_i18n::Locale;

    const MIN: u64 = 60_000;
    const DELAY: u64 = 10 * MIN;
    const NOW: u64 = 1_000 * MIN;
    /// Long enough ago for every rule.
    const LONG_AGO: u64 = NOW - 3 * DELAY;

    fn pane(id: &str, agent: AgentId) -> Pane {
        with_session(id, agent, Some(&format!("s-{id}")))
    }

    fn with_session(id: &str, agent: AgentId, session: Option<&str>) -> Pane {
        Pane {
            id: id.into(),
            name: id.into(),
            agent,
            session_id: session.map(str::to_string),
            ..Pane::default()
        }
    }

    /// A pane that runs no agent has no session either.
    fn shell(id: &str) -> Pane {
        with_session(id, AgentId::Shell, None)
    }

    fn role(mut pane: Pane, role: PaneRole) -> Pane {
        pane.role = Some(role);
        if role == PaneRole::Worker {
            pane.task_id = Some("t1".into());
        }
        pane
    }

    /// At rest since long ago; a shell whose children are not known yet.
    fn rest() -> PaneRest {
        PaneRest {
            last_output_at: LONG_AGO,
            busy: false,
            resting_since: LONG_AGO,
            queued: 0,
            agent_exited: false,
            has_children: None,
        }
    }

    fn rests(entries: &[(&str, PaneRest)]) -> HashMap<PaneId, PaneRest> {
        entries
            .iter()
            .map(|(id, rest)| ((*id).to_string(), rest.clone()))
            .collect()
    }

    fn all_resting(panes: &[Pane]) -> HashMap<PaneId, PaneRest> {
        panes.iter().map(|p| (p.id.clone(), rest())).collect()
    }

    fn three() -> Vec<Pane> {
        vec![
            pane("a", AgentId::Claude),
            pane("b", AgentId::Claude),
            pane("c", AgentId::Codex),
        ]
    }

    /// Two Claude Code panes and a Codex, all resting: a workspace ready to sleep.
    fn verdict_of(
        panes: &[Pane],
        rest: &HashMap<PaneId, PaneRest>,
        patch: impl FnOnce(&mut HibernateInput),
    ) -> Verdict {
        let mut input = HibernateInput {
            now: NOW,
            delay_ms: DELAY,
            panes,
            mounted: true,
            on_screen: false,
            detached: false,
            delivering: false,
            last_interaction: LONG_AGO,
            rest,
        };
        patch(&mut input);
        hibernation_verdict(&input)
    }

    fn input(patch: impl FnOnce(&mut HibernateInput)) -> Verdict {
        let panes = three();
        let rest = all_resting(&panes);
        verdict_of(&panes, &rest, patch)
    }

    fn awake(reason: Blocker, pane: &str) -> Verdict {
        Verdict::Awake(Awake {
            reason,
            pane_id: Some(pane.into()),
        })
    }

    fn reason(verdict: &Verdict) -> String {
        match verdict {
            Verdict::Awake(awake) => format!("{:?}", awake.reason),
            Verdict::Probe { .. } => "Probe".into(),
            Verdict::Sleep { .. } => "Sleep".into(),
        }
    }

    fn kind(verdict: &Verdict) -> &'static str {
        match verdict {
            Verdict::Awake(_) => "awake",
            Verdict::Probe { .. } => "probe",
            Verdict::Sleep { .. } => "sleep",
        }
    }

    fn probe(ids: &[&str]) -> Verdict {
        Verdict::Probe {
            pane_ids: ids.iter().map(|id| (*id).to_string()).collect(),
        }
    }

    // hibernationVerdict

    #[test]
    fn puts_an_untouched_workspace_of_resting_resumable_agents_to_sleep() {
        assert_eq!(input(|_| {}), Verdict::Sleep { agents: 3 });
    }

    #[test]
    fn never_touches_a_workspace_that_is_not_mounted_on_screen_or_in_its_own_window() {
        assert_eq!(reason(&input(|i| i.mounted = false)), "NotMounted");
        assert_eq!(reason(&input(|i| i.on_screen = true)), "OnScreen");
        assert_eq!(reason(&input(|i| i.detached = true)), "Detached");
    }

    #[test]
    fn waits_out_the_whole_delay_after_the_last_interaction_and_not_a_millisecond_more() {
        assert_eq!(
            reason(&input(|i| i.last_interaction = NOW - DELAY + 1)),
            "RecentInteraction"
        );
        assert_eq!(
            reason(&input(|i| i.last_interaction = NOW)),
            "RecentInteraction"
        );
        assert_eq!(kind(&input(|i| i.last_interaction = NOW - DELAY)), "sleep");
    }

    #[test]
    fn follows_the_delay_it_is_given() {
        let five_minutes = |i: &mut HibernateInput| {
            i.delay_ms = 5 * MIN;
            i.last_interaction = NOW - 6 * MIN;
        };
        assert_eq!(kind(&input(five_minutes)), "sleep");
        assert_eq!(
            reason(&input(|i| {
                five_minutes(i);
                i.delay_ms = 60 * MIN;
            })),
            "RecentInteraction"
        );
    }

    #[test]
    fn leaves_a_workspace_of_shells_alone_there_is_no_agent_to_free() {
        let panes = [shell("x"), shell("y")];
        let rest = all_resting(&panes);
        assert_eq!(reason(&verdict_of(&panes, &rest, |_| {})), "NoAgents");
    }

    #[test]
    fn keeps_awake_an_agent_that_could_not_resume_its_session() {
        for agent in [AgentId::Gemini, AgentId::Opencode] {
            let panes = [pane("a", AgentId::Claude), pane("g", agent)];
            let rest = all_resting(&panes);
            assert_eq!(
                verdict_of(&panes, &rest, |_| {}),
                awake(Blocker::NotResumable, "g")
            );
        }
    }

    #[test]
    fn keeps_awake_a_resumable_agent_whose_session_was_never_captured() {
        let panes = [
            pane("a", AgentId::Claude),
            with_session("b", AgentId::Codex, None),
        ];
        let rest = all_resting(&panes);
        assert_eq!(
            verdict_of(&panes, &rest, |_| {}),
            awake(Blocker::NoSession, "b")
        );
    }

    #[test]
    fn keeps_awake_a_workspace_with_an_agent_working_or_waiting_for_an_answer() {
        let panes = three();
        let rest = rests(&[
            ("a", rest()),
            (
                "b",
                PaneRest {
                    busy: true,
                    ..rest()
                },
            ),
            ("c", rest()),
        ]);
        assert_eq!(verdict_of(&panes, &rest, |_| {}), awake(Blocker::Busy, "b"));
    }

    #[test]
    fn waits_out_the_delay_after_an_agents_last_turn_to_the_millisecond() {
        let panes = three();
        let at = |resting_since: u64| {
            let rest = rests(&[
                ("a", rest()),
                ("b", rest()),
                (
                    "c",
                    PaneRest {
                        resting_since,
                        ..rest()
                    },
                ),
            ]);
            verdict_of(&panes, &rest, |_| {})
        };
        assert_eq!(reason(&at(NOW - DELAY + 1)), "RecentlyBusy");
        assert_eq!(kind(&at(NOW - DELAY)), "sleep");
    }

    #[test]
    fn keeps_awake_an_agent_with_prompts_queued_for_it() {
        let panes = three();
        let rest = rests(&[
            (
                "a",
                PaneRest {
                    queued: 2,
                    ..rest()
                },
            ),
            ("b", rest()),
            ("c", rest()),
        ]);
        assert_eq!(
            verdict_of(&panes, &rest, |_| {}),
            awake(Blocker::Queued, "a")
        );
    }

    mod orchestration {
        use super::*;

        fn orchestrator() -> Pane {
            role(pane("o", AgentId::Claude), PaneRole::Orchestrator)
        }

        fn worker() -> Pane {
            role(pane("w", AgentId::Claude), PaneRole::Worker)
        }

        #[test]
        fn never_puts_a_workspace_to_sleep_while_one_of_its_workers_is_there() {
            let panes = [orchestrator(), worker(), pane("c", AgentId::Codex)];
            let rest = all_resting(&panes);
            assert_eq!(
                verdict_of(&panes, &rest, |_| {}),
                awake(Blocker::Worker, "w")
            );
        }

        #[test]
        fn holds_the_workers_above_everything_else_a_workspace_left_alone_could_pass() {
            // Busy, never seen, without a session: the worker still answers first.
            let mut stuck = worker();
            stuck.session_id = None;
            let panes = [stuck, pane("c", AgentId::Codex)];
            let rest = HashMap::new();
            assert_eq!(reason(&verdict_of(&panes, &rest, |_| {})), "Worker");
        }

        #[test]
        fn never_puts_a_workspace_to_sleep_while_one_of_its_deliveries_runs() {
            assert_eq!(reason(&input(|i| i.delivering = true)), "Delivering");
            let panes = [orchestrator(), pane("c", AgentId::Codex)];
            let rest = all_resting(&panes);
            assert_eq!(
                reason(&verdict_of(&panes, &rest, |i| i.delivering = true)),
                "Delivering"
            );
        }

        #[test]
        fn lets_an_orchestrator_with_no_worker_and_nothing_landing_sleep_like_any_agent() {
            let panes = [orchestrator(), pane("c", AgentId::Codex)];
            let rest = all_resting(&panes);
            assert_eq!(
                verdict_of(&panes, &rest, |_| {}),
                Verdict::Sleep { agents: 2 }
            );
        }

        #[test]
        fn still_leaves_the_workspace_on_screen_to_the_screen() {
            let panes = [orchestrator(), worker()];
            let rest = all_resting(&panes);
            assert_eq!(
                reason(&verdict_of(&panes, &rest, |i| i.on_screen = true)),
                "OnScreen"
            );
        }
    }

    mod the_hq {
        use super::*;

        fn manager() -> Pane {
            role(pane("m", AgentId::Claude), PaneRole::Manager)
        }

        #[test]
        fn never_sleeps_left_alone_however_long_whatever_its_manager_is_doing() {
            let panes = [manager(), pane("c", AgentId::Codex)];
            let rest = all_resting(&panes);
            assert_eq!(
                verdict_of(&panes, &rest, |_| {}),
                awake(Blocker::Manager, "m")
            );
            assert_eq!(
                reason(&verdict_of(&panes, &HashMap::new(), |_| {})),
                "Manager"
            );
        }

        #[test]
        fn refuses_a_sleep_asked_for_by_hand_too_with_a_sentence() {
            let panes = [manager()];
            let rest = rests(&[("m", rest())]);
            let verdict = forced_sleep_verdict(&ForcedSleepInput {
                panes: &panes,
                asleep: false,
                mounted: true,
                on_screen: false,
                can_step_aside: true,
                detached: false,
                delivering: false,
                rest: &rest,
            });
            let Verdict::Awake(awake) = verdict else {
                panic!("expected awake");
            };
            assert_eq!(awake.reason, Blocker::Manager);
            assert_eq!(
                sleep_refusal(Translator::new(Locale::En), &awake, &panes, &|_| false),
                "m is the project manager: it has to hear what happens"
            );
        }
    }

    #[test]
    fn keeps_awake_while_a_pane_has_not_been_seen_at_all() {
        let panes = three();
        let rest = rests(&[("a", rest()), ("b", rest())]);
        assert_eq!(
            verdict_of(&panes, &rest, |_| {}),
            awake(Blocker::UnknownPane, "c")
        );
    }

    mod shells {
        use super::*;

        fn with_shells() -> Vec<Pane> {
            vec![pane("a", AgentId::Claude), shell("sh"), shell("sh2")]
        }

        #[test]
        fn asks_about_the_shells_processes_only_once_everything_else_holds() {
            let panes = with_shells();
            let rest = all_resting(&panes);
            assert_eq!(verdict_of(&panes, &rest, |_| {}), probe(&["sh", "sh2"]));
            // An agent still working is answer enough: nobody is asked anything.
            let busy = rests(&[
                (
                    "a",
                    PaneRest {
                        busy: true,
                        ..rest.get("a").unwrap().clone()
                    },
                ),
                ("sh", self::rest()),
                ("sh2", self::rest()),
            ]);
            assert_eq!(reason(&verdict_of(&panes, &busy, |_| {})), "Busy");
        }

        #[test]
        fn sleeps_once_the_shells_are_known_to_run_nothing() {
            let panes = with_shells();
            let quiet = PaneRest {
                has_children: Some(false),
                ..rest()
            };
            let rest = rests(&[("a", rest()), ("sh", quiet.clone()), ("sh2", quiet)]);
            assert_eq!(
                verdict_of(&panes, &rest, |_| {}),
                Verdict::Sleep { agents: 1 }
            );
        }

        #[test]
        fn keeps_awake_a_shell_running_a_dev_server_a_watcher_or_a_build() {
            let panes = with_shells();
            let rest = rests(&[
                ("a", rest()),
                (
                    "sh",
                    PaneRest {
                        has_children: Some(false),
                        ..rest()
                    },
                ),
                (
                    "sh2",
                    PaneRest {
                        has_children: Some(true),
                        ..rest()
                    },
                ),
            ]);
            assert_eq!(
                verdict_of(&panes, &rest, |_| {}),
                awake(Blocker::ShellChildren, "sh2")
            );
        }

        #[test]
        fn keeps_awake_a_shell_that_printed_within_the_delay_to_the_millisecond() {
            let panes = with_shells();
            let at = |last_output_at: u64| {
                let rest = rests(&[
                    ("a", rest()),
                    (
                        "sh",
                        PaneRest {
                            last_output_at,
                            ..rest()
                        },
                    ),
                    (
                        "sh2",
                        PaneRest {
                            has_children: Some(false),
                            ..rest()
                        },
                    ),
                ]);
                verdict_of(&panes, &rest, |_| {})
            };
            assert_eq!(at(NOW - DELAY + 1), awake(Blocker::ShellOutput, "sh"));
            // Quiet for exactly the delay: only its processes are left to ask about.
            assert_eq!(at(NOW - DELAY), probe(&["sh"]));
        }

        #[test]
        fn never_asks_the_shell_of_a_running_agent_the_agent_is_its_child() {
            let panes = [pane("a", AgentId::Claude), pane("b", AgentId::Codex)];
            let rest = all_resting(&panes);
            assert_eq!(kind(&verdict_of(&panes, &rest, |_| {})), "sleep");
        }

        #[test]
        fn holds_an_agent_pane_whose_agent_quit_to_the_shell_rules() {
            let panes = [pane("a", AgentId::Claude), pane("b", AgentId::Claude)];
            let exited = |patch: PaneRest| {
                let rest = rests(&[("a", rest()), ("b", patch)]);
                verdict_of(&panes, &rest, |_| {})
            };
            let quit = || PaneRest {
                agent_exited: true,
                ..rest()
            };
            assert_eq!(exited(quit()), probe(&["b"]));
            assert_eq!(
                exited(PaneRest {
                    has_children: Some(true),
                    ..quit()
                }),
                awake(Blocker::ShellChildren, "b")
            );
            assert_eq!(
                reason(&exited(PaneRest {
                    last_output_at: NOW - MIN,
                    ..quit()
                })),
                "ShellOutput"
            );
            assert_eq!(
                exited(PaneRest {
                    has_children: Some(false),
                    ..quit()
                }),
                Verdict::Sleep { agents: 2 }
            );
        }
    }

    // forcedSleepVerdict

    /// At rest, but only just: every clock the timer waits on reads zero.
    fn just_now() -> PaneRest {
        PaneRest {
            last_output_at: NOW,
            resting_since: NOW,
            ..rest()
        }
    }

    fn forced_of(
        panes: &[Pane],
        rest: &HashMap<PaneId, PaneRest>,
        patch: impl FnOnce(&mut ForcedSleepInput),
    ) -> Verdict {
        let mut input = ForcedSleepInput {
            panes,
            asleep: false,
            mounted: true,
            on_screen: false,
            can_step_aside: true,
            detached: false,
            delivering: false,
            rest,
        };
        patch(&mut input);
        forced_sleep_verdict(&input)
    }

    fn just_now_all(panes: &[Pane]) -> HashMap<PaneId, PaneRest> {
        panes.iter().map(|p| (p.id.clone(), just_now())).collect()
    }

    fn forced(patch: impl FnOnce(&mut ForcedSleepInput)) -> Verdict {
        let panes = three();
        let rest = just_now_all(&panes);
        forced_of(&panes, &rest, patch)
    }

    mod forced_sleep {
        use super::*;

        #[test]
        fn puts_to_sleep_a_workspace_used_a_second_ago_its_agents_only_just_done() {
            assert_eq!(forced(|_| {}), Verdict::Sleep { agents: 3 });
        }

        #[test]
        fn leaves_alone_a_workspace_asleep_in_a_window_of_its_own_or_never_opened() {
            assert_eq!(
                reason(&forced(|i| {
                    i.asleep = true;
                    i.mounted = false;
                })),
                "Asleep"
            );
            assert_eq!(reason(&forced(|i| i.detached = true)), "Detached");
            assert_eq!(reason(&forced(|i| i.mounted = false)), "NotMounted");
        }

        #[test]
        fn takes_the_workspace_on_screen_once_the_window_has_another_to_show() {
            assert_eq!(kind(&forced(|i| i.on_screen = true)), "sleep");
            assert_eq!(
                reason(&forced(|i| {
                    i.on_screen = true;
                    i.can_step_aside = false;
                })),
                "Alone"
            );
            // Out of sight, having nowhere to go matters to nobody.
            assert_eq!(kind(&forced(|i| i.can_step_aside = false)), "sleep");
        }

        #[test]
        fn never_stops_a_worker_nor_a_delivery() {
            let panes = [
                pane("a", AgentId::Claude),
                role(pane("w", AgentId::Claude), PaneRole::Worker),
            ];
            let rest = just_now_all(&panes);
            assert_eq!(
                forced_of(&panes, &rest, |_| {}),
                awake(Blocker::Worker, "w")
            );
            assert_eq!(reason(&forced(|i| i.delivering = true)), "Delivering");
        }

        #[test]
        fn keeps_awake_an_agent_that_could_not_come_back_as_it_was() {
            for agent in [AgentId::Gemini, AgentId::Opencode] {
                let panes = [pane("a", AgentId::Claude), pane("g", agent)];
                let rest = just_now_all(&panes);
                assert_eq!(
                    forced_of(&panes, &rest, |_| {}),
                    awake(Blocker::NotResumable, "g")
                );
            }
            let fresh = [
                pane("a", AgentId::Claude),
                with_session("b", AgentId::Codex, None),
            ];
            let rest = just_now_all(&fresh);
            assert_eq!(
                forced_of(&fresh, &rest, |_| {}),
                awake(Blocker::NoSession, "b")
            );
        }

        #[test]
        fn keeps_awake_an_agent_working_waiting_or_with_prompts_queued() {
            let panes = three();
            let at = |b: PaneRest| {
                let rest = rests(&[("a", just_now()), ("b", b), ("c", just_now())]);
                forced_of(&panes, &rest, |_| {})
            };
            assert_eq!(
                at(PaneRest {
                    busy: true,
                    ..just_now()
                }),
                awake(Blocker::Busy, "b")
            );
            assert_eq!(
                at(PaneRest {
                    queued: 1,
                    ..just_now()
                }),
                awake(Blocker::Queued, "b")
            );
        }

        #[test]
        fn keeps_awake_while_a_pane_has_not_been_seen_at_all() {
            let panes = three();
            let rest = rests(&[("a", just_now()), ("b", just_now())]);
            assert_eq!(
                forced_of(&panes, &rest, |_| {}),
                awake(Blocker::UnknownPane, "c")
            );
        }

        mod shells {
            use super::*;

            fn with_shell(patch: PaneRest) -> Verdict {
                let panes = [pane("a", AgentId::Claude), shell("sh")];
                let rest = rests(&[("a", just_now()), ("sh", patch)]);
                forced_of(&panes, &rest, |_| {})
            }

            #[test]
            fn asks_what_a_shell_runs_however_recently_it_printed_then_goes_by_the_answer() {
                assert_eq!(with_shell(just_now()), probe(&["sh"]));
                assert_eq!(
                    with_shell(PaneRest {
                        has_children: Some(false),
                        ..just_now()
                    }),
                    Verdict::Sleep { agents: 1 }
                );
                assert_eq!(
                    with_shell(PaneRest {
                        has_children: Some(true),
                        ..just_now()
                    }),
                    awake(Blocker::ShellChildren, "sh")
                );
            }

            #[test]
            fn holds_an_agent_pane_whose_agent_quit_to_the_same() {
                let panes = [pane("a", AgentId::Claude)];
                let rest = rests(&[(
                    "a",
                    PaneRest {
                        agent_exited: true,
                        ..just_now()
                    },
                )]);
                assert_eq!(forced_of(&panes, &rest, |_| {}), probe(&["a"]));
            }

            #[test]
            fn puts_quiet_shells_alone_to_sleep_nothing_to_lose_and_it_was_asked() {
                let only = [shell("x"), shell("y")];
                let quiet = PaneRest {
                    has_children: Some(false),
                    ..rest()
                };
                let rest = rests(&[("x", quiet.clone()), ("y", quiet)]);
                assert_eq!(
                    forced_of(&only, &rest, |_| {}),
                    Verdict::Sleep { agents: 0 }
                );
                // Left alone, the same workspace is not worth the timer's while.
                assert_eq!(reason(&verdict_of(&only, &rest, |_| {})), "NoAgents");
            }
        }

        #[test]
        fn never_answers_with_the_timers_clock() {
            let timed = [
                Blocker::RecentInteraction,
                Blocker::RecentlyBusy,
                Blocker::ShellOutput,
                Blocker::OnScreen,
            ];
            let shells = [pane("a", AgentId::Claude), shell("sh")];
            let shells_rest = just_now_all(&shells);
            let busy_rest = rests(&[
                (
                    "a",
                    PaneRest {
                        busy: true,
                        ..just_now()
                    },
                ),
                ("b", just_now()),
                ("c", just_now()),
            ]);
            let cases = [
                forced(|_| {}),
                forced(|i| i.on_screen = true),
                forced_of(&shells, &shells_rest, |_| {}),
                forced_of(&three(), &busy_rest, |_| {}),
            ];
            for verdict in cases {
                if let Verdict::Awake(awake) = verdict {
                    assert!(!timed.contains(&awake.reason), "{:?}", awake.reason);
                }
            }
        }
    }

    // stepAside

    mod step_aside_rules {
        use super::*;

        fn ws(id: &str, detached: bool) -> Candidate {
            Candidate {
                id: id.into(),
                detached,
            }
        }

        fn ids(list: &[&str]) -> Vec<WsId> {
            list.iter().map(|id| (*id).to_string()).collect()
        }

        fn standard() -> Vec<Candidate> {
            vec![
                ws("a", false),
                ws("b", false),
                ws("c", false),
                ws("d", false),
            ]
        }

        fn from(recent: &[&str], mounted: &[&str], list: &[Candidate]) -> Option<WsId> {
            step_aside(&StepAsideInput {
                current: "a",
                workspaces: list,
                recent: &ids(recent),
                mounted: &ids(mounted),
            })
        }

        #[test]
        fn goes_back_to_the_workspace_used_last_before_this_one() {
            assert_eq!(
                from(&["a", "c", "b"], &["a", "b", "c"], &standard()).as_deref(),
                Some("c")
            );
        }

        #[test]
        fn passes_over_one_asleep_so_putting_several_to_sleep_never_wakes_the_last() {
            // c was shown just before, then put to sleep: it is no longer mounted.
            assert_eq!(
                from(&["a", "c", "b"], &["a", "b"], &standard()).as_deref(),
                Some("b")
            );
        }

        #[test]
        fn takes_the_first_awake_in_the_navigator_when_none_was_shown_before() {
            assert_eq!(
                from(&["a"], &["a", "d", "c"], &standard()).as_deref(),
                Some("c")
            );
        }

        #[test]
        fn wakes_one_rather_than_leave_the_window_empty_the_one_used_last_first() {
            assert_eq!(from(&["a", "d"], &["a"], &standard()).as_deref(), Some("d"));
            assert_eq!(from(&["a"], &["a"], &standard()).as_deref(), Some("b"));
        }

        #[test]
        fn never_goes_to_a_workspace_in_a_window_of_its_own_and_says_when_there_is_nowhere() {
            assert_eq!(
                from(
                    &["a", "b"],
                    &["a", "b"],
                    &[ws("a", false), ws("b", true), ws("c", false)]
                )
                .as_deref(),
                Some("c")
            );
            assert_eq!(
                from(&["a", "b"], &["a", "b"], &[ws("a", false), ws("b", true)]),
                None
            );
            assert_eq!(from(&["a"], &["a"], &[ws("a", false)]), None);
        }
    }

    // sleepRefusal

    mod refusals {
        use super::*;

        fn panes() -> Vec<Pane> {
            let mut ava = pane("p1", AgentId::Claude);
            ava.name = "Ava".into();
            let mut leo = shell("p2");
            leo.name = "Leo".into();
            let mut gus = with_session("p3", AgentId::Gemini, None);
            gus.name = "Gus".into();
            vec![ava, leo, gus]
        }

        fn say(
            locale: Locale,
            reason: Blocker,
            pane: Option<&str>,
            waiting: &dyn Fn(&str) -> bool,
        ) -> String {
            let verdict = Awake {
                reason,
                pane_id: pane.map(str::to_string),
            };
            sleep_refusal(Translator::new(locale), &verdict, &panes(), waiting)
        }

        fn calm(locale: Locale, reason: Blocker, pane: Option<&str>) -> String {
            say(locale, reason, pane, &|_| false)
        }

        #[test]
        fn names_the_pane_that_keeps_the_workspace_awake() {
            assert_eq!(
                calm(Locale::Fr, Blocker::Busy, Some("p1")),
                "Ava travaille encore"
            );
            assert_eq!(
                say(Locale::Fr, Blocker::Busy, Some("p1"), &|id| id == "p1"),
                "Ava attend ta réponse"
            );
            assert_eq!(
                calm(Locale::Fr, Blocker::ShellChildren, Some("p2")),
                "Un processus tourne dans le shell de Leo"
            );
            assert_eq!(
                calm(Locale::Fr, Blocker::NotResumable, Some("p3")),
                "Gemini ne reprend pas sa session (Gus)"
            );
            assert_eq!(
                calm(Locale::En, Blocker::ShellChildren, Some("p2")),
                "A process is running in Leo's shell"
            );
            assert_eq!(
                calm(Locale::En, Blocker::NotResumable, Some("p3")),
                "Gemini cannot resume its session (Gus)"
            );
        }

        #[test]
        fn has_a_sentence_for_every_reason_in_both_languages() {
            let every = [
                Blocker::Asleep,
                Blocker::NotMounted,
                Blocker::OnScreen,
                Blocker::Alone,
                Blocker::Detached,
                Blocker::Worker,
                Blocker::Manager,
                Blocker::Delivering,
                Blocker::RecentInteraction,
                Blocker::NoAgents,
                Blocker::UnknownPane,
                Blocker::NotResumable,
                Blocker::NoSession,
                Blocker::Busy,
                Blocker::RecentlyBusy,
                Blocker::Queued,
                Blocker::ShellOutput,
                Blocker::ShellChildren,
                Blocker::UnknownProcesses,
            ];
            for locale in [Locale::Fr, Locale::En] {
                for reason in every {
                    let text = calm(locale, reason, Some("p1"));
                    assert!(
                        !text.starts_with("sleep.")
                            && !text.contains(['{', '}'])
                            && !text.contains("undefined"),
                        "{reason:?}: {text}"
                    );
                    assert!(text.chars().count() > 5, "{reason:?}: {text}");
                }
            }
        }

        #[test]
        fn reads_as_a_notice_when_the_click_finds_the_answer_changed() {
            let reason = calm(Locale::Fr, Blocker::ShellChildren, Some("p2"));
            let notice = Translator::new(Locale::Fr)
                .t_with(
                    Key::SleepRefused,
                    &args!("name" => "checkout", "reason" => reason.as_str()),
                )
                .into_owned();
            assert_eq!(
                notice,
                "checkout reste éveillé. Un processus tourne dans le shell de Leo."
            );
        }
    }
}
