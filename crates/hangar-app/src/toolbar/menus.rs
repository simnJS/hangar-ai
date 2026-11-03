//! The toolbar's menus and the questions it asks before it acts
//! (`Toolbar.tsx`, `ToolbarMenu.tsx`): the « … » menu, the Kits menu, and the
//! confirmations of resetting the agents, stopping the workers and closing
//! panes with a layout preset.

use iced::widget::{Space, column, container, row, text};
use iced::{Alignment, Element, Length};

use hangar_i18n::{Key, Translator, args};
use hangar_theme::tokens::{font_size, radius, space};
use hangar_theme::{Theme, fonts, style};
use hangar_ui::menu::{self, Entry};
use hangar_ui::{Icon, glyph};

use super::{Msg, PresetAsk};
use crate::commands::CommandId;
use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::navigator::reads;
use crate::state::AgentActivity;
use crate::state::types::{AgentId, MAX_PANES, Workspace};

/// The layout presets the toolbar offers (`LAYOUTS`).
pub const LAYOUTS: [u8; 4] = [1, 2, 4, 8];

/// The icon of each preset (`LAYOUT_ICONS`).
pub fn layout_icon(size: u8) -> Icon {
    match size {
        1 => Icon::Layout1,
        2 => Icon::Layout2,
        4 => Icon::Layout4,
        _ => Icon::Layout8,
    }
}

fn pick(intent: Intent) -> Message {
    Message::Toolbar(Msg::Pick(vec![intent]))
}

/// Panes running an agent: what « reset agents » would start afresh.
pub fn agent_pane_count(ws: &Workspace) -> usize {
    ws.panes
        .iter()
        .filter(|pane| pane.agent != AgentId::Shell)
        .count()
}

/// What the « … » menu lists (`Toolbar.tsx`, "More menu").
///
/// `folded`: the terminal actions did not fit in the bar and are listed here.
pub fn more_entries(
    ctx: Ctx<'_>,
    ws: &Workspace,
    terminals: bool,
    folded: bool,
    can_detach: bool,
    in_own_window: bool,
) -> Vec<Entry<Message>> {
    let tr = ctx.i18n;
    let keys = |id| reads::binding(&ctx, id);
    let mut entries: Vec<Entry<Message>> = Vec::new();

    if folded && terminals {
        let new = menu::item(tr.t(Key::ToolbarNewTerminal).into_owned())
            .icon(Icon::Plus)
            .shortcut(keys(CommandId::PaneSplit))
            .on_select(pick(Intent::SplitPane {
                ws: ws.id.clone(),
                near: None,
                dir: None,
            }));
        entries.push(if ws.panes.len() >= MAX_PANES {
            new.disabled().into()
        } else {
            new.into()
        });
        let tidy = menu::item(tr.t(Key::TopbarTidy).into_owned())
            .icon(Icon::Tidy)
            .shortcut(keys(CommandId::PaneTidy))
            .on_select(pick(Intent::Tidy(ws.id.clone())));
        entries.push(if ws.panes.len() < 2 {
            tidy.disabled().into()
        } else {
            tidy.into()
        });
        entries.push(
            menu::item(tr.t(Key::TopbarRestartAll).into_owned())
                .icon(Icon::Restart)
                .shortcut(keys(CommandId::PaneRestartAll))
                .on_select(pick(restart_all(ws)))
                .into(),
        );
        entries.push(menu::separator());
    }

    if can_detach || in_own_window {
        if can_detach {
            entries.push(
                menu::item(tr.t(Key::ToolbarDetach).into_owned())
                    .icon(Icon::Window)
                    .on_select(pick(Intent::Detach(ws.id.clone())))
                    .into(),
            );
        }
        if in_own_window {
            entries.push(
                menu::item(tr.t(Key::WindowReattach).into_owned())
                    .icon(Icon::WindowReturn)
                    .on_select(pick(Intent::Reattach(ws.id.clone())))
                    .into(),
            );
        }
        entries.push(menu::separator());
    }

    entries.push(
        menu::item(tr.t(Key::ToolbarPalette).into_owned())
            .icon(Icon::Search)
            .shortcut(keys(CommandId::ViewPalette))
            .on_select(pick(Intent::TogglePalette))
            .into(),
    );

    if crate::needs_you::model::is_orchestrated(ws) {
        entries.push(menu::separator());
        entries.push(
            menu::item(tr.t(Key::OrchSettingsMenu).into_owned())
                .icon(Icon::Orchestrator)
                .shortcut(keys(CommandId::WorkspaceOrchestration))
                .on_select(pick(Intent::OpenOrchestration(ws.id.clone())))
                .into(),
        );
        if worker_count(ws) > 0 {
            entries.push(
                menu::item(tr.t(Key::OrchStopMenu).into_owned())
                    .icon(Icon::Stop)
                    .danger()
                    .shortcut(keys(CommandId::WorkspaceStopWorkers))
                    .on_select(Message::Toolbar(Msg::Pick(vec![
                        // One question at a time: stopping the workers takes
                        // over from a reset that was asking.
                        Intent::ArmResetAgents(false),
                        Intent::ArmStopWorkers(true),
                    ])))
                    .into(),
            );
        }
    }

    // Last, on its own, where the menu's first highlight and a stray Enter
    // never land: it throws away every conversation.
    entries.push(menu::separator());
    let reset = menu::item(tr.t(Key::ToolbarResetAll).into_owned())
        .icon(Icon::Reset)
        .danger()
        .shortcut(keys(CommandId::PaneResetAll));
    entries.push(if agent_pane_count(ws) == 0 {
        reset
            .disabled_because(tr.t(Key::TopbarResetAllNone).into_owned())
            .into()
    } else {
        reset
            .on_select(Message::Toolbar(Msg::Pick(vec![
                Intent::ArmStopWorkers(false),
                Intent::ArmResetAgents(true),
            ])))
            .into()
    });
    entries
}

/// Every pane of the workspace, restarted where it stands (`onRestartAll`).
pub fn restart_all(ws: &Workspace) -> Intent {
    Intent::RestartPanes {
        ws: ws.id.clone(),
        panes: ws.panes.iter().map(|pane| pane.id.clone()).collect(),
    }
}

/// Panes with a worker role (`workersOf`).
pub fn worker_count(ws: &Workspace) -> usize {
    ws.panes
        .iter()
        .filter(|pane| pane.role == Some(crate::state::types::PaneRole::Worker))
        .count()
}

/// A question asked in a popover: a title, a sentence, and the two answers,
/// the one that loses nothing first.
fn question<'a>(
    t: &'a Theme,
    title: String,
    body: String,
    extra: Option<Element<'a, Message>>,
    cancel: (String, Message),
    confirm: (String, Message),
) -> Element<'a, Message> {
    let mut content = column![
        text(title).size(font_size::BODY).font(fonts::ui_semibold()),
        text(body)
            .size(font_size::UI)
            .style(style::text::secondary(t))
            .width(Length::Fill),
    ]
    .spacing(space::XS)
    .width(300);
    if let Some(extra) = extra {
        content = content.push(extra);
    }
    let actions = row![
        Space::new().width(Length::Fill),
        hangar_ui::button::ghost(t, cancel.0).on_press(cancel.1),
        hangar_ui::button::danger(t, confirm.0).on_press(confirm.1),
    ]
    .spacing(space::SM)
    .align_y(Alignment::Center);
    menu::panel(
        t,
        content
            .push(container(actions).padding(iced::Padding {
                top: space::MD,
                ..iced::Padding::ZERO
            }))
            .padding(2),
    )
}

/// « Reset N agents? » (`toolbar.resetTitle`).
pub fn reset_question<'a>(ctx: Ctx<'a>, ws: &Workspace) -> Element<'a, Message> {
    let tr = ctx.i18n;
    let n = agent_pane_count(ws);
    question(
        ctx.theme,
        tr.t_with(Key::ToolbarResetTitle, &args!("n" => n))
            .into_owned(),
        tr.t(Key::ToolbarResetBody).into_owned(),
        None,
        (
            tr.t(Key::CreateCancel).into_owned(),
            Message::Intent(Intent::ArmResetAgents(false)),
        ),
        (
            tr.t_with(Key::TopbarResetAllConfirm, &args!("n" => n))
                .into_owned(),
            Message::Intent(Intent::ResetAgents(ws.id.clone())),
        ),
    )
}

/// « Stop N workers? » (`orch.stopTitle`).
pub fn stop_question<'a>(ctx: Ctx<'a>, ws: &Workspace) -> Element<'a, Message> {
    let tr = ctx.i18n;
    let n = worker_count(ws);
    question(
        ctx.theme,
        tr.t_with(Key::OrchStopTitle, &args!("n" => n)).into_owned(),
        tr.t(Key::OrchStopBody).into_owned(),
        None,
        (
            tr.t(Key::CreateCancel).into_owned(),
            Message::Intent(Intent::ArmStopWorkers(false)),
        ),
        (
            tr.t_with(Key::OrchStopConfirm, &args!("n" => n))
                .into_owned(),
            Message::Intent(Intent::StopWorkers(ws.id.clone())),
        ),
    )
}

/// « Ben and Cleo » (`toolbar.presetNames`): the names of the panes a preset
/// would close, in a sentence.
pub fn name_list(tr: Translator, names: &[String]) -> String {
    match names {
        [] => String::new(),
        [only] => only.clone(),
        [init @ .., last] => tr
            .t_with(
                Key::ToolbarPresetNames,
                &args!("names" => init.join(", "), "last" => last.as_str()),
            )
            .into_owned(),
    }
}

/// A preset asking first: the panes it would stop mid-work, one per line.
pub fn preset_question<'a>(ctx: Ctx<'a>, ask: &'a PresetAsk) -> Element<'a, Message> {
    let t = ctx.theme;
    let tr = ctx.i18n;
    let n = ask.closing.len();
    let busy = column(ask.busy.iter().map(|(name, activity)| {
        let word = match activity {
            AgentActivity::Working => Key::PaneStateWorking,
            AgentActivity::Waiting => Key::PaneStateWaiting,
            AgentActivity::Yours => Key::PaneStateYours,
            AgentActivity::Idle => Key::PaneStateIdle,
        };
        row![
            glyph::state(t, reads::activity_glyph(*activity), 7.0),
            text(name.as_str())
                .size(font_size::UI)
                .font(fonts::ui_semibold()),
            text(tr.t(word).into_owned())
                .size(font_size::UI)
                .style(style::text::secondary(t)),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center)
        .height(20)
        .into()
    }))
    .spacing(2);
    let list = container(busy)
        .padding([6.0, 8.0])
        .width(Length::Fill)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(t.colors.line)),
            border: iced::Border {
                radius: radius::SM.into(),
                ..iced::Border::default()
            },
            ..container::Style::default()
        });
    question(
        t,
        tr.t_with(
            Key::ToolbarPresetTitle,
            &args!("names" => name_list(tr, &ask.closing_names())),
        )
        .into_owned(),
        tr.tn(Key::ToolbarPresetBody, n, &[]).into_owned(),
        Some(list.into()),
        (
            tr.t(Key::CreateCancel).into_owned(),
            Message::Toolbar(Msg::PresetCancel),
        ),
        (
            tr.tn(Key::ToolbarPresetConfirm, n, &[]).into_owned(),
            Message::Toolbar(Msg::PresetConfirm),
        ),
    )
}

/// The Kits menu (`kitsTab.menu`): one line per tab, or why there is none.
pub fn kits_entries(
    ctx: Ctx<'_>,
    ws: &Workspace,
    tabs: &[reads::KitTab],
    loading: bool,
) -> Vec<Entry<Message>> {
    let tr = ctx.i18n;
    if tabs.is_empty() {
        let key = if loading {
            Key::KitsTabMenuLoading
        } else {
            Key::KitsTabMenuNone
        };
        return vec![menu::item(tr.t(key).into_owned()).disabled().into()];
    }
    tabs.iter()
        .map(|tab| {
            menu::item(tab.title.clone())
                .icon(Icon::Window)
                .shortcut(tab.kit.clone())
                .on_select(Message::Toolbar(Msg::Pick(vec![Intent::OpenKitTab {
                    ws: ws.id.clone(),
                    kit: tab.kit_id.clone(),
                    tab: tab.tab_id.clone(),
                }])))
                .into()
        })
        .collect()
}
