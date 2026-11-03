//! P8: Claude accounts.
//!
//! Ports components/accounts/*, lib/{accounts,accountSwitch}.ts. Read
//! inventory-frontend.md §2.1/13 and crates/hangar-app/PACKAGES.md.
//!
//! Files: `model` (reading a choice, resolving the account of a pane, the
//! accounts panes were launched on), `switch` (restart sets, usage, labels),
//! `usage` (the usage cache on disk), `menu_items` (the lines of the
//! navigator's and the pane's menus), `switcher` (the toolbar button),
//! `badge`, `picker` (the workspace dialog's select) and `settings_page`
//! (Settings › Accounts).
//!
//! # What other packages call
//!
//! - the views: [`switcher`], [`pane_badge`], [`badge`], [`menu_items`],
//!   [`workspace_picker`] and [`settings_page`], with the signatures the shell
//!   fixed;
//! - P5, when it launches a Claude Code pane: [`launch_account`] to know the
//!   account to start it on, then `m.live.accounts.launched.note(pane,
//!   Some(account))`; and `note(pane, None)` when the pane ends. A pane's
//!   usage counts against the account it was launched on, and "running" in
//!   the switcher's question means "has noted an account".

mod badge;
mod menu_items;
mod model;
mod picker;
mod settings_page;
mod switch;
mod switcher;
#[cfg(test)]
mod tests;
mod usage;

use std::time::{SystemTime, UNIX_EPOCH};

use iced::futures::stream;
use iced::{Element, Subscription, Task};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{MenuEntry, Region};
use crate::state::live::BridgeEvent;
use crate::state::{Model, PaneId, Patch, Settings, StoreAction, Workspace, WsId};

#[allow(unused_imports)]
pub use model::{
    AccountId, AccountSource, ClaudeAccount, LOGIN_COMMAND, Launched, MAIN_ACCOUNT,
    ResolvedAccount, launch_account, launch_account_with, read_account_choice, resolve_account,
};
use switch::{
    SwitchPane, SwitchWorkspace, Usage, UsageMeasure, merge_usage,
    panes_to_restart_on_default_change, panes_to_restart_on_workspace_change, running_count,
    usage_by_account,
};
pub use switcher::SwitchScope;

/// The package's UI state; lives in `App.pkg.accounts`.
#[derive(Debug, Default)]
pub struct State {
    switcher: switcher::Menu,
    page: settings_page::Page,
    writes: usage::Writes,
}

/// What logging in on a separate profile does on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Support {
    /// False on a macOS whose Keychain cannot separate logins.
    pub isolated_logins: bool,
    pub note: Option<String>,
}

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.accounts`.
#[derive(Debug, Default)]
pub struct Live {
    /// Every account, main included. `None` until the backend has answered:
    /// the interface then has no accounts to show, as it had none without a
    /// backend.
    pub accounts: Option<Vec<ClaudeAccount>>,
    pub support: Option<Support>,
    /// The last usage each account reported.
    pub usage: Usage,
    /// The account each running Claude Code pane was launched on (P5 keeps it).
    pub launched: Launched,
}

/// The package's messages, routed by the shell as `Message::Accounts(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The app has started: read the accounts and the usage.
    Started,
    Listed(Vec<ClaudeAccount>),
    SupportRead(Support),
    UsageRead(Usage),
    UsageSaved,

    // The toolbar's switcher.
    ToggleMenu,
    CloseMenu,
    Choose {
        ws: WsId,
        id: AccountId,
        scope: SwitchScope,
    },
    /// The answer to « restart the running agents now? ».
    Decide(bool),
    /// Back from the question to the list.
    Back,

    // Choosing an account for something, from a menu or a picker.
    SetWorkspace {
        ws: WsId,
        id: Option<AccountId>,
        restart: bool,
    },
    SetDefault {
        id: AccountId,
        restart: bool,
    },
    SetPane {
        ws: WsId,
        pane: PaneId,
        id: Option<AccountId>,
    },

    // Settings › Accounts.
    RenameStart(AccountId),
    RenameInput(String),
    RenameSubmit,
    RenameCancel,
    RemoveAsk(AccountId),
    RemoveConfirm(AccountId),
    RemoveCancel,
    MakeDefault(AccountId),
    LogIn(AccountId),
    AddInput(String),
    AddSubmit,
    /// `account_add` ended: the new account's id, or why not.
    Added(Result<AccountId, String>),
    Renamed(Result<(), String>),
    Removed(AccountId, Result<(), String>),
}

fn live<'a>(ctx: Ctx<'a>) -> &'a Live {
    &ctx.model.live.accounts
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Started => Task::batch([refresh(), read_support(), read_usage(sv)]),
        Msg::Listed(accounts) => {
            m.live.accounts.accounts = Some(accounts);
            Task::none()
        }
        Msg::SupportRead(support) => {
            m.live.accounts.support = Some(support);
            Task::none()
        }
        Msg::UsageRead(read) => {
            // What arrived while the file was being read is newer than it.
            let fresh = std::mem::take(&mut m.live.accounts.usage);
            m.live.accounts.usage = merge_usage(&read, &fresh);
            Task::none()
        }
        Msg::UsageSaved => {
            if st.writes.ended() {
                write_usage(m, sv)
            } else {
                Task::none()
            }
        }

        menu @ (Msg::ToggleMenu
        | Msg::CloseMenu
        | Msg::Choose { .. }
        | Msg::Decide(_)
        | Msg::Back) => switcher_step(st, m, menu),

        Msg::SetWorkspace { ws, id, restart } => set_workspace_account(m, &ws, id, restart),
        Msg::SetDefault { id, restart } => set_default_account(m, id, restart),
        Msg::SetPane { ws, pane, id } => set_pane_account(m, &ws, &pane, id),

        page @ (Msg::RenameStart(_)
        | Msg::RenameInput(_)
        | Msg::RenameSubmit
        | Msg::RenameCancel
        | Msg::RemoveAsk(_)
        | Msg::RemoveConfirm(_)
        | Msg::RemoveCancel
        | Msg::MakeDefault(_)
        | Msg::LogIn(_)
        | Msg::AddInput(_)
        | Msg::AddSubmit
        | Msg::Added(_)
        | Msg::Renamed(_)
        | Msg::Removed(..)) => settings_page::update(&mut st.page, m, sv, page),
    }
}

/// The switcher's popover: the list, and the question asked before a switch
/// that restarts running agents. It needs no service, only the model.
fn switcher_step(st: &mut State, m: &mut Model, msg: Msg) -> Task<Message> {
    match msg {
        Msg::ToggleMenu => {
            st.switcher = match st.switcher {
                switcher::Menu::Closed => switcher::Menu::Open,
                _ => switcher::Menu::Closed,
            };
            Task::none()
        }
        Msg::CloseMenu => {
            st.switcher = switcher::Menu::Closed;
            Task::none()
        }
        Msg::Choose { ws, id, scope } => {
            let running = running_in(m, &ws, scope);
            if running == 0 {
                st.switcher = switcher::Menu::Closed;
                return apply_switch(m, &ws, id, scope, false);
            }
            st.switcher = switcher::Menu::Ask { ws, id, scope };
            Task::none()
        }
        Msg::Decide(restart) => {
            let switcher::Menu::Ask { ws, id, scope } =
                std::mem::replace(&mut st.switcher, switcher::Menu::Closed)
            else {
                return Task::none();
            };
            apply_switch(m, &ws, id, scope, restart)
        }
        Msg::Back => {
            st.switcher = switcher::Menu::Open;
            Task::none()
        }
        _ => Task::none(),
    }
}

/// The accounts, read again: at start and after every `accounts:changed`.
/// They are small files on disk, so off the UI thread.
fn refresh() -> Task<Message> {
    Task::perform(
        async {
            tokio::task::spawn_blocking(hangar_core::accounts::accounts_list)
                .await
                .ok()
        },
        |read| match read {
            Some(accounts) => Message::Accounts(Msg::Listed(accounts)),
            None => Message::Noop,
        },
    )
}

/// What the machine can do with them, read once (on macOS it asks the
/// Keychain).
fn read_support() -> Task<Message> {
    Task::perform(
        async {
            tokio::task::spawn_blocking(|| {
                let support = hangar_core::accounts::accounts_support();
                Support {
                    isolated_logins: support.isolated_logins,
                    note: support.note,
                }
            })
            .await
            .ok()
        },
        |read| match read {
            Some(support) => Message::Accounts(Msg::SupportRead(support)),
            None => Message::Noop,
        },
    )
}

fn read_usage(sv: &Services) -> Task<Message> {
    let Some(file) = usage::path(sv.core().host()) else {
        return Task::none();
    };
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || usage::load(&file))
                .await
                .unwrap_or_default()
        },
        |read| Message::Accounts(Msg::UsageRead(read)),
    )
}

/// Writes the figures, unless a write is running: it then writes again, with
/// the latest, when it ends.
fn save_usage(st: &mut State, m: &Model, sv: &Services) -> Task<Message> {
    if st.writes.wanted() {
        write_usage(m, sv)
    } else {
        Task::none()
    }
}

fn write_usage(m: &Model, sv: &Services) -> Task<Message> {
    let Some(file) = usage::path(sv.core().host()) else {
        return Task::done(Message::Accounts(Msg::UsageSaved));
    };
    let figures = m.live.accounts.usage.clone();
    Task::perform(
        async move {
            let _ = tokio::task::spawn_blocking(move || usage::save(&file, &figures)).await;
        },
        |()| Message::Accounts(Msg::UsageSaved),
    )
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    Subscription::run(started)
}

/// A stream that says once that the app has started: this package has no
/// other hook that runs before the first window is drawn.
fn started() -> impl iced::futures::Stream<Item = Message> {
    stream::once(async { Message::Accounts(Msg::Started) })
}

/// What takes the focus when F6 reaches a region this package owns. It owns
/// none, so the shell never calls it.
pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    None
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    // A popover goes when something else asks for it (a dialog opens).
    if matches!(intent, Intent::ClosePopovers(_)) {
        st.switcher = switcher::Menu::Closed;
    }
    None
}

/// Every core event, after the shell's own handling of it.
pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    match event {
        CoreEvent::AccountsChanged => Some(refresh()),
        // A session counts for the account its pane was launched on.
        CoreEvent::Bridge { pane, event } => {
            record_usage(m, pane, event, now_ms()).then(|| save_usage(st, m, sv))
        }
        _ => None,
    }
}

/// A report of a pane's bridge, kept when it carries the plan's limits and
/// the pane runs on an account. Returns whether the figures changed.
fn record_usage(m: &mut Model, pane: &PaneId, event: &serde_json::Value, at: u64) -> bool {
    let Some(BridgeEvent::Usage { rate_limits, .. }) = BridgeEvent::parse(event) else {
        return false;
    };
    if rate_limits.is_empty() {
        return false;
    }
    let Some(account) = m.live.accounts.launched.account(pane).map(str::to_string) else {
        return false;
    };
    let fresh = usage_by_account(
        &[UsageMeasure {
            pane_id: pane.clone(),
            at,
            rate_limits,
        }],
        |_| Some(account.clone()),
    );
    m.live.accounts.usage = merge_usage(&m.live.accounts.usage, &fresh);
    true
}

// ── switching ─────────────────────────────────────────────────────────

/// A workspace as the switching logic reads it; `running` = a Claude Code is
/// up in the pane.
fn switch_workspace(workspace: &Workspace, live: &Live) -> SwitchWorkspace {
    SwitchWorkspace {
        id: workspace.id.clone(),
        claude_account: workspace.claude_account.clone(),
        panes: workspace
            .panes
            .iter()
            .map(|pane| SwitchPane {
                id: pane.id.clone(),
                // A pane opened to log in runs no agent: nothing to restart.
                agent: if pane.login == Some(true) {
                    "login".to_string()
                } else {
                    pane.agent.as_str().to_string()
                },
                claude_account: pane.claude_account.clone(),
                running: live.launched.is_running(&pane.id),
            })
            .collect(),
    }
}

fn switch_workspaces(m: &Model) -> Vec<SwitchWorkspace> {
    m.state()
        .workspaces
        .iter()
        .map(|workspace| switch_workspace(workspace, &m.live.accounts))
        .collect()
}

/// The panes of `workspace` a change of its account restarts, running ones
/// only: a stopped pane has no conversation to resume.
fn running_ids(panes: Vec<&SwitchPane>) -> Vec<PaneId> {
    panes
        .into_iter()
        .filter(|pane| pane.running)
        .map(|pane| pane.id.clone())
        .collect()
}

/// How many running agents a switch in this scope would restart.
fn running_in(m: &Model, ws: &WsId, scope: SwitchScope) -> usize {
    match scope {
        SwitchScope::Workspace => m
            .workspace(ws)
            .map(|workspace| switch_workspace(workspace, &m.live.accounts))
            .map_or(0, |workspace| {
                running_count(&panes_to_restart_on_workspace_change(&workspace))
            }),
        SwitchScope::Default => {
            running_count(&panes_to_restart_on_default_change(&switch_workspaces(m)))
        }
    }
}

fn apply_switch(
    m: &mut Model,
    ws: &WsId,
    id: AccountId,
    scope: SwitchScope,
    restart: bool,
) -> Task<Message> {
    match scope {
        SwitchScope::Workspace => set_workspace_account(m, ws, Some(id), restart),
        SwitchScope::Default => set_default_account(m, id, restart),
    }
}

/// The agents restart with their conversation: a respawn with no patch keeps
/// the session. Always the person's call.
fn restart_task(ws: &WsId, panes: Vec<PaneId>) -> Task<Message> {
    if panes.is_empty() {
        return Task::none();
    }
    Task::done(Message::Intent(Intent::RestartPanes {
        ws: ws.clone(),
        panes,
    }))
}

fn set_workspace_account(
    m: &mut Model,
    ws: &WsId,
    id: Option<AccountId>,
    restart: bool,
) -> Task<Message> {
    let Some(workspace) = m.workspace(ws) else {
        return Task::none();
    };
    // Which panes follow is decided before the change; ids survive it, and
    // the respawn re-resolves the account at launch.
    let switch = switch_workspace(workspace, &m.live.accounts);
    let panes = running_ids(panes_to_restart_on_workspace_change(&switch));
    m.apply(StoreAction::UpdateWorkspace(
        ws.clone(),
        Patch::new(move |workspace: &mut Workspace| workspace.claude_account = id.clone()),
    ));
    if restart {
        restart_task(ws, panes)
    } else {
        Task::none()
    }
}

fn set_default_account(m: &mut Model, id: AccountId, restart: bool) -> Task<Message> {
    let workspaces = switch_workspaces(m);
    let affected: Vec<(WsId, Vec<PaneId>)> = workspaces
        .iter()
        .filter(|workspace| read_account_choice(workspace.claude_account.as_deref()).is_none())
        .map(|workspace| {
            (
                workspace.id.clone(),
                running_ids(panes_to_restart_on_workspace_change(workspace)),
            )
        })
        .collect();
    m.apply(StoreAction::UpdateSettings(Patch::new(
        move |settings: &mut Settings| settings.claude_account = Some(id.clone()),
    )));
    if !restart {
        return Task::none();
    }
    Task::batch(
        affected
            .into_iter()
            .map(|(ws, panes)| restart_task(&ws, panes)),
    )
}

/// The person's pick in the pane's menu: a running agent restarts on it,
/// conversation kept.
fn set_pane_account(
    m: &mut Model,
    ws: &WsId,
    pane: &PaneId,
    id: Option<AccountId>,
) -> Task<Message> {
    let running = m.live.accounts.launched.is_running(pane);
    m.apply(StoreAction::UpdatePane {
        ws: ws.clone(),
        pane: pane.clone(),
        patch: Patch::new(move |pane: &mut crate::state::Pane| pane.claude_account = id.clone()),
    });
    if running {
        restart_task(ws, vec![pane.clone()])
    } else {
        Task::none()
    }
}

// ── the views other packages embed ────────────────────────────────────

/// The toolbar's account switcher (embedded by the toolbar).
pub fn switcher<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    switcher::view(st, ctx)
}

/// A pane header's account badge.
pub fn pane_badge<'a>(ctx: Ctx<'a>, ws: &'a WsId, pane: &'a PaneId) -> Element<'a, Message> {
    badge::pane(ctx, ws, pane)
}

/// A navigator row's account badge.
pub fn badge<'a>(ctx: Ctx<'a>, ws: &'a WsId) -> Element<'a, Message> {
    badge::workspace(ctx, ws)
}

/// The account entries of the navigator's and the pane's menus.
pub fn menu_items<'a>(ctx: Ctx<'a>, ws: &'a WsId, pane: Option<&'a PaneId>) -> Vec<MenuEntry> {
    menu_items::menu_entries(ctx, ws, pane)
}

/// The workspace dialog's account picker. `value` is the account the
/// workspace being made has chosen for itself (`None`: the default).
pub fn workspace_picker<'a>(ctx: Ctx<'a>, value: Option<&'a str>) -> Element<'a, Message> {
    picker::view(ctx, value)
}

/// Settings › Accounts (embedded by the settings).
pub fn settings_page<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    settings_page::view(&st.page, ctx)
}

/// Whether the person has more than one account to tell apart: the pickers
/// and the badges are for them.
pub fn has_several(ctx: Ctx<'_>) -> bool {
    live(ctx)
        .accounts
        .as_ref()
        .is_some_and(|accounts| accounts.len() > 1)
}

/// Whether there is any account to choose from (the dialog shows the picker
/// then).
pub fn has_any(ctx: Ctx<'_>) -> bool {
    live(ctx)
        .accounts
        .as_ref()
        .is_some_and(|accounts| !accounts.is_empty())
}
