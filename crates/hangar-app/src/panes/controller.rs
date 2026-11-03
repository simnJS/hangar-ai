//! A pane's life, from its id appearing in a workspace to its going: the
//! process half of TerminalPane.tsx (inventory §4/1.5, "Mount sequence",
//! "Unmount", and §4/3, "PaneController").
//!
//! A pane's identity is its id. A restart, a change of agent or of shell
//! gives the pane a new id upstream (`respawnPane`), which unmounts the old
//! one (its shell killed) and mounts the new one (a new shell), as the React
//! effect keyed on `pane.id` did. Mounts and unmounts follow the model:
//! whatever changes a workspace's panes, the next sync starts what appeared
//! and stops what went (mod.rs, `sync`).
//!
//! Where the agent stands (its activity, the mod's reports, hand-backs and
//! their notifications, the queue, the session to resume) is P4's
//! (`panes_logic`); this file tells it what happens to the pane, in the
//! order its contract asks: `Mounted` before the PTY spawns, `Status` once
//! the shell runs or could not start, `WatchSession` once the agent is
//! launched, `Unmounted` when the terminal goes.

use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use hangar_core::kits::{KitInfo, LaunchIssue};
use hangar_i18n::{Key, Locale, Translator, args};
use hangar_term::Terminal;
use iced::Task;

use crate::kits::model::{
    KitLaunchIssue, LaunchReason, PaneKits, kit_issue_notes, load_pane_kits_with,
};
use crate::mcp::scope::{ShellFamily, mcp_config_arg, shell_family, workspace_mcp};
use crate::message::{Intent, Message};
use crate::orchestration_ui::actions::{
    RestartText, RoleRestart, RoleRestartInput, is_mid_turn, queue_restart_note, restart_text,
    role_restart, take_restart_note,
};
use crate::panes_logic::Msg as Logic;
use crate::panes_logic::agents::{LaunchOptions, is_resumable, known_session_ids, launch_command};
use crate::panes_logic::shells::resolve_shell;
use crate::services::{CoreEvent, Services};
use crate::state::live::{PaneMcp, PaneMcpMode, PaneStatus};
use crate::state::roles::{ORCHESTRATION_ALLOWED_TOOLS, orchestration_of};
use crate::state::types::{AgentId, McpMode, Pane, PaneRole, Settings, ShellInfo, Workspace};
use crate::state::{Model, PaneId, Patch, StoreAction, WsId};

use super::Msg;
use super::hub::{Hub, HubChannel};
use super::look::{self, Look};
use super::pty_link::PaneLink;
use super::search::Search;

/// How long a pane waits for its terminal to measure itself before its
/// shell starts at the default 80×24: a pane of a workspace off screen is
/// never measured, as a hidden xterm.js was never fitted.
pub const MEASURE_WAIT: Duration = Duration::from_millis(250);

/// A pane, while it is mounted.
pub struct PaneRt {
    pub ws: WsId,
    pub name: String,
    pub agent: AgentId,
    pub link: Arc<PaneLink>,
    /// The shell's life as this package lives it; P4's copy is what the
    /// glyphs read.
    pub status: PaneStatus,
    /// What the launch had to say: kept out of the terminal, which Claude
    /// Code clears as it starts.
    pub notes: Vec<String>,
    pub dismissed: HashSet<String>,
    pub search: Option<Search>,
    /// What the search bar held when it last closed.
    pub last_query: String,
    /// The account its Claude Code runs on, as core started it.
    pub account: Option<String>,
    /// The MCP servers its Claude Code started with.
    pub mcp: Option<PaneMcp>,
    /// The role the pane was launched with: a role then meant Hangar's
    /// server came along.
    pub launched_role: Option<PaneRole>,
    /// The restart a role asked for was dealt with, once per pin.
    pub role_handled: bool,
    /// The spawn, while the terminal has not measured itself yet.
    pending: Option<Spawn>,
    /// The resume the launch is waiting to type, and whether the hibernation
    /// owed it.
    resume: Option<String>,
    owed: bool,
    /// The `--mcp-config` argument the launch types, and whether it replaces
    /// Claude Code's own servers.
    mcp_arg: Option<(String, bool)>,
    /// The look last applied, to restyle only on a change.
    pub look: Look,
    /// Whether the terminal was last told it has the keyboard.
    pub focused: bool,
    /// The launch's sleep, aborted with the pane.
    launch: Option<iced::task::Handle>,
}

impl std::fmt::Debug for PaneRt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaneRt")
            .field("ws", &self.ws)
            .field("name", &self.name)
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

/// What `pty_spawn` answered, and what the launch learnt on the way.
#[derive(Debug, Clone)]
pub struct SpawnReport {
    /// Why each kit the pane was given stays out of it: the ones held for
    /// what they run by themselves first, then those `pty_spawn` kept out.
    pub kit_issues: Vec<KitLaunchIssue>,
    /// The account the pane was launched on, as P8's choice resolved it.
    pub resolved: Option<String>,
    /// The account that was asked for and is gone: the fallback applies.
    pub missing: Option<String>,
    /// The account core started its Claude Code on (main when it fell back).
    pub account: Option<String>,
    pub mcp: Option<PaneMcp>,
    pub mcp_arg: Option<(String, bool)>,
    /// The MCP scope's warnings, already worded.
    pub notes: Vec<String>,
}

/// The shell a pane runs: its own, its workspace's, the settings', then the
/// first detected (`resolveShell`).
pub fn shell_of<'a>(m: &'a Model, ws: &Workspace, pane: &Pane) -> Option<&'a ShellInfo> {
    resolve_shell(
        &m.ui.shells,
        &[
            pane.shell_id.as_deref(),
            ws.shell_id.as_deref(),
            m.settings().shell_id.as_deref(),
        ],
    )
}

/// The folder a pane's shell opens in. P4 has the same rule
/// (`panes_logic::folder_of`), private to its module: this copy goes once P4
/// makes it public, which is a change in P4's files, not in this package's.
pub fn folder_of(ws: &Workspace, pane: &Pane) -> String {
    pane.cwd
        .clone()
        .filter(|cwd| !cwd.is_empty())
        .unwrap_or_else(|| ws.cwd.clone())
}

/// What a pane's kits are decided from: its workspace's entries and the ones
/// turned on everywhere, with their grants, as the model held them when the
/// pane mounted. Only these two lists are kept, not the whole workspace and
/// settings.
#[derive(Debug, Clone, Default)]
struct KitChoice {
    ws: Workspace,
    settings: Settings,
}

impl KitChoice {
    fn of(settings: &Settings, ws: &Workspace) -> KitChoice {
        KitChoice {
            ws: Workspace {
                kits: ws.kits.clone(),
                ..Workspace::default()
            },
            settings: Settings {
                kits_everywhere: settings.kits_everywhere.clone(),
                ..Settings::default()
            },
        }
    }
}

/// The kits a pane starts with, and the ones held out (`loadPaneKits`), from
/// `list`, the kits on disk as they are at the spawn. A GitHub kit whose
/// hooks (or anything else it runs by itself) changed since the workspace
/// accepted them stays out of the pane until they are accepted again: core's
/// `pty_spawn` loads every id it is handed, so this is where the consent is
/// held. What else keeps a kit out (missing, broken, forbidden by a policy)
/// core says at spawn. `None`, a login pane, starts with no kit at all.
async fn launch_kits<F, Fut>(choice: Option<&KitChoice>, list: F) -> PaneKits
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Vec<KitInfo>, String>>,
{
    match choice {
        Some(choice) => load_pane_kits_with(Some(&choice.ws), Some(&choice.settings), list).await,
        None => PaneKits::default(),
    }
}

/// Why each kit a pane was given stays out of it, in the order its notes are
/// shown (`[...held, ...kitIssues]`): those held for what they run by
/// themselves, then what `pty_spawn` kept out.
fn kit_issues(held: &[String], spawned: &[LaunchIssue]) -> Vec<KitLaunchIssue> {
    held.iter()
        .map(|id| KitLaunchIssue {
            id: id.clone(),
            reason: LaunchReason::Hooks,
            detail: None,
        })
        .chain(spawned.iter().map(KitLaunchIssue::from))
        .collect()
}

/// `--permission-mode` and `--allowedTools` of a launch: for an orchestrator
/// and its workers, which work while nobody watches, the mode the workspace
/// chose for them, read as `orchestrationOf` reads it (a mode that is not
/// offered, edited in by hand, is the person's own default), and Hangar's
/// own tools; nothing for any other pane.
fn role_flags(ws: &Workspace, pane: &Pane) -> (Option<String>, Vec<String>) {
    if pane.role.is_none() {
        return (None, Vec::new());
    }
    let tools = ORCHESTRATION_ALLOWED_TOOLS
        .iter()
        .map(|tool| (*tool).to_owned())
        .collect();
    (orchestration_of(ws).permission_mode, tools)
}

/// The Claude account choices of a launch, the settings', the workspace's
/// and the pane's, which P8's `launch_account` resolves (pane, workspace,
/// default, then main) off the UI thread: it reads the accounts' files.
#[derive(Debug, Clone, Default)]
struct AccountChoice {
    default: Option<String>,
    workspace: Option<String>,
    pane: Option<String>,
}

/// What `pty_spawn` is called with, gathered on the UI thread.
struct Spawn {
    id: PaneId,
    workspace_id: WsId,
    workspace_cwd: String,
    cwd: String,
    shell: Option<ShellInfo>,
    name: String,
    bridge: bool,
    port_base: Option<u16>,
    /// `None` for a login pane, which starts with no kit.
    kits: Option<KitChoice>,
    agent: AgentId,
    /// A Claude Code pane's: the account it is launched on.
    account: Option<AccountChoice>,
    /// The workspace's MCP choice applies (a Claude Code that is not a login
    /// pane).
    mcp: Option<McpLaunch>,
}

/// What the MCP scope of a launch is made from (`mcpLaunchConfig`).
struct McpLaunch {
    mode: McpMode,
    enabled: Vec<String>,
    /// An orchestrator or a worker gets Hangar's server even in `inherit`:
    /// the board, `worker_*` and `delivery_submit` are its work.
    role: bool,
    family: Option<ShellFamily>,
    tr: Translator,
}

/// Mounts `pane`: its terminal and its writer. The shell starts once the
/// terminal has measured itself (`MEASURE_WAIT` at most), so that the agent
/// draws its first screen at the size it will be seen at.
pub fn mount(
    hub: &Hub,
    sv: &Services,
    m: &Model,
    ws: &Workspace,
    pane: &Pane,
) -> (PaneRt, Task<Message>) {
    let settings = m.settings();
    let look = look::look(settings, ws, pane.agent);
    let mut terminal = Terminal::new(look.config.clone(), 80, 24);
    terminal.set_palette(look.palette.clone());
    terminal.set_font(look.font.clone());
    let link = PaneLink::new(pane.id.clone(), terminal, sv.core().pty().clone());
    hub.insert(link.clone());

    let login = pane.agent == AgentId::Claude && pane.login == Some(true);
    let account = (pane.agent == AgentId::Claude).then(|| AccountChoice {
        default: settings.claude_account.clone(),
        workspace: ws.claude_account.clone(),
        pane: pane.claude_account.clone(),
    });
    let shell = shell_of(m, ws, pane).cloned();
    let scope = workspace_mcp(Some(ws));
    let spawn = Spawn {
        id: pane.id.clone(),
        workspace_id: ws.id.clone(),
        workspace_cwd: ws.cwd.clone(),
        cwd: folder_of(ws, pane),
        shell: shell.clone(),
        name: pane.name.clone(),
        bridge: settings.claude_bridge,
        port_base: pane.port_base.and_then(|port| u16::try_from(port).ok()),
        // A pane opened to log its Claude account in runs the login and
        // nothing else: no kits, no servers, no conversation. The TS said so
        // too, but still handed such a pane the kits turned on everywhere
        // (`loadPaneKits(null, settings)`); the comment is followed here.
        // Every other pane gets the kits, a shell pane too: a `claude` typed
        // into it loads them, like the bridge.
        kits: (!login).then(|| KitChoice::of(settings, ws)),
        agent: pane.agent,
        account,
        mcp: (pane.agent == AgentId::Claude && !login).then(|| McpLaunch {
            mode: scope.mode,
            enabled: scope.enabled,
            role: pane.role.is_some(),
            family: shell_family(shell.as_ref()),
            tr: Translator::new(m.locale()),
        }),
    };

    let id = pane.id.clone();
    // P4 hears of the pane before its shell exists, so no report of the mod
    // is lost; the queue a restart carried over goes back to core there.
    let mounted = Task::done(Message::PanesLogic(Logic::Mounted { pane: id.clone() }));
    let due = Task::perform(tokio::time::sleep(MEASURE_WAIT), move |()| {
        Message::Panes(Msg::SpawnDue(id.clone()))
    });

    let rt = PaneRt {
        ws: ws.id.clone(),
        name: pane.name.clone(),
        agent: pane.agent,
        link,
        status: PaneStatus::Starting,
        notes: Vec::new(),
        dismissed: HashSet::new(),
        search: None,
        last_query: String::new(),
        account: None,
        mcp: None,
        launched_role: pane.role,
        role_handled: false,
        pending: Some(spawn),
        resume: None,
        owed: false,
        mcp_arg: None,
        look,
        focused: false,
        launch: None,
    };
    (rt, Task::batch([mounted, due]))
}

/// Starts the shell, if it has not started: on the terminal's first
/// measure, or once `MEASURE_WAIT` is over. The kits on disk are read first,
/// to hold out those whose hooks were not accepted; the spawn then runs on
/// the blocking pool, since it waits up to 5 s for a shell of the same id
/// still being reaped, and the MCP scope file is written before it.
pub fn start(rt: &mut PaneRt, sv: &Services, channel: &HubChannel) -> Task<Message> {
    let Some(spawn) = rt.pending.take() else {
        return Task::none();
    };
    let (columns, rows) = rt.link.lock().size();
    let sink = rt.link.sink(channel.sender());
    let pty = sv.core().pty().clone();
    let host = sv.core().host().clone();
    let id = spawn.id.clone();
    Task::perform(
        async move {
            let kits = launch_kits(spawn.kits.as_ref(), hangar_core::kits::kits_list).await;
            tokio::task::spawn_blocking(move || spawn.run(&host, &pty, sink, columns, rows, kits))
                .await
                .unwrap_or_else(|error| Err(format!("the spawn stopped: {error}")))
        },
        move |result| Message::Panes(Msg::Spawned(id.clone(), result)),
    )
}

impl Spawn {
    /// `kits` is what [`launch_kits`] found: only its `ids` reach the shell
    /// and the MCP scope, and its `held` become notes.
    fn run(
        self,
        host: &hangar_core::Host,
        pty: &hangar_core::pty::PtyManager,
        sink: hangar_core::pty::PtySink,
        columns: u16,
        rows: u16,
        kits: PaneKits,
    ) -> Result<SpawnReport, String> {
        let mut notes = Vec::new();
        let resolved = self.account.as_ref().map(|choice| {
            crate::accounts::launch_account(
                choice.default.as_deref(),
                choice.workspace.as_deref(),
                choice.pane.as_deref(),
            )
        });
        let account = resolved.as_ref().map(|resolved| resolved.id.clone());
        let missing = resolved.and_then(|resolved| resolved.missing);
        let (mcp, scope) = match &self.mcp {
            Some(launch) => self.mcp_scope(host, launch, &kits.ids, account.as_deref(), &mut notes),
            None => (None, None),
        };
        let spawned = hangar_core::pty::pty_spawn(
            pty,
            sink,
            self.id,
            self.cwd,
            self.shell,
            columns,
            rows,
            Some(self.name),
            Some(self.bridge),
            self.port_base,
            // The shell is handed the file's path (`HANGAR_MCP_CONFIG`); the
            // typed command names the variable, never the path, which holds
            // the person's name.
            scope.as_ref().map(|(path, _, _)| path.clone()),
            Some(kits.ids),
            Some(self.agent.as_str().to_owned()),
            account.clone(),
        )?;
        Ok(SpawnReport {
            kit_issues: kit_issues(&kits.held, &spawned.kit_issues),
            resolved: account,
            missing,
            account: spawned.account,
            mcp,
            mcp_arg: scope.map(|(_, arg, strict)| (arg, strict)),
            notes,
        })
    }

    /// The `--mcp-config` file of this launch, when the workspace chose its
    /// servers or the pane has a role, and the argument naming it
    /// (`mcpLaunchConfig`). Written again at each launch, from Claude Code's
    /// configuration as it stands. Whatever goes wrong (a WSL pane, a path
    /// the shell cannot be handed, a failed write), the agent still starts
    /// with its own servers, and the pane says so.
    fn mcp_scope(
        &self,
        host: &hangar_core::Host,
        launch: &McpLaunch,
        kits: &[String],
        account: Option<&str>,
        notes: &mut Vec<String>,
    ) -> (Option<PaneMcp>, Option<(String, String, bool)>) {
        let tr = launch.tr;
        let inherit = |hangar| PaneMcp {
            mode: PaneMcpMode::Inherit,
            hangar: Some(hangar),
        };
        let failed = |error: String| {
            tr.t_with(
                Key::McpScopeWarnFailed,
                &args!("error" => error.trim_start_matches("Error: ")),
            )
            .into_owned()
        };
        let write = |enabled: Vec<String>, kits: Option<Vec<String>>| {
            hangar_core::mcp_scope::mcp_scope_write(
                host,
                self.workspace_id.clone(),
                self.workspace_cwd.clone(),
                self.cwd.clone(),
                enabled,
                kits,
                account.map(str::to_owned),
            )
        };
        if launch.mode == McpMode::Inherit {
            if !launch.role || launch.family == Some(ShellFamily::Wsl) {
                return (Some(inherit(false)), None);
            }
            return match write(Vec::new(), None) {
                Ok(file) => match mcp_config_arg(&file.path, launch.family) {
                    Some(arg) => (Some(inherit(true)), Some((file.path, arg, false))),
                    None => (Some(inherit(false)), None),
                },
                Err(error) => {
                    notes.push(failed(error));
                    (Some(inherit(false)), None)
                }
            };
        }
        if launch.family == Some(ShellFamily::Wsl) {
            notes.push(tr.t(Key::McpScopeWarnWsl).into_owned());
            return (Some(inherit(false)), None);
        }
        // The servers of the kits the pane starts with, never of one held
        // out: its servers are among what it runs by itself.
        let file = match write(launch.enabled.clone(), Some(kits.to_vec())) {
            Ok(file) => file,
            Err(error) => {
                notes.push(failed(error));
                return (Some(inherit(false)), None);
            }
        };
        let Some(arg) = mcp_config_arg(&file.path, launch.family) else {
            notes.push(failed(tr.t(Key::McpScopeWarnQuote).into_owned()));
            return (Some(inherit(false)), None);
        };
        if !file.missing.is_empty() {
            notes.push(
                tr.t_with(
                    Key::McpScopeWarnMissing,
                    &args!("names" => file.missing.join(", ")),
                )
                .into_owned(),
            );
        }
        let mcp = PaneMcp {
            mode: PaneMcpMode::Custom {
                servers: file.servers.clone(),
            },
            hangar: Some(true),
        };
        (Some(mcp), Some((file.path, arg, true)))
    }
}

fn status(pane: &PaneId, status: PaneStatus) -> Task<Message> {
    Task::done(Message::PanesLogic(Logic::Status {
        pane: pane.clone(),
        status,
    }))
}

/// The spawn answered.
pub fn spawned(
    rt: &mut PaneRt,
    m: &mut Model,
    result: Result<SpawnReport, String>,
) -> Task<Message> {
    let tr = Translator::new(m.locale());
    let id = rt.link.id.clone();
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
            return status(&id, PaneStatus::Exited);
        }
    };
    rt.status = PaneStatus::Running;
    // The terminal may have been measured again while the shell started.
    let (columns, rows) = rt.link.lock().size();
    rt.link.resize(columns, rows);
    rt.mcp = report.mcp.clone();
    rt.mcp_arg = report.mcp_arg.clone();

    let Some((ws, pane)) = find(m, &rt.ws, &id) else {
        return status(&id, PaneStatus::Running);
    };
    let (ws, pane) = (ws.clone(), pane.clone());

    // In the pane's bar, not its terminal: Claude Code clears the screen as
    // it starts and the notes would go with it. A restart that gave the
    // pane Hangar's tools says so on the way in.
    let mut notes: Vec<String> = take_restart_note(pane.session_id.as_deref())
        .into_iter()
        .collect();
    // The account a pane fell back to: the one asked for is gone, or core
    // could not start Claude Code on it.
    let fell_back =
        report.missing.is_some() || (report.account.is_some() && report.account != report.resolved);
    if rt.agent == AgentId::Claude {
        if let Some(missing) = &report.missing {
            notes.push(
                tr.t_with(Key::AccountsPickerMissing, &args!("id" => missing.as_str()))
                    .into_owned(),
            );
        }
        notes.extend(report.notes.iter().cloned());
        // Said where Claude Code runs; a shell pane has no use for it.
        notes.extend(kit_issue_notes(&report.kit_issues, tr));
        // What the pane's usage counts against until it restarts, whatever
        // is chosen meanwhile (P8's `Launched`).
        let account = report
            .account
            .clone()
            .or(report.resolved.clone())
            .unwrap_or_else(|| crate::accounts::MAIN_ACCOUNT.to_owned());
        m.live.accounts.launched.note(&id, Some(&account));
        rt.account = Some(account);
    }
    rt.notes = notes;
    let running = status(&id, PaneStatus::Running);
    if rt.agent == AgentId::Shell {
        return running;
    }

    let delay = Duration::from_millis(m.settings().launch_delay_ms);
    let auto_resume = m.settings().auto_resume;

    // The person logs in in the browser; nothing to resume or follow. Never
    // on an account the pane fell back to: logging in to an account that is
    // gone would log main in again in its place.
    if pane.login == Some(true) && rt.agent == AgentId::Claude {
        if fell_back {
            return running;
        }
        let (task, handle) = Task::perform(tokio::time::sleep(delay), move |()| {
            Message::Panes(Msg::LoginReady(id.clone()))
        })
        .abortable();
        rt.launch = Some(handle.abort_on_drop());
        return Task::batch([running, task]);
    }

    // A pane its workspace's sleep stopped resumes whatever the setting
    // says: a sleep stops an agent to free memory, and must not cost a
    // conversation.
    let owed = m.live.panes_logic.must_resume(&id, false);
    let saved = (auto_resume || owed)
        .then(|| pane.session_id.clone())
        .flatten()
        .filter(|_| is_resumable(rt.agent));
    rt.resume = saved.clone();
    rt.owed = owed;
    let agent = rt.agent;
    let cwd = folder_of(&ws, &pane);
    // Claude Code names a session the moment it starts but writes its
    // transcript only on the first message, so a pane reset and left alone
    // holds an id with nothing behind it, and resuming that only prints "No
    // conversation found". Asked while the shell settles; the transcripts
    // known before the launch are read then too, so a brand new one stands
    // out to the session hunt.
    let (task, handle) = Task::perform(
        async move {
            let exists = async {
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
            let known = known_session_ids(agent, cwd);
            let (exists, known, ()) =
                iced::futures::join!(exists, known, tokio::time::sleep(delay));
            (exists, known)
        },
        move |(exists, known)| Message::Panes(Msg::LaunchReady(id.clone(), exists, known)),
    )
    .abortable();
    rt.launch = Some(handle.abort_on_drop());
    Task::batch([running, task])
}

/// The shell has settled: type the agent's command, and start following its
/// session.
pub fn launch_ready(
    rt: &mut PaneRt,
    m: &mut Model,
    exists: Option<bool>,
    known: HashSet<String>,
) -> Task<Message> {
    rt.launch = None;
    let id = rt.link.id.clone();
    let Some((ws, pane)) = find(m, &rt.ws, &id) else {
        return Task::none();
    };
    let (ws, pane) = (ws.clone(), pane.clone());
    if rt.owed {
        m.live.panes_logic.must_resume(&id, true);
    }
    let mut resume = rt.resume.take();
    let forget = resume.is_some() && exists == Some(false);
    if forget {
        resume = None;
    }
    let settings = m.settings();
    let (permission_mode, tools) = role_flags(&ws, &pane);
    // Read when the agent starts: a workspace renamed later shows on the
    // next launch.
    let name = crate::state::pane_names::session_name(&pane.name, &ws.name);
    let options = LaunchOptions {
        extra_roots: &ws.extra_roots,
        commands: Some(&settings.agent_commands),
        name: Some(&name),
        mcp_config: rt.mcp_arg.as_ref().map(|(arg, _)| arg.as_str()),
        mcp_strict: rt.mcp_arg.as_ref().map(|(_, strict)| *strict),
        permission_mode: permission_mode.as_deref(),
        allowed_tools: &tools,
        model: pane.model.as_deref(),
    };
    if let Some(command) = launch_command(rt.agent, resume.as_deref(), &options) {
        if let Some(id) = &resume {
            m.live.panes_logic.claim_session(id);
        }
        rt.link.write(format!("{command}\r").into_bytes());
    }
    if forget {
        m.apply(StoreAction::UpdatePane {
            ws: ws.id.clone(),
            pane: id.clone(),
            patch: Patch::new(|pane: &mut Pane| pane.session_id = None),
        });
    }
    // Followed for as long as the pane lives, resumed or not: /new inside
    // the agent opens another transcript, and the pane has to follow it.
    if is_resumable(rt.agent) {
        let mut known = known;
        known.extend(resume);
        return Task::done(Message::PanesLogic(Logic::WatchSession {
            pane: id,
            agent: rt.agent,
            cwd: folder_of(&ws, &pane),
            known,
        }));
    }
    Task::none()
}

/// The login pane's shell has settled.
pub fn login_ready(rt: &mut PaneRt) {
    rt.launch = None;
    rt.link
        .write(format!("{}\r", crate::accounts::LOGIN_COMMAND).into_bytes());
}

/// The workspace and the pane of `pane`, if both still exist.
pub fn find<'a>(m: &'a Model, ws: &str, pane: &str) -> Option<(&'a Workspace, &'a Pane)> {
    let ws = m.workspace(ws)?;
    let pane = ws.panes.iter().find(|p| p.id == pane)?;
    Some((ws, pane))
}

/// Unmounts a pane: its shell killed (off the UI thread: the kill takes
/// core's registry lock, which a blocked write may hold), its launch
/// aborted with it, P4 told.
pub fn unmount(hub: &Hub, sv: &Services, m: &mut Model, rt: PaneRt) -> Task<Message> {
    hub.remove(&rt.link.id);
    let pty = sv.core().pty().clone();
    let id = rt.link.id.clone();
    drop(rt);
    // Closed, restarted under a new id, or put to sleep: not counted against
    // its account any more.
    m.live.accounts.launched.note(&id, None);
    let unmounted = Task::done(Message::PanesLogic(Logic::Unmounted {
        pane: id.clone(),
        handing_over: false,
    }));
    let kill = Task::perform(
        tokio::task::spawn_blocking(move || hangar_core::pty::pty_kill(&pty, id, None)),
        |_| Message::Noop,
    );
    Task::batch([unmounted, kill])
}

/// The shell ended by itself: P4 and the orchestration engine hear of it
/// as core's `pty:exit` (inventory §1.9).
pub fn exited(rt: &mut PaneRt, m: &mut Model, sv: &Services) {
    rt.status = PaneStatus::Exited;
    rt.launch = None;
    m.live.accounts.launched.note(&rt.link.id, None);
    sv.send(Message::Core(CoreEvent::PtyExit {
        pane: rt.link.id.clone(),
    }));
}

/// A Claude Code pinned (or given a role) while it runs, started without
/// Hangar's server, restarts on its own conversation, at once or when its
/// turn ends, so the role comes with the tools (`roleRestart`, P10's).
pub fn role_check(rt: &mut PaneRt, m: &Model, locale: Locale) -> Task<Message> {
    let id = rt.link.id.clone();
    let Some((ws, pane)) = find(m, &rt.ws, &id) else {
        return Task::none();
    };
    if pane.role.is_none() {
        rt.role_handled = false;
    }
    if rt.status != PaneStatus::Running || rt.role_handled {
        return Task::none();
    }
    let activity = m.live.panes_logic.activity_or_idle(&id);
    let verdict = role_restart(&RoleRestartInput {
        agent: pane.agent,
        role: pane.role,
        launched_role: rt.launched_role,
        hangar: rt.mcp.as_ref().and_then(|mcp| mcp.hangar),
        mid_turn: is_mid_turn(activity),
        session_id: pane.session_id.clone(),
    });
    let say = |rt: &PaneRt, which| {
        let line = format!("\r\n\x1b[33m{}\x1b[0m\r\n", restart_text(locale, which));
        rt.link.lock().feed(line.as_bytes());
    };
    let session = pane.session_id.clone();
    match (verdict, session) {
        (RoleRestart::Skip | RoleRestart::Wait, _) => Task::none(),
        (RoleRestart::Unresumable, _) | (RoleRestart::Now, None) => {
            rt.role_handled = true;
            say(rt, RestartText::Unresumable);
            Task::none()
        }
        (RoleRestart::Now, Some(session)) => {
            rt.role_handled = true;
            say(rt, RestartText::Restarting);
            // The restart gives the pane a new id: the line follows to its
            // terminal.
            queue_restart_note(&session, restart_text(locale, RestartText::Restarting));
            let ws = ws.id.clone();
            let agent = pane.agent;
            Task::perform(
                tokio::task::spawn_blocking(move || {
                    let found = hangar_core::sessions::session_exists(
                        agent.as_str().to_owned(),
                        session.clone(),
                    );
                    (found == Some(true), session)
                }),
                move |answer| {
                    let (found, session) = answer.unwrap_or((false, String::new()));
                    Message::Panes(Msg::RoleRestart(ws.clone(), id.clone(), session, found))
                },
            )
        }
    }
}

/// The session the restart for a role was to resume exists, or not.
pub fn role_restart_answered(
    rt: &PaneRt,
    ws: WsId,
    session: String,
    found: bool,
    locale: Locale,
) -> Task<Message> {
    if found {
        return Task::done(Message::Intent(Intent::ReplacePane {
            ws,
            pane: rt.link.id.clone(),
            patch: Patch::none(),
        }));
    }
    take_restart_note(Some(&session));
    let line = format!(
        "\r\n\x1b[33m{}\x1b[0m\r\n",
        restart_text(locale, RestartText::Failed)
    );
    rt.link.lock().feed(line.as_bytes());
    Task::none()
}

#[cfg(test)]
mod tests {
    use hangar_core::kits::{GitSource, IssueReason, Source};

    use super::*;
    use crate::panes_logic::agents::launch_command;
    use crate::state::types::{AppState, OrchestrationSettings, WorkspaceKit};

    /// What a workspace that accepted a kit's hooks holds for it.
    const HOOKS: &str = "hooks:0123456789abcdef";

    fn block_on<T>(future: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime")
            .block_on(future)
    }

    fn entry(id: &str, granted: &[&str]) -> WorkspaceKit {
        WorkspaceKit {
            id: id.into(),
            granted: granted.iter().map(|g| (*g).to_owned()).collect(),
        }
    }

    /// A kit installed from GitHub; with `hooks`, one whose plugin runs
    /// something by itself that a workspace has to accept.
    fn git_kit(id: &str, hooks: Option<&str>) -> KitInfo {
        KitInfo {
            id: id.into(),
            source: Source::Git(GitSource {
                url: format!("https://github.com/someone/{id}"),
                git_ref: None,
                subdir: None,
            }),
            dir: format!("/kits/{id}"),
            installed_version: "1.0.0".into(),
            added_at: 0,
            manifest: None,
            error: None,
            update: None,
            hooks_permission: hooks.map(str::to_owned),
            hooks_consent: hooks.is_some(),
            commands_permission: None,
        }
    }

    fn choice(own: Vec<WorkspaceKit>, everywhere: Vec<WorkspaceKit>) -> KitChoice {
        let settings = Settings {
            kits_everywhere: Some(everywhere),
            ..Settings::default()
        };
        let ws = Workspace {
            kits: Some(own),
            ..Workspace::default()
        };
        KitChoice::of(&settings, &ws)
    }

    /// A mounted pane without its shell, for what the launch does once the
    /// spawn answers.
    fn mounted(ws: &Workspace, pane: &Pane) -> PaneRt {
        let link = PaneLink::new(
            pane.id.clone(),
            Terminal::new(hangar_term::Config::default(), 80, 24),
            Arc::new(hangar_core::pty::PtyManager::default()),
        );
        PaneRt {
            ws: ws.id.clone(),
            name: pane.name.clone(),
            agent: pane.agent,
            link,
            status: PaneStatus::Starting,
            notes: Vec::new(),
            dismissed: HashSet::new(),
            search: None,
            last_query: String::new(),
            account: None,
            mcp: None,
            launched_role: pane.role,
            role_handled: false,
            pending: None,
            resume: None,
            owed: false,
            mcp_arg: None,
            look: look::look(&Settings::default(), ws, pane.agent),
            focused: false,
            launch: None,
        }
    }

    fn report(kit_issues: Vec<KitLaunchIssue>) -> SpawnReport {
        SpawnReport {
            kit_issues,
            resolved: None,
            missing: None,
            account: None,
            mcp: None,
            mcp_arg: None,
            notes: Vec::new(),
        }
    }

    #[test]
    fn a_pane_gets_its_workspace_kits_then_the_ones_everywhere() {
        let kits = block_on(launch_kits(
            Some(&choice(
                vec![entry("alpha", &[]), entry("beta", &[])],
                vec![entry("beta", &[]), entry("gamma", &[])],
            )),
            || async { Ok(vec![git_kit("alpha", None), git_kit("gamma", None)]) },
        ));
        // "beta" is not installed: core says so at spawn, so it goes on.
        assert_eq!(kits.ids, ["alpha", "beta", "gamma"]);
        assert!(kits.held.is_empty());
    }

    #[test]
    fn a_git_kit_with_hooks_not_accepted_stays_out_of_the_pane_and_says_so() {
        // "studio" changed what it runs by itself since this workspace
        // enabled it; "quiet" runs nothing; "trusted" was accepted as it is.
        let kits = block_on(launch_kits(
            Some(&choice(
                vec![
                    entry("studio", &[]),
                    entry("quiet", &[]),
                    entry("trusted", &[HOOKS]),
                ],
                vec![],
            )),
            || async {
                Ok(vec![
                    git_kit("studio", Some(HOOKS)),
                    git_kit("quiet", None),
                    git_kit("trusted", Some(HOOKS)),
                ])
            },
        ));
        assert_eq!(kits.ids, ["quiet", "trusted"], "only these reach the spawn");
        assert_eq!(kits.held, ["studio"]);

        // The pane's note bar says so once its shell runs.
        let pane = Pane {
            id: "p1".into(),
            name: "Ava".into(),
            agent: AgentId::Claude,
            ..Pane::default()
        };
        let ws = Workspace {
            id: "w1".into(),
            name: "hangar".into(),
            cwd: "/work/hangar".into(),
            panes: vec![pane.clone()],
            kits: Some(vec![entry("studio", &[])]),
            ..Workspace::default()
        };
        let mut m = Model::default();
        m.hydrate(AppState {
            workspaces: vec![ws.clone()],
            ..AppState::default()
        });
        let mut rt = mounted(&ws, &pane);
        let _ = spawned(&mut rt, &mut m, Ok(report(kit_issues(&kits.held, &[]))));
        let warning = Translator::new(Locale::En)
            .t_with(Key::KitsManageWarnHooks, &args!("id" => "studio"))
            .into_owned();
        assert!(warning.contains("studio"), "{warning}");
        assert_eq!(rt.notes, [warning]);
    }

    #[test]
    fn the_kits_on_everywhere_are_held_the_same_way() {
        let kits = block_on(launch_kits(
            Some(&choice(vec![], vec![entry("studio", &[])])),
            || async { Ok(vec![git_kit("studio", Some(HOOKS))]) },
        ));
        assert!(kits.ids.is_empty());
        assert_eq!(kits.held, ["studio"]);
    }

    #[test]
    fn a_login_pane_starts_with_no_kit_and_reads_none() {
        let asked = std::cell::Cell::new(false);
        let kits = block_on(launch_kits(None, || {
            asked.set(true);
            async { Ok(Vec::new()) }
        }));
        assert_eq!(kits, PaneKits::default());
        assert!(!asked.get());
    }

    #[test]
    fn kit_notes_put_the_held_kits_first_and_the_forbidden_ones_together() {
        let issues = kit_issues(
            &["studio".to_owned()],
            &[
                LaunchIssue::new("alpha", IssueReason::Missing, None),
                LaunchIssue::new("beta", IssueReason::Forbidden, None),
                LaunchIssue::new("gamma", IssueReason::Forbidden, None),
            ],
        );
        assert_eq!(issues[0].reason, LaunchReason::Hooks);
        let tr = Translator::new(Locale::Fr);
        assert_eq!(
            kit_issue_notes(&issues, tr),
            [
                tr.t_with(Key::KitsManageWarnHooks, &args!("id" => "studio")),
                tr.t_with(Key::KitsWarnMissing, &args!("id" => "alpha")),
                tr.t_with(Key::KitsWarnForbidden, &args!("ids" => "beta, gamma")),
            ]
        );
    }

    fn role_ws(mode: Option<&str>) -> Workspace {
        Workspace {
            orchestration: Some(OrchestrationSettings {
                permission_mode: mode.map(str::to_owned),
                ..OrchestrationSettings::default()
            }),
            ..Workspace::default()
        }
    }

    fn with_role(role: Option<PaneRole>) -> Pane {
        Pane {
            agent: AgentId::Claude,
            role,
            ..Pane::default()
        }
    }

    #[test]
    fn an_orchestration_pane_gets_its_workspace_mode_and_hangars_tools() {
        let ws = role_ws(Some("acceptEdits"));
        let (mode, tools) = role_flags(&ws, &with_role(Some(PaneRole::Worker)));
        assert_eq!(mode.as_deref(), Some("acceptEdits"));
        assert_eq!(tools, ["mcp__hangar"]);
        let options = LaunchOptions {
            permission_mode: mode.as_deref(),
            allowed_tools: &tools,
            ..LaunchOptions::default()
        };
        assert_eq!(
            launch_command(AgentId::Claude, None, &options).as_deref(),
            Some("claude --permission-mode acceptEdits --allowedTools mcp__hangar")
        );
    }

    #[test]
    fn a_mode_that_is_not_offered_is_the_persons_own_default() {
        // Typed into state.json by hand: plain letters, which the command
        // line would have taken, and not a mode the workspace can choose.
        let ws = role_ws(Some("dontAsk"));
        let (mode, tools) = role_flags(&ws, &with_role(Some(PaneRole::Orchestrator)));
        assert_eq!(mode, None);
        assert_eq!(tools, ["mcp__hangar"]);
    }

    #[test]
    fn a_pane_without_a_role_gets_neither() {
        let ws = role_ws(Some("bypassPermissions"));
        assert_eq!(role_flags(&ws, &with_role(None)), (None, Vec::new()));
    }

    // The four cases of mcpScope.test.ts, "launchCommand with a server
    // choice": the argument the launch hands the command, quoted for
    // PowerShell as `mcp_config_arg` quotes it.
    fn ps_file() -> String {
        crate::mcp::scope::quote_arg("C:\\data\\mcp\\ws-1.json", Some(ShellFamily::Powershell))
            .unwrap()
    }

    #[test]
    fn a_server_choice_adds_nothing_in_inherit_mode() {
        assert_eq!(
            launch_command(AgentId::Claude, None, &LaunchOptions::default()).as_deref(),
            Some("claude")
        );
    }

    #[test]
    fn a_server_choice_goes_between_the_resume_and_the_directories() {
        let file = ps_file();
        let roots = vec!["D:\\shared".to_string()];
        let options = LaunchOptions {
            name: Some("Ava"),
            mcp_config: Some(&file),
            extra_roots: &roots,
            ..LaunchOptions::default()
        };
        assert_eq!(
            launch_command(AgentId::Claude, Some("abc"), &options),
            Some(format!(
                "claude --resume abc --name Ava --mcp-config {file} --strict-mcp-config --add-dir \"D:\\shared\""
            ))
        );
    }

    #[test]
    fn a_server_choice_keeps_a_custom_launcher_in_front() {
        let file = ps_file();
        let commands = serde_json::json!({"claude": "claude --model haiku"});
        let options = LaunchOptions {
            commands: commands.as_object(),
            mcp_config: Some(&file),
            ..LaunchOptions::default()
        };
        assert_eq!(
            launch_command(AgentId::Claude, None, &options),
            Some(format!(
                "claude --model haiku --mcp-config {file} --strict-mcp-config"
            ))
        );
    }

    #[test]
    fn a_server_choice_leaves_the_other_agents_alone() {
        let file = ps_file();
        let options = LaunchOptions {
            mcp_config: Some(&file),
            ..LaunchOptions::default()
        };
        assert_eq!(
            launch_command(AgentId::Codex, Some("s1"), &options).as_deref(),
            Some("codex resume s1")
        );
        assert_eq!(
            launch_command(AgentId::Gemini, None, &options).as_deref(),
            Some("gemini")
        );
    }
}
