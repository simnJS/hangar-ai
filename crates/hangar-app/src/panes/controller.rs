//! A pane's life, from its id appearing in a workspace to its going: the
//! non-view half of TerminalPane.tsx (inventory §4/1.5, "Mount sequence",
//! "Unmount", and §4/3, "PaneController").
//!
//! A pane's identity is its id. A restart, a change of agent or of shell
//! gives the pane a new id upstream (`respawnPane`), which unmounts the old
//! one (its shell killed) and mounts the new one (a new shell), exactly as
//! the React effect keyed on `pane.id` did. Mounts and unmounts follow the
//! model: whatever changes a workspace's panes (a split in a header, a
//! preset, an agent asking through the control API), the next sync starts
//! what appeared and stops what went (mod.rs, `sync`).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hangar_core::kits::IssueReason;
use hangar_i18n::{Key, Translator, args};
use hangar_term::Terminal;
use iced::Task;

use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::Notification;
use crate::state::live::QueuedPrompt;
use crate::state::types::{AgentId, Pane, Settings, ShellInfo, Workspace};
use crate::state::{Model, PaneId, Patch, StoreAction, WsId};

use super::Msg;
use super::agent_watch::{ActivityWatcher, AgentWatch, Said, TRUST_LOOK, Words};
use super::hub::{Hub, HubChannel};
use super::launch::{self, LaunchOptions};
use super::look::{self, Look};
use super::pty_link::PaneLink;
use super::search::Search;
use super::status::PaneStatus;

/// A pane, while it is mounted.
pub struct PaneRt {
    pub ws: WsId,
    /// Never changes for an id; carried here for the notifications and the
    /// queue carried over a restart.
    pub name: String,
    pub agent: AgentId,
    pub link: Arc<PaneLink>,
    pub status: PaneStatus,
    pub watch: AgentWatch,
    pub watcher: ActivityWatcher,
    /// When to read the screen for the trust question.
    pub trust_due: Option<Instant>,
    /// The pane handed back while the person was looking elsewhere: its
    /// outline stays until they look.
    pub attention: bool,
    pub queue: Vec<QueuedPrompt>,
    /// What the launch had to say: kept out of the terminal, which Claude
    /// Code clears as it starts.
    pub notes: Vec<String>,
    pub dismissed: HashSet<String>,
    pub search: Option<Search>,
    /// What the search bar held when it last closed.
    pub last_query: String,
    /// The account its Claude Code runs on, as core started it.
    pub account: Option<String>,
    /// The resume the launch is waiting to type.
    pub resume: Option<String>,
    /// The look last applied, to restyle only on a change.
    pub look: Look,
    /// Whether the terminal was last told it has the keyboard.
    pub focused: bool,
    /// The launch's sleep, aborted with the pane.
    pub launch: Option<iced::task::Handle>,
}

impl std::fmt::Debug for PaneRt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaneRt")
            .field("ws", &self.ws)
            .field("name", &self.name)
            .field("status", &self.status)
            .field("watch", &self.watch)
            .finish_non_exhaustive()
    }
}

/// What `pty_spawn` answered, kept to what the pane says about it.
#[derive(Debug, Clone)]
pub struct SpawnReport {
    pub notes: Vec<(String, IssueReason, Option<String>)>,
    pub account: Option<String>,
}

/// The shell a pane runs: its own, its workspace's, the settings', then the
/// first detected; an id that no longer exists is skipped (`resolveShell` of
/// `lib/shells.ts`, a P4 module of four lines repeated here for the launch).
pub fn resolve_shell<'a>(
    shells: &'a [ShellInfo],
    preferred: [Option<&str>; 3],
) -> Option<&'a ShellInfo> {
    preferred
        .into_iter()
        .flatten()
        .filter(|id| !id.is_empty())
        .find_map(|id| shells.iter().find(|shell| shell.id == id))
        .or_else(|| shells.first())
}

pub fn shell_of<'a>(m: &'a Model, ws: &Workspace, pane: &Pane) -> Option<&'a ShellInfo> {
    resolve_shell(
        &m.ui.shells,
        [
            pane.shell_id.as_deref(),
            ws.shell_id.as_deref(),
            m.settings().shell_id.as_deref(),
        ],
    )
}

/// The kits a pane of `ws` starts with: the workspace's, then those turned on
/// everywhere (`effectiveKits`). Which of them stay out (missing, broken,
/// forbidden by a policy) is core's to say at spawn. The hooks a GitHub kit
/// changed since they were accepted (`paneKits`' `held`) are P14's to
/// check; until it does, such a kit loads with the pane.
fn kits_of(settings: &Settings, ws: &Workspace) -> Vec<String> {
    let mut ids: Vec<String> = ws.kits.iter().flatten().map(|kit| kit.id.clone()).collect();
    for kit in settings.kits_everywhere.iter().flatten() {
        if !ids.contains(&kit.id) {
            ids.push(kit.id.clone());
        }
    }
    ids
}

/// The Claude account a pane is launched on: its own, its workspace's, the
/// settings', then main. Core falls back to main for one that is gone and
/// says which it used.
fn account_of(settings: &Settings, ws: &Workspace, pane: &Pane) -> String {
    [
        pane.claude_account.as_deref(),
        ws.claude_account.as_deref(),
        settings.claude_account.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .find(|id| !id.is_empty())
    .unwrap_or("main")
    .to_owned()
}

/// Mounts `pane`: its terminal, its writer, its spawn. The spawn runs on the
/// blocking pool, since it waits up to 5 s for a shell of the same id still
/// being reaped.
#[allow(clippy::too_many_arguments)]
pub fn mount(
    hub: &Hub,
    channel: &HubChannel,
    sv: &Services,
    m: &Model,
    ws: &Workspace,
    pane: &Pane,
    carried: Vec<QueuedPrompt>,
) -> (PaneRt, Task<Message>) {
    let settings = m.settings();
    let look = look::look(settings, ws, pane.agent);
    let mut terminal = Terminal::new(look.config.clone(), 80, 24);
    terminal.set_palette(look.palette.clone());
    terminal.set_font(look.font.clone());
    let link = PaneLink::new(pane.id.clone(), terminal, sv.core().pty().clone());
    hub.insert(link.clone());

    let login = pane.agent == AgentId::Claude && pane.login == Some(true);
    let account = (pane.agent == AgentId::Claude).then(|| account_of(settings, ws, pane));
    let (columns, rows) = link.lock().size();
    let spawn = Spawn {
        id: pane.id.clone(),
        cwd: pane
            .cwd
            .clone()
            .filter(|cwd| !cwd.is_empty())
            .unwrap_or_else(|| ws.cwd.clone()),
        shell: shell_of(m, ws, pane).cloned(),
        columns,
        rows,
        name: pane.name.clone(),
        bridge: settings.claude_bridge,
        port_base: pane.port_base.and_then(|port| u16::try_from(port).ok()),
        // A pane opened to log its Claude account in runs the login and
        // nothing else: no kits, no conversation.
        kits: if login {
            Vec::new()
        } else {
            kits_of(settings, ws)
        },
        agent: pane.agent,
        account: account.clone(),
    };
    let sink = link.sink(channel.sender());
    let pty = sv.core().pty().clone();
    let id = pane.id.clone();
    let spawn_task = Task::perform(
        async move {
            tokio::task::spawn_blocking(move || spawn.run(&pty, sink))
                .await
                .unwrap_or_else(|error| Err(format!("the spawn stopped: {error}")))
        },
        move |result| Message::Panes(Msg::Spawned(id.clone(), result)),
    );

    // A restart gave this pane a new id: the prompts still queued for the
    // old one come along (`takeStashedQueue`).
    let requeue = if carried.is_empty() {
        Task::none()
    } else {
        let host = sv.core().host().clone();
        let id = pane.id.clone();
        Task::perform(
            tokio::task::spawn_blocking(move || {
                for prompt in carried {
                    hangar_core::bridge::bridge_enqueue(
                        &host,
                        id.clone(),
                        prompt.text,
                        prompt.from,
                    );
                }
            }),
            |_| Message::Noop,
        )
    };

    let rt = PaneRt {
        ws: ws.id.clone(),
        name: pane.name.clone(),
        agent: pane.agent,
        link,
        status: PaneStatus::Starting,
        watch: AgentWatch::default(),
        watcher: ActivityWatcher::default(),
        trust_due: None,
        attention: false,
        queue: Vec::new(),
        notes: Vec::new(),
        dismissed: HashSet::new(),
        search: None,
        last_query: String::new(),
        account,
        resume: None,
        look,
        focused: false,
        launch: None,
    };
    (rt, Task::batch([spawn_task, requeue]))
}

/// What `pty_spawn` is called with, gathered on the UI thread.
struct Spawn {
    id: PaneId,
    cwd: String,
    shell: Option<ShellInfo>,
    columns: u16,
    rows: u16,
    name: String,
    bridge: bool,
    port_base: Option<u16>,
    kits: Vec<String>,
    agent: AgentId,
    account: Option<String>,
}

impl Spawn {
    fn run(
        self,
        pty: &hangar_core::pty::PtyManager,
        sink: hangar_core::pty::PtySink,
    ) -> Result<SpawnReport, String> {
        let spawned = hangar_core::pty::pty_spawn(
            pty,
            sink,
            self.id,
            self.cwd,
            self.shell,
            self.columns,
            self.rows,
            Some(self.name),
            Some(self.bridge),
            self.port_base,
            // The workspace's MCP scope file (`mcpLaunchConfig`) is P10's
            // `mcpScope`; without it the agent starts with its own servers.
            None,
            Some(self.kits),
            Some(self.agent.as_str().to_owned()),
            self.account,
        )?;
        Ok(SpawnReport {
            notes: spawned
                .kit_issues
                .into_iter()
                .map(|issue| (issue.id, issue.reason, issue.detail))
                .collect(),
            account: spawned.account,
        })
    }
}

/// The lines a Claude Code pane shows before its agent starts, one per kit
/// left out and one for every kit a sideloading policy kept out
/// (`kitIssueNotes`).
fn kit_notes(tr: Translator, issues: &[(String, IssueReason, Option<String>)]) -> Vec<String> {
    let mut notes = Vec::new();
    let mut forbidden = Vec::new();
    for (id, reason, detail) in issues {
        match reason {
            IssueReason::Forbidden => forbidden.push(id.as_str()),
            IssueReason::Missing => notes.push(
                tr.t_with(Key::KitsWarnMissing, &args!("id" => id.as_str()))
                    .into_owned(),
            ),
            _ => notes.push(
                tr.t_with(
                    Key::KitsWarnBroken,
                    &args!("id" => id.as_str(), "detail" => detail.as_deref().unwrap_or_default()),
                )
                .into_owned(),
            ),
        }
    }
    if !forbidden.is_empty() {
        notes.push(
            tr.t_with(
                Key::KitsWarnForbidden,
                &args!("ids" => forbidden.join(", ")),
            )
            .into_owned(),
        );
    }
    notes
}

/// The spawn answered.
pub fn spawned(
    rt: &mut PaneRt,
    m: &Model,
    tr: Translator,
    result: Result<SpawnReport, String>,
) -> Task<Message> {
    let report = match result {
        Ok(report) => report,
        Err(error) => {
            // A pane that cannot start says so in its terminal and offers its
            // restart button, instead of sitting on "starting".
            let line = format!(
                "\r\n\x1b[31m{}: {error}\x1b[0m\r\n",
                tr.t(Key::PaneSpawnFailed)
            );
            rt.link.lock().feed(line.as_bytes());
            rt.status = PaneStatus::Exited;
            return Task::none();
        }
    };
    rt.status = PaneStatus::Running;
    // The size it was spawned at may already be stale: the widget measured
    // the pane while the shell was starting, and a resize before the PTY
    // existed went nowhere.
    let (columns, rows) = rt.link.lock().size();
    rt.link.resize(columns, rows);

    let requested = rt.account.clone();
    if rt.agent == AgentId::Claude {
        let mut notes = kit_notes(tr, &report.notes);
        if let (Some(asked), Some(used)) = (requested.as_deref(), report.account.as_deref())
            && asked != used
        {
            notes.insert(
                0,
                tr.t_with(Key::AccountsPickerMissing, &args!("id" => asked))
                    .into_owned(),
            );
        }
        rt.notes = notes;
        rt.account = report.account.clone().or(requested.clone());
    }
    if rt.agent == AgentId::Shell {
        return Task::none();
    }

    let Some((_, pane)) = find(m, &rt.ws, &rt.link.id) else {
        return Task::none();
    };
    let settings = m.settings();
    let delay = Duration::from_millis(settings.launch_delay_ms);
    let id = rt.link.id.clone();

    // The person logs in in the browser; nothing to resume or follow. Never
    // on an account the pane fell back to: logging in to an account that is
    // gone would log main in again in its place.
    if pane.login == Some(true) && rt.agent == AgentId::Claude {
        let fell_back = report.account.is_some() && report.account != requested;
        if fell_back {
            return Task::none();
        }
        let (task, handle) = Task::perform(tokio::time::sleep(delay), move |()| {
            Message::Panes(Msg::LoginReady(id.clone()))
        })
        .abortable();
        rt.launch = Some(handle.abort_on_drop());
        return task;
    }

    // Claude Code names a session the moment it starts but writes its
    // transcript only on the first message, so a pane reset and left alone
    // holds an id with nothing behind it; resuming that only prints "No
    // conversation found". Asked while the shell settles.
    //
    // TODO(P4): `mustResume` (a pane its workspace's sleep stopped resumes
    // whatever `autoResume` says) is hibernation's; until it lands only the
    // setting decides.
    let saved = settings
        .auto_resume
        .then(|| pane.session_id.clone())
        .flatten()
        .filter(|_| launch::is_resumable(rt.agent));
    rt.resume = saved.clone();
    let agent = rt.agent;
    let (task, handle) = Task::perform(
        async move {
            let check = async {
                match saved {
                    Some(id) => tokio::task::spawn_blocking(move || {
                        hangar_core::sessions::session_exists(agent.as_str().to_owned(), id)
                    })
                    .await
                    .ok()
                    .flatten(),
                    None => None,
                }
            };
            let (exists, ()) = iced::futures::join!(check, tokio::time::sleep(delay));
            exists
        },
        move |exists| Message::Panes(Msg::LaunchReady(id.clone(), exists)),
    )
    .abortable();
    rt.launch = Some(handle.abort_on_drop());
    task
}

/// The shell has settled: type the agent's command.
pub fn launch_ready(rt: &mut PaneRt, m: &mut Model, exists: Option<bool>) -> Task<Message> {
    rt.launch = None;
    let Some((ws, pane)) = find(m, &rt.ws, &rt.link.id) else {
        return Task::none();
    };
    let mut resume = rt.resume.take();
    let mut forget = false;
    if resume.is_some() && exists == Some(false) {
        resume = None;
        forget = true;
    }
    let settings = m.settings();
    let orchestration = ws.orchestration.clone().unwrap_or_default();
    let role = pane.role.is_some();
    let options = LaunchOptions {
        extra_roots: &ws.extra_roots,
        commands: Some(&settings.agent_commands),
        // Read when the agent starts: a workspace renamed later shows on the
        // next launch.
        name: Some(launch::session_name(&pane.name, &ws.name)),
        model: pane.model.as_deref(),
        // An orchestrator and its workers work while nobody watches: the mode
        // the workspace chose for them, and Hangar's own tools.
        permission_mode: if role {
            orchestration.permission_mode.as_deref()
        } else {
            None
        },
        allowed_tools: if role {
            &launch::ORCHESTRATION_ALLOWED_TOOLS
        } else {
            &[]
        },
    };
    if let Some(command) = launch::launch_command(rt.agent, resume.as_deref(), &options) {
        rt.link.write(format!("{command}\r").into_bytes());
    }
    if forget {
        let (ws, pane) = (ws.id.clone(), pane.id.clone());
        m.apply(StoreAction::UpdatePane {
            ws,
            pane,
            patch: Patch::new(|pane: &mut Pane| pane.session_id = None),
        });
    }
    // TODO(P4): `watchForSession`, which follows the transcripts of an agent
    // without the mod (Codex, a Claude Code without the bridge) to capture
    // its session, is `panes_logic::session_watch`; a pane with the mod gets
    // its session from the mod's hello.
    Task::none()
}

/// The login pane's shell has settled.
pub fn login_ready(rt: &mut PaneRt) {
    rt.launch = None;
    rt.link
        .write(format!("{}\r", launch::LOGIN_COMMAND).into_bytes());
}

/// The workspace and the pane of `pane`, if both still exist.
pub fn find<'a>(m: &'a Model, ws: &str, pane: &str) -> Option<(&'a Workspace, &'a Pane)> {
    let ws = m.workspace(ws)?;
    let pane = ws.panes.iter().find(|p| p.id == pane)?;
    Some((ws, pane))
}

/// Unmounts a pane: its shell killed (off the UI thread: the kill takes
/// core's registry lock, which a blocked write may hold), its launch
/// aborted with it.
pub fn unmount(hub: &Hub, sv: &Services, rt: PaneRt) -> Task<Message> {
    hub.remove(&rt.link.id);
    let pty = sv.core().pty().clone();
    let id = rt.link.id.clone();
    drop(rt);
    Task::perform(
        tokio::task::spawn_blocking(move || hangar_core::pty::pty_kill(&pty, id, None)),
        |_| Message::Noop,
    )
}

/// Turns what the agent watch said into the window's work: the activity
/// changes, a notification, the captured session.
pub struct Context<'a> {
    pub tr: Translator,
    pub settings: &'a Settings,
    /// The window drawing the pane has the system's focus.
    pub window_focused: bool,
    /// The person is on this pane: the window in front, the pane focused and
    /// on screen.
    pub looking: bool,
}

pub fn said(rt: &mut PaneRt, m: &mut Model, cx: &Context, said: Vec<Said>) -> Task<Message> {
    let mut tasks = Vec::new();
    for item in said {
        match item {
            Said::HandBack(words) => {
                rt.watch.hand_back(cx.looking);
                tasks.push(announce(rt, cx, words));
            }
            Said::Announce(words) => tasks.push(announce(rt, cx, words)),
            Said::Session(id) => {
                let (ws, pane) = (rt.ws.clone(), rt.link.id.clone());
                m.apply(StoreAction::UpdatePane {
                    ws,
                    pane,
                    patch: Patch::new(move |pane: &mut Pane| pane.session_id = Some(id.clone())),
                });
            }
        }
    }
    Task::batch(tasks)
}

fn words(tr: Translator, words: Words) -> String {
    match words {
        Words::Text(text) => text,
        Words::Settled => tr.t(Key::NotifySettled).into_owned(),
        Words::Aborted => tr.t(Key::NotifyAborted).into_owned(),
        Words::Permission(tool) => tr
            .t_with(Key::NotifyPermission, &args!("tool" => tool))
            .into_owned(),
        Words::Dropped(text) => tr
            .t_with(Key::NotifyDropped, &args!("text" => text))
            .into_owned(),
        Words::TrustFolder => tr.t(Key::NotifyTrustFolder).into_owned(),
    }
}

/// Tells the person, unless they are already on the pane: the outline, and a
/// desktop notification when the settings want one (`announce`).
fn announce(rt: &mut PaneRt, cx: &Context, said: Words) -> Task<Message> {
    if cx.looking {
        return Task::none();
    }
    rt.attention = true;
    if !cx.settings.notify_on_idle || (cx.settings.notify_only_when_away && cx.window_focused) {
        return Task::none();
    }
    let title = cx.tr.t_with(
        Key::NotifyTitle,
        &args!("name" => rt.name.as_str(), "agent" => rt.agent.info().label),
    );
    Task::done(Message::Intent(Intent::Notify(Notification {
        title: title.into_owned(),
        body: words(cx.tr, said),
        target: Some((rt.ws.clone(), rt.link.id.clone())),
    })))
}

/// The pane printed: the watcher counts it, and the screen is read again a
/// moment after the last output for the trust question.
pub fn output(rt: &mut PaneRt, now: Instant) {
    rt.link.take_wake();
    rt.watcher.push(now);
    if !rt.watch.bridged && rt.agent == AgentId::Claude {
        rt.trust_due = Some(now + TRUST_LOOK);
    }
}

/// The shell ended by itself.
pub fn exited(rt: &mut PaneRt, sv: &Services) {
    rt.status = PaneStatus::Exited;
    rt.watch.exited();
    rt.launch = None;
    // The orchestration engine listened to every pane's exit (inventory
    // §1.9, `pty:exit`).
    sv.send(Message::Core(CoreEvent::PtyExit {
        pane: rt.link.id.clone(),
    }));
}

/// The trust look that is due: the visible rows, read as a person would.
pub fn trust_look(rt: &mut PaneRt) -> Option<Said> {
    rt.trust_due = None;
    let screen = rt.link.lock().screen_text().join("\n");
    rt.watch.trust(launch::is_trust_prompt(&screen))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(id: &str) -> ShellInfo {
        ShellInfo {
            id: id.into(),
            label: id.into(),
            program: format!("/bin/{id}"),
            args: vec![],
        }
    }

    #[test]
    fn the_shell_follows_the_pane_then_the_workspace_then_the_settings() {
        let shells = vec![shell("bash"), shell("zsh"), shell("fish")];
        let pick = |preferred| resolve_shell(&shells, preferred).map(|s| s.id.as_str());
        assert_eq!(pick([Some("fish"), Some("zsh"), None]), Some("fish"));
        assert_eq!(pick([Some("gone"), Some("zsh"), None]), Some("zsh"));
        assert_eq!(pick([None, None, Some("fish")]), Some("fish"));
        assert_eq!(pick([None, None, None]), Some("bash"));
        assert_eq!(
            resolve_shell(&[], [None, None, None]).map(|s| s.id.as_str()),
            None
        );
    }

    #[test]
    fn a_pane_gets_its_workspace_kits_then_the_ones_everywhere() {
        use crate::state::types::WorkspaceKit;
        let kit = |id: &str| WorkspaceKit {
            id: id.into(),
            granted: vec![],
        };
        let settings = Settings {
            kits_everywhere: Some(vec![kit("b"), kit("c")]),
            ..Settings::default()
        };
        let ws = Workspace {
            kits: Some(vec![kit("a"), kit("b")]),
            ..Workspace::default()
        };
        assert_eq!(kits_of(&settings, &ws), ["a", "b", "c"]);
    }

    #[test]
    fn the_account_is_the_panes_then_the_workspaces_then_the_settings() {
        let settings = Settings::default();
        let ws = Workspace {
            claude_account: Some("work".into()),
            ..Workspace::default()
        };
        let pane = Pane::default();
        assert_eq!(account_of(&settings, &ws, &pane), "work");
        let own = Pane {
            claude_account: Some("perso".into()),
            ..Pane::default()
        };
        assert_eq!(account_of(&settings, &ws, &own), "perso");
        assert_eq!(
            account_of(
                &Settings {
                    claude_account: None,
                    ..settings
                },
                &Workspace::default(),
                &pane
            ),
            "main"
        );
    }

    #[test]
    fn kit_notes_say_each_kit_left_out_and_the_forbidden_ones_together() {
        let tr = Translator::new(hangar_i18n::Locale::En);
        let notes = kit_notes(
            tr,
            &[
                ("a".into(), IssueReason::Missing, None),
                ("b".into(), IssueReason::Forbidden, None),
                ("c".into(), IssueReason::Forbidden, None),
            ],
        );
        assert_eq!(notes.len(), 2);
        assert!(notes[0].contains("“a”"));
        assert!(notes[1].contains("b, c"));
    }
}
