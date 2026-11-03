//! P6: Toolbar (`Toolbar.tsx`, `ToolbarActivity.tsx`, `ToolbarMenu.tsx`).
//!
//! The bar over the workspace: which view, how the panes are laid out, and the
//! actions on all of them at once. Everything it does is the app's: it lays
//! the controls out and turns their clicks into intents.
//!
//! The web toolbar was narrowed by container queries written in em of its own
//! font; here the same steps are taken from the width the toolbar is given
//! (`responsive`), in the same units: under 80 em the workspace's name goes,
//! under 71 em the capsule drops its context chip and counters, under 64 em
//! the views are icons, and under 46 em the terminal actions fold into the
//! « … » menu.
//!
//! Gap: iced 0.14's buttons take no keyboard focus, so the view switcher's
//! arrow-key walk (a radio group) is not here; the `view.*` shortcuts do the
//! same from anywhere.

pub mod activity;
pub mod menus;

use std::collections::HashMap;

use iced::widget::{Space, container, responsive, row, text};
use iced::{Alignment, Element, Length, Padding, Subscription, Task};

use hangar_i18n::{Key, args};
use hangar_theme::tokens::{font_size, space};
use hangar_theme::{Theme, fonts, style};
use hangar_ui::layer;
use hangar_ui::menu::{self as ui_menu};
use hangar_ui::{Icon, button as ui_button, icon, tip};

use crate::commands::CommandId;
use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::navigator::model::closed_by_preset;
use crate::navigator::reads;
use crate::services::{CoreEvent, Services};
use crate::shell::{Region, View, WindowKind};
use crate::state::live::BridgeEvent;
use crate::state::types::{MAX_PANES, Workspace};
use crate::state::{AgentActivity, Model, PaneId, WsId};

use activity::Glance;
use menus::LAYOUTS;

/// One em of the toolbar's own font (`.toolbar { font-size: 12px }`), the unit
/// of its container queries.
const EM: f32 = 12.0;

/// A preset asking first (`presetAsk`): it would close panes that still have
/// something going on.
#[derive(Debug, Clone)]
pub struct PresetAsk {
    ws: WsId,
    view: View,
    size: u8,
    /// Every pane it would close, in reading order.
    closing: Vec<(PaneId, String)>,
    /// Those that are busy, with what they are doing.
    busy: Vec<(String, AgentActivity)>,
}

impl PresetAsk {
    pub fn closing_names(&self) -> Vec<String> {
        self.closing.iter().map(|(_, name)| name.clone()).collect()
    }
}

/// The package's UI state; lives in `App.pkg.toolbar`.
#[derive(Debug, Default)]
pub struct State {
    /// What the capsule says of each pane's agent.
    glances: HashMap<PaneId, Glance>,
    /// The « … » menu is open, from its button.
    more_open: bool,
    /// The workspace whose Kits menu is open: another workspace's kits are
    /// another menu.
    kits_for: Option<WsId>,
    preset: Option<PresetAsk>,
}

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.toolbar`.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::Toolbar(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    ToggleMore,
    ToggleKits,
    /// Escape, Tab or a press elsewhere on the « … » menu or one of its
    /// questions.
    CloseMenus,
    /// A menu line: the menus close, then the intents run (an entry runs once
    /// the menu is gone, so it never fights it for the keyboard).
    Pick(Vec<Intent>),
    /// A layout preset was pressed.
    Preset(u8),
    PresetConfirm,
    PresetCancel,
    /// A counter of the capsule: the terminals, and the next pane in that
    /// state.
    GoTo(PaneId),
}

fn intents(list: Vec<Intent>) -> Task<Message> {
    Task::batch(
        list.into_iter()
            .map(|intent| Task::done(Message::Intent(intent))),
    )
}

pub fn update(st: &mut State, m: &mut Model, _sv: &Services, msg: Msg) -> Task<Message> {
    let active = m.active_workspace().map(|ws| ws.id.clone());
    match msg {
        Msg::ToggleMore | Msg::CloseMenus => {
            let opening = matches!(msg, Msg::ToggleMore)
                && !st.more_open
                && !m.ui.confirm_reset
                && !m.ui.confirm_stop_workers;
            // A question the menu was showing is called off with it.
            let mut asked = Vec::new();
            if m.ui.confirm_reset {
                asked.push(Intent::ArmResetAgents(false));
            }
            if m.ui.confirm_stop_workers {
                asked.push(Intent::ArmStopWorkers(false));
            }
            st.more_open = opening;
            intents(asked)
        }
        Msg::ToggleKits => {
            st.kits_for = match (&st.kits_for, active) {
                (Some(open), Some(now)) if *open == now => None,
                (_, now) => now,
            };
            Task::none()
        }
        Msg::Pick(list) => {
            st.more_open = false;
            st.kits_for = None;
            intents(list)
        }
        Msg::Preset(size) => {
            let Some(ws) = m.active_workspace() else {
                return Task::none();
            };
            let closing: Vec<(PaneId, String)> = closed_by_preset(ws, size)
                .iter()
                .map(|pane| (pane.id.clone(), pane.name.clone()))
                .collect();
            // A smaller preset closes the panes read last. When one of them
            // still has an agent at work, waiting on you or with an answer you
            // have not read, the control, which looks like a harmless view
            // switch, asks first.
            let busy = busy_among(&closing, |id| reads::activity(m, id));
            if busy.is_empty() {
                return intents(vec![Intent::ApplyPreset {
                    ws: ws.id.clone(),
                    size,
                }]);
            }
            st.preset = Some(PresetAsk {
                ws: ws.id.clone(),
                view: m.ui.view,
                size,
                closing,
                busy,
            });
            Task::none()
        }
        Msg::PresetConfirm => match st.preset.take() {
            Some(ask) => intents(vec![Intent::ApplyPreset {
                ws: ask.ws,
                size: ask.size,
            }]),
            None => Task::none(),
        },
        Msg::PresetCancel => {
            st.preset = None;
            Task::none()
        }
        Msg::GoTo(pane) => {
            let Some(ws) = active else {
                return Task::none();
            };
            let mut list = Vec::new();
            if m.ui.view != View::Terminals {
                list.push(Intent::SetView(View::Terminals));
            }
            list.push(Intent::FocusPane {
                ws,
                pane,
                terminal: true,
            });
            intents(list)
        }
    }
}

/// The panes among `closing` with something going on, and what: the ones a
/// preset would stop mid-work.
pub fn busy_among(
    closing: &[(PaneId, String)],
    activity: impl Fn(&PaneId) -> AgentActivity,
) -> Vec<(String, AgentActivity)> {
    closing
        .iter()
        .filter_map(|(id, name)| {
            let doing = activity(id);
            (doing != AgentActivity::Idle).then(|| (name.clone(), doing))
        })
        .collect()
}

pub fn subscription(_st: &State, _m: &Model) -> Subscription<Message> {
    Subscription::none()
}

/// What takes the focus when F6 reaches region `Toolbar`: nothing, since an
/// iced button takes no keyboard focus; the region is reached and its keys
/// are the shortcuts'.
pub fn focus_entry(_st: &State, _m: &Model, _region: Region) -> Option<Task<Message>> {
    None
}

/// Every intent, after the shell's own handling of it.
pub fn on_intent(
    st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    // A question about a workspace's panes goes with a change of workspace.
    if matches!(intent, Intent::ActivateWorkspace(_) | Intent::SetView(_)) {
        st.preset = None;
    }
    None
}

/// The mod's reports feed the capsule.
pub fn on_event(
    st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    if let CoreEvent::Bridge { pane, event } = event
        && let Some(report) = BridgeEvent::parse(event)
    {
        st.glances.entry(pane.clone()).or_default().note(&report);
    }
    None
}

/// Region `toolbar`. Embeds `needs_you::button` and `accounts::switcher`.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let Some(ws) = ctx.model.active_workspace() else {
        return Space::new().into();
    };
    responsive(move |size| bar(st, ctx, ws, size.width)).into()
}

/// The toolbar, laid out for `width` logical pixels.
fn bar<'a>(st: &'a State, ctx: Ctx<'a>, ws: &'a Workspace, width: f32) -> Element<'a, Message> {
    let t = ctx.theme;
    let tr = ctx.i18n;
    let model = ctx.model;
    let settings = model.settings();
    let view = model.ui.view;
    let terminals = view == View::Terminals;
    let kind = model.live.windows.get(&ctx.window);
    let is_main = kind == Some(&WindowKind::Main);
    let in_own_window = matches!(kind, Some(WindowKind::Workspace(_)));
    let keys = |id: CommandId| reads::binding(&ctx, id);

    let show_title = width >= 80.0 * EM;
    let extras = width >= activity::EXTRAS_FROM_EM * EM;
    let icon_views = width < 64.0 * EM;
    let folded = width < 46.0 * EM;

    // ---- The lead: the navigator's door, the name, the views.
    let mut lead = row![].spacing(space::MD).align_y(Alignment::Center);
    if is_main && !settings.navigator_open {
        lead = lead.push(
            ui_button::icon_button(t, Icon::SidebarLeft)
                .tip(tr.t(Key::ToolbarShowNavigator).into_owned())
                .keys(keys(CommandId::ViewNavigator))
                .on_press(Message::Intent(Intent::ToggleNavigator)),
        );
    }
    if show_title {
        // The name alone: its folder is a detail, in the tooltip.
        lead = lead.push(tip::bottom(
            t,
            container(
                text(ws.name.as_str())
                    .size(font_size::BODY)
                    .font(fonts::ui_semibold())
                    .wrapping(text::Wrapping::None),
            )
            .max_width(200)
            .clip(true)
            .padding(Padding {
                right: space::SM,
                ..Padding::ZERO
            }),
            format!("{}\n{}", ws.name, ws.cwd),
        ));
    }
    lead = lead.push(views(ctx, view, icon_views));

    // ---- The centre: what the focused pane is doing.
    let room = (width - 2.0 * 24.0).max(0.0) * 0.4;
    let centre = container(activity::view(&st.glances, ctx, ws, extras, room))
        .max_width(520)
        .width(Length::Fill);

    // ---- The trail.
    let mut trail = row![].spacing(2).align_y(Alignment::Center);
    trail = trail.push(crate::needs_you::button(&ctx.pkg.needs_you, ctx));
    trail = trail.push(crate::accounts::switcher(&ctx.pkg.accounts, ctx));
    if terminals {
        trail = trail.push(presets(st, ctx, ws));
        if !folded {
            trail = trail.push(
                row![
                    ui_button::icon_button(t, Icon::Tidy)
                        .tip(tr.t(Key::ToolbarTidyHint).into_owned())
                        .keys(keys(CommandId::PaneTidy))
                        .on_press_maybe(
                            (ws.panes.len() >= 2)
                                .then(|| { Message::Intent(Intent::Tidy(ws.id.clone())) })
                        ),
                    sep(t, if icon_views { 3.0 } else { 5.0 }),
                    ui_button::icon_button(t, Icon::Plus)
                        .tip(tr.t(Key::ToolbarNewTerminalHint).into_owned())
                        .keys(keys(CommandId::PaneSplit))
                        .on_press_maybe((ws.panes.len() < MAX_PANES).then(|| {
                            Message::Intent(Intent::SplitPane {
                                ws: ws.id.clone(),
                                near: None,
                                dir: None,
                            })
                        })),
                    ui_button::icon_button(t, Icon::Restart)
                        .tip(tr.t(Key::TopbarRestartAllHint).into_owned())
                        .keys(keys(CommandId::PaneRestartAll))
                        .on_press(Message::Intent(menus::restart_all(ws))),
                ]
                .spacing(2)
                .align_y(Alignment::Center)
                .padding(Padding {
                    left: space::XS,
                    ..Padding::ZERO
                }),
            );
        }
    }
    trail = trail.push(more(st, ctx, ws, terminals, folded, is_main, in_own_window));
    trail = trail.push(sep(t, if icon_views { 3.0 } else { 5.0 }));
    if let Some(kits) = kits_button(st, ctx, ws) {
        trail = trail.push(kits);
    }
    // TODO(P15): the 3D toggle (`reads::models`) waits for the preview store;
    // `Intent::TogglePreview` is what it sends.
    let models = reads::models(model, ws);
    if models.open {
        trail = trail.push(
            ui_button::toggle_button(t, Icon::Cube, models.shown)
                .tip(tr.t(Key::ModelToggleHint).into_owned())
                .keys(keys(CommandId::ViewModelPreview))
                .on_press(Message::Intent(Intent::TogglePreview)),
        );
    }
    trail = trail
        .push(
            ui_button::toggle_button(t, Icon::Board, settings.board_dock_open)
                .tip(tr.t(Key::BoardDockHint).into_owned())
                .keys(keys(CommandId::ViewBoardDock))
                .on_press(Message::Intent(Intent::ToggleDock)),
        )
        .push(
            ui_button::toggle_button(t, Icon::SidebarRight, settings.inspector_open)
                .tip(tr.t(Key::CmdViewInspector).into_owned())
                .keys(keys(CommandId::ViewInspector))
                .on_press(Message::Intent(Intent::ToggleInspector)),
        );

    row![
        container(lead).width(Length::Shrink),
        centre,
        container(trail).width(Length::Shrink)
    ]
    .spacing(space::XL)
    .padding(Padding {
        top: 0.0,
        right: space::MD,
        bottom: 0.0,
        left: space::XL,
    })
    .align_y(Alignment::Center)
    .height(Length::Fill)
    .into()
}

/// A hairline between two groups of controls.
fn sep<'a>(t: &'a Theme, margin: f32) -> Element<'a, Message> {
    container(
        container(Space::new())
            .width(1)
            .height(18)
            .style(move |_: &iced::Theme| container::Style {
                background: Some(iced::Background::Color(t.colors.line_strong)),
                ..container::Style::default()
            }),
    )
    .padding([0.0, margin])
    .into()
}

/// The views as a segmented control (`segmented--views`): the label of each,
/// or only its icon when the bar is narrow.
fn views<'a>(ctx: Ctx<'a>, current: View, icons_only: bool) -> Element<'a, Message> {
    let t = ctx.theme;
    let tr = ctx.i18n;
    const VIEWS: [(View, Icon, Key, Key, CommandId); 4] = [
        (
            View::Terminals,
            Icon::Terminal,
            Key::ViewTerminals,
            Key::CmdViewTerminals,
            CommandId::ViewTerminals,
        ),
        (
            View::Board,
            Icon::Board,
            Key::ViewBoard,
            Key::CmdViewBoard,
            CommandId::ViewBoard,
        ),
        (
            View::Memory,
            Icon::Bookmark,
            Key::ViewMemory,
            Key::CmdViewMemory,
            CommandId::ViewMemory,
        ),
        (
            View::Stats,
            Icon::Chart,
            Key::ViewStats,
            Key::CmdViewStats,
            CommandId::ViewStats,
        ),
    ];
    let segments = VIEWS.map(|(view, glyph, label, hint, command)| {
        let content: Element<'a, Message> = if icons_only {
            icon(glyph, 15.0).into()
        } else {
            text(tr.t(label).into_owned())
                .size(font_size::UI)
                .font(fonts::ui_medium())
                .into()
        };
        let segment = iced::widget::button(container(content).center_y(Length::Fill).center_x(
            if icons_only {
                Length::Fill
            } else {
                Length::Shrink
            },
        ))
        .padding(if icons_only {
            Padding::ZERO
        } else {
            Padding::from([0.0, 11.0])
        })
        .width(if icons_only {
            Length::Fixed(30.0)
        } else {
            Length::Shrink
        })
        .height(22)
        .style(style::button::tab(t, view == current))
        .on_press(Message::Intent(Intent::SetView(view)));
        tip::keys(
            t,
            segment,
            tr.t(hint).into_owned(),
            &reads::binding(&ctx, command),
        )
    });
    container(row(segments).spacing(2).align_y(Alignment::Center))
        .padding(2)
        .style(style::container::segmented_track(t))
        .into()
}

/// The layout presets as a segmented control of icons: the pane count says
/// which is on. A smaller one that would stop a busy agent asks first, from
/// the button that was pressed.
fn presets<'a>(st: &'a State, ctx: Ctx<'a>, ws: &'a Workspace) -> Element<'a, Message> {
    let t = ctx.theme;
    let tr = ctx.i18n;
    let asking = st.preset.as_ref().filter(|ask| {
        ask.ws == ws.id && ask.view == ctx.model.ui.view && !ctx.model.ui.confirm_reset
    });
    let buttons = LAYOUTS.map(|size| {
        let words = tr
            .t_with(Key::TopbarPresetHint, &args!("n" => size))
            .into_owned();
        let on = ws.panes.len() == usize::from(size);
        let segment = iced::widget::button(
            container(icon(menus::layout_icon(size), 16.0)).center(Length::Fill),
        )
        .padding(0)
        .width(28)
        .height(22)
        .style(style::button::tab(t, on))
        .on_press(Message::Toolbar(Msg::Preset(size)));
        let segment = tip::bottom(t, segment, words);
        let content = asking
            .filter(|ask| ask.size == size)
            .map(|ask| menus::preset_question(ctx, ask));
        layer::dropdown(segment, content)
            .tab_inside()
            .on_dismiss(Message::Toolbar(Msg::PresetCancel))
            .into()
    });
    container(row(buttons).spacing(2).align_y(Alignment::Center))
        .padding(2)
        .style(style::container::segmented_track(t))
        .into()
}

/// The « … » button, with the menu or the question that hangs off it. An armed
/// reset or stop opens its question from anywhere (the shortcut, the palette).
fn more<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ws: &'a Workspace,
    terminals: bool,
    folded: bool,
    can_detach: bool,
    in_own_window: bool,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let tr = ctx.i18n;
    let ui = &ctx.model.ui;
    let confirming = ui.confirm_reset || ui.confirm_stop_workers;
    let shown = st.more_open || confirming;
    let button = ui_button::toggle_button(t, Icon::More, shown)
        .tip(tr.t(Key::ToolbarMore).into_owned())
        .on_press(Message::Toolbar(Msg::ToggleMore));

    if ui.confirm_reset {
        return layer::dropdown(button, Some(menus::reset_question(ctx, ws)))
            .align_end()
            .tab_inside()
            .on_dismiss(Message::Toolbar(Msg::CloseMenus))
            .into();
    }
    if ui.confirm_stop_workers {
        return layer::dropdown(button, Some(menus::stop_question(ctx, ws)))
            .align_end()
            .tab_inside()
            .on_dismiss(Message::Toolbar(Msg::CloseMenus))
            .into();
    }
    let entries = st
        .more_open
        .then(|| menus::more_entries(ctx, ws, terminals, folded, can_detach, in_own_window));
    ui_menu::dropdown(t, button, entries)
        .align_end()
        .on_dismiss(Message::Toolbar(Msg::CloseMenus))
        .into()
}

/// The Kits button and its menu, only when a kit is enabled on the workspace.
fn kits_button<'a>(st: &'a State, ctx: Ctx<'a>, ws: &'a Workspace) -> Option<Element<'a, Message>> {
    let t = ctx.theme;
    let tr = ctx.i18n;
    let (tabs, loading) = reads::kit_tabs(ctx.model, ws)?;
    let open = st.kits_for.as_ref() == Some(&ws.id);
    let button = ui_button::toggle_button(t, Icon::Window, open)
        .tip(tr.t(Key::KitsTabMenuHint).into_owned())
        .on_press(Message::Toolbar(Msg::ToggleKits));
    let entries = open.then(|| menus::kits_entries(ctx, ws, &tabs, loading));
    Some(
        ui_menu::dropdown(t, button, entries)
            .align_end()
            .on_dismiss(Message::Toolbar(Msg::ToggleKits))
            .into(),
    )
}

#[cfg(test)]
mod tests;
