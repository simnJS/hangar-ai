//! « Serveurs MCP »: which MCP servers a workspace's Claude Code panes start
//! with (`McpServersPanel.tsx`, `scope.rs`).
//!
//! Changes are saved as they are made, and reach the panes on their next
//! launch: restarting a running agent interrupts it, so that stays a button of
//! its own, and it only restarts the panes that run with something else.

use iced::widget::{column, container, row, rule, space, text};
use iced::{Alignment, Element, Length, Padding, Task};

use hangar_core::mcp_scope::{self, Discovery, ServerInfo, Status};
use hangar_i18n::{Key, args};
use hangar_theme::{fonts, style, tokens};
use hangar_ui::badge::{self, Tone};
use hangar_ui::{Icon, button, dialog, icon, input, text as typography, tip};

use super::scope::{
    PaneMcp, inherit, is_loaded, needs_restart, preset_all, preset_hangar_only, stdio_count,
    toggle_server, workspace_mcp,
};
use super::{Msg, State};
use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::Services;
use crate::state::types::{AgentId, McpMode, Pane, WorkspaceMcp, WsId};
use crate::state::{Model, Patch, StoreAction, Workspace};

/// What the panel knows of the servers.
#[derive(Debug)]
enum Found {
    Loading,
    Failed(String),
    Ready(Discovery),
}

/// The panel while it is open.
#[derive(Debug)]
pub struct Servers {
    ws: WsId,
    found: Found,
}

#[derive(Debug, Clone)]
pub enum ServersMsg {
    /// `mcp_servers` answered for this workspace.
    Found(WsId, Result<Discovery, String>),
    /// « Claude Code's » (`Inherit`) or « Chosen here ».
    Mode(McpMode),
    /// The « All » preset.
    All,
    /// The « None but Hangar » preset.
    None,
    Toggle(String, bool),
    /// « Restart N agents »: the panes that run with something else.
    Restart,
}

/// Opens the panel on a workspace and reads Claude Code's configuration for
/// its folder, off the UI thread.
pub fn open(st: &mut State, m: &Model, ws_id: &str) -> Task<Message> {
    let Some(ws) = m.workspace(ws_id) else {
        st.servers = None;
        return Task::none();
    };
    st.servers = Some(Servers {
        ws: ws.id.clone(),
        found: Found::Loading,
    });
    let (id, cwd, account) = (ws.id.clone(), ws.cwd.clone(), ws.claude_account.clone());
    Task::perform(
        async move {
            let at = id.clone();
            let found = tokio::task::spawn_blocking(move || mcp_scope::mcp_servers(cwd, account))
                .await
                .map_err(|error| error.to_string());
            ServersMsg::Found(at, found)
        },
        |message| Message::Mcp(Msg::Servers(message)),
    )
}

/// What a pane's Claude Code was started with, if this window saw it launch.
///
/// TODO(P4): `PaneInsight.mcp` is raw JSON in `state::live` until P4 types it
/// and keeps a map of insights in `panes_logic::Live`; read it from there.
/// Until then no launch is known, so every Claude Code pane counts as out of
/// date, which is the TS's rule for a launch this window never saw.
fn running_mcp(_m: &Model, _pane: &Pane) -> Option<PaneMcp> {
    None
}

/// Whether a pane is in the middle of a turn.
///
/// TODO(P4): `panes_logic::Live` holds each pane's activity once P4 fills it.
fn is_working(_m: &Model, _pane: &Pane) -> bool {
    false
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: ServersMsg) -> Task<Message> {
    let Some(panel) = st.servers.as_mut() else {
        return Task::none();
    };
    match msg {
        ServersMsg::Found(ws, found) => {
            // An answer for a workspace the panel has since left.
            if ws == panel.ws {
                panel.found = match found {
                    Ok(discovery) => Found::Ready(discovery),
                    Err(error) => Found::Failed(error),
                };
            }
            Task::none()
        }
        ServersMsg::Mode(McpMode::Inherit) => save(m, sv, &panel.ws, inherit()),
        ServersMsg::Mode(McpMode::Custom) => {
            // Starts from what Claude Code loads, so switching changes nothing
            // until a server is switched off.
            let Found::Ready(discovery) = &panel.found else {
                return Task::none();
            };
            let current = workspace_mcp(m.workspace(&panel.ws));
            if current.mode == McpMode::Inherit {
                let next = preset_all(&discovery.servers);
                return save(m, sv, &panel.ws, next);
            }
            Task::none()
        }
        ServersMsg::All => match &panel.found {
            Found::Ready(discovery) => {
                let next = preset_all(&discovery.servers);
                save(m, sv, &panel.ws, next)
            }
            _ => Task::none(),
        },
        ServersMsg::None => match &panel.found {
            Found::Ready(_) => save(m, sv, &panel.ws, preset_hangar_only()),
            _ => Task::none(),
        },
        ServersMsg::Toggle(name, on) => {
            let Found::Ready(discovery) = &panel.found else {
                return Task::none();
            };
            let current = workspace_mcp(m.workspace(&panel.ws));
            let next = toggle_server(&current, &discovery.servers, &name, on);
            save(m, sv, &panel.ws, next)
        }
        ServersMsg::Restart => {
            let Found::Ready(discovery) = &panel.found else {
                return Task::none();
            };
            let Some(ws) = m.workspace(&panel.ws) else {
                return Task::none();
            };
            let mcp = workspace_mcp(Some(ws));
            let stale: Vec<String> = stale_panes(m, ws, &mcp, &discovery.servers)
                .map(|pane| pane.id.clone())
                .collect();
            Task::batch([
                Task::done(Message::Intent(Intent::RestartPanes {
                    ws: panel.ws.clone(),
                    panes: stale,
                })),
                Task::done(Message::Intent(Intent::CloseOverlay)),
            ])
        }
    }
}

/// Saves the choice, on every toggle. Back to Claude Code's own configuration,
/// the copies Hangar made of the servers' entries have nothing left to serve.
fn save(m: &mut Model, sv: &Services, ws_id: &WsId, next: WorkspaceMcp) -> Task<Message> {
    let back_to_inherit = next.mode == McpMode::Inherit;
    let stored = next;
    m.apply(StoreAction::UpdateWorkspace(
        ws_id.clone(),
        Patch::new(move |ws: &mut Workspace| ws.mcp = Some(stored.clone())),
    ));
    if !back_to_inherit {
        return Task::none();
    }
    // Nothing to report: a file that stays behind is harmless, and the next
    // launch overwrites it.
    let host = sv.core().host().clone();
    let id = ws_id.clone();
    Task::perform(
        async move {
            let _ =
                tokio::task::spawn_blocking(move || mcp_scope::mcp_scope_clear(&host, id)).await;
        },
        |()| Message::Noop,
    )
}

/// The Claude Code panes that run with something else than the choice.
fn stale_panes<'a>(
    m: &'a Model,
    ws: &'a Workspace,
    mcp: &'a WorkspaceMcp,
    servers: &'a [ServerInfo],
) -> impl Iterator<Item = &'a Pane> {
    ws.panes
        .iter()
        .filter(|pane| pane.agent == AgentId::Claude)
        .filter(move |pane| needs_restart(running_mcp(m, pane).as_ref(), mcp, servers))
}

fn msg(message: ServersMsg) -> Message {
    Message::Mcp(Msg::Servers(message))
}

/// A label without arguments, for the widgets that keep a `&str` for the whole
/// frame. The dictionary lends such a label as a `'static` borrow
/// (hangar-i18n, `t`), so nothing is allocated.
fn label(ctx: Ctx<'_>, key: Key) -> &'static str {
    match ctx.t(key) {
        std::borrow::Cow::Borrowed(text) => text,
        std::borrow::Cow::Owned(text) => Box::leak(text.into_boxed_str()),
    }
}

fn same_names(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().all(|name| b.contains(name))
}

/// `Overlay::McpServers`: a workspace's MCP servers.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>, ws_id: &'a WsId) -> Element<'a, Message> {
    let t = ctx.theme;
    let (Some(panel), Some(ws)) = (
        st.servers.as_ref().filter(|panel| &panel.ws == ws_id),
        ctx.model.workspace(ws_id),
    ) else {
        return space().into();
    };
    let mcp = workspace_mcp(Some(ws));
    let discovery = match &panel.found {
        Found::Ready(discovery) => Some(discovery),
        _ => None,
    };
    let servers: &[ServerInfo] = discovery.map_or(&[], |d| d.servers.as_slice());

    let mode_hint = if mcp.mode == McpMode::Inherit {
        ctx.t(Key::McpScopeInheritHint)
    } else {
        ctx.t(Key::McpScopeCustomHint)
    };
    let mode = row![
        input::segmented(
            t,
            &[
                (McpMode::Inherit, label(ctx, Key::McpScopeInherit)),
                (McpMode::Custom, label(ctx, Key::McpScopeCustom)),
            ],
            &mcp.mode,
            |mode| msg(ServersMsg::Mode(mode)),
        ),
        container(typography::hint(t, mode_hint)).width(Length::Fill),
    ]
    .spacing(tokens::space::LG)
    .align_y(Alignment::Center);

    let mut body = column![
        typography::label(t, ctx.t(Key::McpScopeIntro)),
        mode,
        list_group(ctx, panel, &mcp, servers),
    ]
    .spacing(tokens::space::XL + 2.0);
    if discovery.is_some() {
        body = body.push(stats(ctx, ws, &mcp, servers));
    }

    // The footer: what the choice reaches, and the restart that makes it so.
    let stale: Vec<&Pane> = if discovery.is_some() {
        stale_panes(ctx.model, ws, &mcp, servers).collect()
    } else {
        Vec::new()
    };
    let claude_panes = ws
        .panes
        .iter()
        .filter(|pane| pane.agent == AgentId::Claude)
        .count();
    let working = stale
        .iter()
        .filter(|pane| is_working(ctx.model, pane))
        .count();
    let mut note = if discovery.is_some() && claude_panes > 0 && stale.is_empty() {
        ctx.t(Key::McpScopeUpToDate).into_owned()
    } else {
        ctx.t(Key::McpScopeApplies).into_owned()
    };
    if working > 0 {
        note.push(' ');
        note.push_str(&ctx.i18n.tn(Key::McpScopeBusy, working, &[]));
    }
    let restart = button::primary(
        t,
        if stale.is_empty() {
            ctx.t(Key::McpScopeRestartNone)
        } else {
            ctx.i18n.tn(Key::McpScopeRestart, stale.len(), &[])
        },
    )
    .icon(Icon::Restart)
    .tip(ctx.t(Key::McpScopeRestartHint))
    .on_press_maybe((!stale.is_empty()).then(|| msg(ServersMsg::Restart)));
    let close =
        button::ghost(t, ctx.t(Key::McpScopeClose)).on_press(Message::Intent(Intent::CloseOverlay));

    let shown = dialog::dialog(t, ctx.t(Key::McpScopeTitle), body)
        .subtitle(format!("{} · {}", ws.name, ws.cwd))
        .note(note)
        .width(tokens::size::DIALOG_LIST_WIDTH + 40.0)
        .body_max_height(480.0)
        .close_tip(ctx.t(Key::McpScopeClose))
        .on_close(Message::Intent(Intent::CloseOverlay))
        .action(close)
        .action(restart);
    dialog::over(
        t,
        space().width(Length::Fill).height(Length::Fill),
        Some(shown),
    )
    .into()
}

/// The list's title with the two presets, and the list under it, or why there
/// is none.
fn list_group<'a>(
    ctx: Ctx<'a>,
    panel: &'a Servers,
    mcp: &WorkspaceMcp,
    servers: &'a [ServerInfo],
) -> Element<'a, Message> {
    let t = ctx.theme;
    let ready = matches!(panel.found, Found::Ready(_));
    let all = preset_all(servers);
    let is_all = mcp.mode == McpMode::Custom && same_names(&mcp.enabled, &all.enabled);
    let is_none = mcp.mode == McpMode::Custom && mcp.enabled.is_empty();

    let preset = |key: Key, hint: Key, pressed: bool, message: ServersMsg| {
        let chip = button::tab_button(t, ctx.t(key), pressed);
        let chip = if ready {
            chip.on_press(msg(message))
        } else {
            chip
        };
        tip::bottom(t, chip, ctx.t(hint))
    };
    let head = row![
        typography::heading(t, ctx.t(Key::McpScopeListTitle).to_uppercase()),
        space::horizontal(),
        preset(
            Key::McpScopeAll,
            Key::McpScopeAllHint,
            is_all,
            ServersMsg::All
        ),
        preset(
            Key::McpScopeNone,
            Key::McpScopeNoneHint,
            is_none,
            ServersMsg::None
        ),
    ]
    .spacing(tokens::space::SM)
    .align_y(Alignment::Center);

    let mut group = column![head].spacing(tokens::space::SM);
    match &panel.found {
        Found::Failed(error) => {
            group = group.push(alert(
                ctx,
                ctx.i18n
                    .t_with(Key::McpScopeLoadFailed, &args!("error" => error.as_str()))
                    .into_owned(),
            ));
        }
        Found::Loading => {
            group = group.push(typography::muted(t, ctx.t(Key::McpScopeLoading)));
        }
        Found::Ready(discovery) => {
            group = group.push(rows(ctx, mcp, &discovery.servers));
            // `--strict-mcp-config` drops every source but the file, and the
            // list above only covers the three the file is built from.
            if mcp.mode == McpMode::Custom {
                group = group.push(
                    row![
                        icon(Icon::Info, tokens::size::ICON).color(t.colors.fg_secondary),
                        typography::hint(t, ctx.t(Key::McpScopeStrictNote)).width(Length::Fill),
                    ]
                    .spacing(tokens::space::SM),
                );
            }
            for path in &discovery.unreadable {
                group = group.push(alert(
                    ctx,
                    ctx.i18n
                        .t_with(Key::McpScopeUnreadable, &args!("path" => path.as_str()))
                        .into_owned(),
                ));
            }
        }
    }
    group.into()
}

/// A line that something is wrong, in the danger tint.
fn alert<'a>(ctx: Ctx<'a>, words: String) -> Element<'a, Message> {
    let t = ctx.theme;
    container(
        row![
            icon(Icon::Alert, tokens::size::ICON).color(t.colors.danger),
            text(words)
                .size(tokens::font_size::CAPTION_LG)
                .width(Length::Fill),
        ]
        .spacing(tokens::space::MD),
    )
    .padding(Padding::from([tokens::space::MD, tokens::space::LG]))
    .width(Length::Fill)
    .style(style::container::badge(t, Tone::Danger))
    .into()
}

/// Hangar's own server first, then the others, in one well.
fn rows<'a>(ctx: Ctx<'a>, mcp: &WorkspaceMcp, servers: &'a [ServerInfo]) -> Element<'a, Message> {
    let t = ctx.theme;
    let mut list = column![];
    let others: Vec<&ServerInfo> = servers.iter().filter(|server| !server.hangar).collect();
    list = list.push(server_row(ctx, mcp, servers.iter().find(|s| s.hangar)));
    for server in &others {
        list = list
            .push(rule::horizontal(1).style(style::rule::hairline(t)))
            .push(server_row(ctx, mcp, Some(server)));
    }
    if others.is_empty() {
        list = list
            .push(rule::horizontal(1).style(style::rule::hairline(t)))
            .push(
                container(typography::muted(t, ctx.t(Key::McpScopeEmpty)))
                    .padding(tokens::space::XL),
            );
    }
    container(list)
        .width(Length::Fill)
        .clip(true)
        .style(style::container::well(t))
        .into()
}

/// A server of the list; `None` is Hangar's own where no file registers it.
fn server_row<'a>(
    ctx: Ctx<'a>,
    mcp: &WorkspaceMcp,
    server: Option<&'a ServerInfo>,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let is_hangar = server.is_none_or(|server| server.hangar);
    let name = server.map_or("hangar", |server| server.name.as_str());
    let loaded = match server {
        Some(server) => is_loaded(server, mcp),
        None => mcp.mode == McpMode::Custom,
    };

    let mut title = row![
        text(name)
            .size(tokens::font_size::BODY)
            .font(fonts::ui_medium())
            .style(move |theme: &iced::Theme| if loaded {
                style::text::primary(t)(theme)
            } else {
                style::text::muted(t)(theme)
            })
    ]
    .spacing(tokens::space::SM)
    .align_y(Alignment::Center);
    if is_hangar {
        title = title.push(badge::tag(t, ctx.t(Key::McpScopeHangar)));
    }
    if let Some(server) = server
        && server.status != Status::Active
    {
        let (word, hint) = match server.status {
            Status::Unapproved => (
                Key::McpScopeStatusUnapproved,
                Key::McpScopeStatusHintUnapproved,
            ),
            Status::Rejected => (Key::McpScopeStatusRejected, Key::McpScopeStatusHintRejected),
            _ => (Key::McpScopeStatusDisabled, Key::McpScopeStatusHintDisabled),
        };
        title = title.push(tip::bottom(
            t,
            badge::badge(t, ctx.t(word), Tone::Neutral),
            ctx.t(hint),
        ));
    }

    let meta: Element<'a, Message> = match server {
        Some(server) => {
            let (scope, scope_hint) = match server.scope {
                mcp_scope::Scope::User => (Key::McpScopeScopeUser, Key::McpScopeScopeHintUser),
                mcp_scope::Scope::Local => (Key::McpScopeScopeLocal, Key::McpScopeScopeHintLocal),
                mcp_scope::Scope::Project => {
                    (Key::McpScopeScopeProject, Key::McpScopeScopeHintProject)
                }
            };
            let transport = match server.transport {
                mcp_scope::Transport::Stdio => "stdio",
                mcp_scope::Transport::Http => "http",
                mcp_scope::Transport::Sse => "sse",
                mcp_scope::Transport::Ws => "ws",
            };
            let dot = || typography::subtle(t, "·");
            row![
                tip::bottom(
                    t,
                    typography::hint(t, ctx.t(scope)).size(tokens::font_size::CAPTION),
                    ctx.i18n
                        .t_with(scope_hint, &args!("path" => server.source.as_str())),
                ),
                dot(),
                typography::mono(t, transport).size(tokens::font_size::LABEL),
                dot(),
                container(
                    typography::mono(t, server.summary.as_str())
                        .size(tokens::font_size::LABEL)
                        .wrapping(text::Wrapping::None)
                )
                .clip(true)
                .width(Length::Fill),
            ]
            .spacing(tokens::space::SM)
            .align_y(Alignment::Center)
            .into()
        }
        None => typography::hint(t, ctx.t(Key::McpScopeHangarMissing)).into(),
    };

    let mut toggle = input::switch(t, loaded);
    // Hangar's own server is not the user's to drop: the panes reach the
    // board, each other and the memory through it.
    if let (Some(server), false) = (server, is_hangar) {
        let name = server.name.clone();
        toggle = toggle.on_toggle(move |on| msg(ServersMsg::Toggle(name.clone(), on)));
    }
    let toggle: Element<'a, Message> = if is_hangar {
        tip::bottom(t, toggle, ctx.t(Key::McpScopeHangarHint))
    } else {
        toggle.into()
    };

    container(
        row![column![title, meta].spacing(2).width(Length::Fill), toggle]
            .spacing(tokens::space::XL)
            .align_y(Alignment::Center),
    )
    .padding(Padding::from([tokens::space::MD, tokens::space::XL]))
    .into()
}

/// What the choice costs, in processes: figures first, set like a meter.
fn stats<'a>(
    ctx: Ctx<'a>,
    ws: &'a Workspace,
    mcp: &WorkspaceMcp,
    servers: &'a [ServerInfo],
) -> Element<'a, Message> {
    let t = ctx.theme;
    let per_session = stdio_count(servers, mcp);
    let sessions = ws
        .panes
        .iter()
        .filter(|pane| pane.agent == AgentId::Claude)
        .count();
    let total = per_session * sessions;
    // What Claude Code's own configuration would start, to compare with.
    let inherited = stdio_count(servers, &inherit()) * sessions;

    let figure = |n: usize| {
        text(n.to_string())
            .size(tokens::font_size::TITLE)
            .font(fonts::ui_semibold())
            .style(style::text::primary(t))
    };
    let tile = |figures: Element<'a, Message>, label: String| -> Element<'a, Message> {
        container(column![figures, typography::hint(t, label)].spacing(2))
            .padding(Padding::from([tokens::space::LG, tokens::space::XL]))
            .width(Length::FillPortion(1))
            .into()
    };

    let mut figures = row![figure(total)]
        .spacing(tokens::space::SM)
        .align_y(Alignment::End);
    if mcp.mode == McpMode::Custom && inherited != total {
        let was = typography::subtle(
            t,
            ctx.i18n
                .t_with(Key::McpScopeStatWas, &args!("n" => inherited)),
        );
        // Fewer than Claude Code's own is good news, and reads as such.
        let was = if inherited > total {
            was.style(style::text::success(t))
        } else {
            was
        };
        figures = figures.push(tip::bottom(t, was, ctx.t(Key::McpScopeStatWasHint)));
    }
    let processes = tip::bottom(
        t,
        tile(
            figures.into(),
            ctx.i18n
                .tn(Key::McpScopeStatProcesses, total, &[])
                .into_owned(),
        ),
        ctx.t(Key::McpScopeStatProcessesHint),
    );
    let divider = || rule::vertical(1).style(style::rule::hairline(t));
    container(
        row![
            tile(
                figure(per_session).into(),
                ctx.i18n
                    .tn(Key::McpScopeStatPerSession, per_session, &[])
                    .into_owned()
            ),
            divider(),
            tile(
                figure(sessions).into(),
                ctx.i18n
                    .tn(Key::McpScopeStatSessions, sessions, &[])
                    .into_owned()
            ),
            divider(),
            container(processes).width(Length::FillPortion(1)),
        ]
        .height(Length::Shrink),
    )
    .width(Length::Fill)
    .clip(true)
    .style(style::container::well(t))
    .into()
}
