//! P7: the « … » menu of a pane header (`PaneMenu.tsx`, and the entries
//! `TerminalPane.tsx` gave it; inventory §2.1/0).
//!
//! What a pane can do that is not worth a button of its own in a header that
//! eight panes share: restart it, resume another session, split it; zoom,
//! its saved commands and its changes when the header folded those buttons
//! away; its part in the orchestration; then the Claude account it runs on
//! (`accounts::menu_items`).
//!
//! The menu opens with its first line highlighted, so the arrows work
//! straight away. Every line closes the menu and gives the keyboard back to
//! the pane's terminal before it acts, so whatever it opens takes the
//! keyboard after.

use iced::{Element, Subscription, Task};

use hangar_i18n::Key;
use hangar_ui::icon::Icon;
use hangar_ui::{button, layer, menu};

use crate::commands::CommandId;
use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::navigator::menu::set_off;
use crate::orchestration_ui::actions::pin_refusal;
use crate::palette::sources::workspace_of;
use crate::services::{CoreEvent, Services};
use crate::shell::{MenuEntry, Region};
use crate::state::types::{Pane, PaneRole, Workspace};
use crate::state::{Model, PaneId, Patch, SplitDir};

/// The menu's width bounds (`.pane-menu`).
const MIN_WIDTH: f32 = 232.0;
const MAX_WIDTH: f32 = 320.0;

/// Which of the header's buttons the pane header folded away for want of
/// room: their actions join the menu then. The pane header knows; it passes
/// this to [`view_folded`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Folded {
    pub commands: bool,
    pub diff: bool,
    pub zoom: bool,
}

/// The package's UI state; lives in `App.pkg.pane_menu`.
#[derive(Debug, Default)]
pub struct State {
    /// The pane whose menu is open, and what its header had folded then.
    open: Option<(PaneId, Folded)>,
}

#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::PaneMenu(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The « … » button: opens the pane's menu, or closes it.
    Toggle(PaneId, Folded),
    /// Escape, Tab, a press elsewhere.
    Close,
    /// A line: the menu closes, the terminal gets the keyboard back, then
    /// what the line does.
    Pick(PaneId, Box<Message>),
}

fn send(msg: Msg) -> Message {
    Message::PaneMenu(msg)
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    let _ = sv;
    match msg {
        Msg::Toggle(pane, folded) => {
            let again = st.open.as_ref().is_some_and(|(open, _)| open == &pane);
            st.open = if again { None } else { Some((pane, folded)) };
            Task::none()
        }
        Msg::Close => {
            let Some((pane, _)) = st.open.take() else {
                return Task::none();
            };
            focus_terminal(m, pane)
        }
        Msg::Pick(pane, action) => {
            st.open = None;
            focus_terminal(m, pane).chain(Task::done(*action))
        }
    }
}

/// Typing is what comes after a menu, whatever it did.
fn focus_terminal(m: &Model, pane: PaneId) -> Task<Message> {
    match workspace_of(m, &pane) {
        Some(ws) => Task::done(Message::Intent(Intent::FocusPane {
            ws: ws.id.clone(),
            pane,
            terminal: true,
        })),
        None => Task::none(),
    }
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    let _ = (st, m);
    Subscription::none()
}

/// The menu owns no region, so the shell never calls this.
pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    let _ = (st, m, region);
    None
}

pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    let _ = (m, sv);
    if let Intent::ClosePopovers(_) = intent {
        st.open = None;
    }
    None
}

pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    let _ = (st, m, sv, event);
    None
}

/// What a pane is to the orchestration of its workspace, for the last group
/// of its menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// It is the workspace's orchestrator: the menu offers to let go, and its
    /// settings.
    Orchestrator,
    /// It may become the orchestrator: the menu offers to pin it.
    Free,
}

/// The pane's part, by the orchestration panels' own rule (`pinRefusal`: one
/// orchestrator per workspace, Claude Code only, never a worker, never the
/// manager, never in the HQ). `None` for a pane the menu says nothing about.
pub fn part_in_orchestration(ws: &Workspace, pane: &Pane) -> Option<Part> {
    if pane.role == Some(PaneRole::Orchestrator) {
        return Some(Part::Orchestrator);
    }
    pin_refusal(ws, &pane.id).is_none().then_some(Part::Free)
}

/// A line that closes the menu, gives the terminal the keyboard back, then
/// sends `action`.
fn line(
    ctx: Ctx<'_>,
    pane: &PaneId,
    label: Key,
    icon: Icon,
    keys: Option<CommandId>,
    action: Message,
) -> menu::Entry<Message> {
    let mut item = menu::item(ctx.t(label))
        .icon(icon)
        .on_select(send(Msg::Pick(pane.clone(), Box::new(action))));
    if let Some(binding) = keys.and_then(|id| ctx.keymap.first_binding(id)) {
        item = item.shortcut(binding);
    }
    item.into()
}

/// The menu's lines for `pane`, with what the header folded.
pub fn entries(ctx: Ctx<'_>, pane: &PaneId, folded: Folded) -> Vec<menu::Entry<Message>> {
    let Some(ws) = workspace_of(ctx.model, pane) else {
        return Vec::new();
    };
    let Some(entry) = ws.panes.iter().find(|entry| &entry.id == pane) else {
        return Vec::new();
    };
    let intent = |intent| Message::Intent(intent);
    let ws_id = ws.id.clone();
    let mut entries = vec![line(
        ctx,
        pane,
        Key::PaneRestart,
        Icon::Restart,
        Some(CommandId::PaneRestart),
        intent(Intent::ReplacePane {
            ws: ws_id.clone(),
            pane: pane.clone(),
            patch: Patch::none(),
        }),
    )];
    if entry.agent.info().resumable {
        entries.push(line(
            ctx,
            pane,
            Key::CmdPaneSessions,
            Icon::Clock,
            Some(CommandId::PaneSessions),
            intent(Intent::OpenSessions {
                ws: ws_id.clone(),
                pane: pane.clone(),
            }),
        ));
    }
    entries.push(menu::separator());
    for (label, icon, keys, dir) in [
        (
            Key::CmdPaneSplitRight,
            Icon::SplitRight,
            CommandId::PaneSplitRight,
            SplitDir::Row,
        ),
        (
            Key::CmdPaneSplitDown,
            Icon::SplitDown,
            CommandId::PaneSplitDown,
            SplitDir::Col,
        ),
    ] {
        entries.push(line(
            ctx,
            pane,
            label,
            icon,
            Some(keys),
            intent(Intent::SplitPane {
                ws: ws_id.clone(),
                near: Some(pane.clone()),
                dir: Some(dir),
            }),
        ));
    }

    // Nothing to zoom out of, or onto, with a single pane.
    let zoomable = folded.zoom && ws.panes.len() > 1;
    if zoomable {
        let ui = &ctx.model.ui;
        let zoomed = ui.zoom_by_workspace.get(&ws.id).copied().unwrap_or(false)
            && ui.focus_by_workspace.get(&ws.id) == Some(pane);
        let (label, icon) = if zoomed {
            (Key::ZoomRestore, Icon::Collapse)
        } else {
            (Key::PaneMenuZoom, Icon::Expand)
        };
        // The zoom is the focused pane's, and every line focuses its pane
        // first: zooming from this menu zooms onto this pane.
        let action = intent(Intent::SetZoom {
            ws: ws_id.clone(),
            on: !zoomed,
        });
        entries.push(menu::separator());
        entries.push(line(
            ctx,
            pane,
            label,
            icon,
            Some(CommandId::PaneZoom),
            action,
        ));
    }
    if folded.commands {
        if !zoomable {
            entries.push(menu::separator());
        }
        let open = Message::SavedCommands(crate::saved_commands::Msg::OpenMenu {
            pane: pane.clone(),
            from_more: true,
        });
        entries.push(line(ctx, pane, Key::CommandsMenu, Icon::Bolt, None, open));
    }
    if folded.diff {
        if !zoomable && !folded.commands {
            entries.push(menu::separator());
        }
        entries.push(line(
            ctx,
            pane,
            Key::PaneMenuDiff,
            Icon::Diff,
            None,
            intent(Intent::OpenDiff {
                ws: ws_id.clone(),
                pane: Some(pane.clone()),
            }),
        ));
    }

    // Last, a group of its own: what the pane is to the workspace rather
    // than what it does.
    let part = part_in_orchestration(ws, entry);
    if part == Some(Part::Orchestrator) {
        entries.push(menu::separator());
        entries.push(line(
            ctx,
            pane,
            Key::OrchUnpin,
            Icon::Orchestrator,
            Some(CommandId::PaneUnpinOrchestrator),
            intent(Intent::PinOrchestrator {
                ws: ws_id.clone(),
                pane: pane.clone(),
                pinned: false,
            }),
        ));
        entries.push(line(
            ctx,
            pane,
            Key::OrchSettingsMenu,
            Icon::Sliders,
            Some(CommandId::WorkspaceOrchestration),
            intent(Intent::OpenOrchestration(ws_id.clone())),
        ));
    } else if part == Some(Part::Free) {
        entries.push(menu::separator());
        entries.push(line(
            ctx,
            pane,
            Key::OrchPin,
            Icon::Orchestrator,
            Some(CommandId::PanePinOrchestrator),
            intent(Intent::PinOrchestrator {
                ws: ws_id.clone(),
                pane: pane.clone(),
                pinned: true,
            }),
        ));
    }

    // The Claude account of the pane, which says nothing for a pane that
    // does not run Claude Code. The accounts package starts its block with its
    // own separator and heading, so none is added here: a second one would
    // draw twice.
    entries.extend(
        set_off(crate::accounts::menu_items(ctx, &ws.id, Some(pane)))
            .into_iter()
            .map(|entry| account_line(pane, entry)),
    );
    entries
}

/// One of the accounts package's lines, in this menu: picking it closes the
/// menu too (`onDone`).
fn account_line(pane: &PaneId, entry: MenuEntry) -> menu::Entry<Message> {
    match entry {
        MenuEntry::Item {
            label,
            icon,
            danger,
            checked,
            disabled,
            on_select,
        } => {
            let mut item = menu::item(label);
            if let Some(icon) = icon.and_then(Icon::from_name) {
                item = item.icon(icon);
            }
            if danger {
                item = item.danger();
            }
            if let Some(checked) = checked {
                item = item.checked(checked);
            }
            match disabled {
                Some(reason) if reason.is_empty() => item.disabled().into(),
                Some(reason) => item.disabled_because(reason).into(),
                None => item
                    .on_select(send(Msg::Pick(pane.clone(), Box::new(on_select))))
                    .into(),
            }
        }
        MenuEntry::Heading(label) => menu::heading(label),
        MenuEntry::Separator => menu::separator(),
    }
}

/// The « … » menu of a pane header (embedded by the pane header), with none
/// of its buttons folded.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>, pane: &'a PaneId) -> Element<'a, Message> {
    view_folded(st, ctx, pane, Folded::default())
}

/// The « … » button and its menu, for a header that folded `folded` away.
pub fn view_folded<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    pane: &'a PaneId,
    folded: Folded,
) -> Element<'a, Message> {
    let open = st
        .open
        .as_ref()
        .filter(|(open, _)| open == pane)
        .map(|(_, folded)| *folded);
    let commands = crate::saved_commands::open_from_more(&ctx.pkg.saved_commands, pane);
    let anchor = button::icon_button(ctx.theme, Icon::More)
        .tip(ctx.t(Key::PaneMore))
        .toggled(open.is_some() || commands)
        .small()
        .on_press(send(Msg::Toggle(pane.clone(), folded)));
    // The saved commands hang off this button when the header folded the ⚡
    // one away.
    if commands {
        return crate::saved_commands::menu_under(&ctx.pkg.saved_commands, ctx, anchor, pane);
    }
    let content = open.map(|folded| {
        menu::menu(ctx.theme, entries(ctx, pane, folded))
            .width_between(MIN_WIDTH, MAX_WIDTH)
            .from_keyboard(true)
            .into()
    });
    layer::dropdown(anchor, content)
        .align_end()
        .on_dismiss(send(Msg::Close))
        .into()
}

#[cfg(test)]
mod tests;
