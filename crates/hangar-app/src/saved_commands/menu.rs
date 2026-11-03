//! The saved commands of one pane, hanging off the ⚡ button of its header
//! (`CommandMenu.tsx`).
//!
//! Grouped by where each is kept, the workspace's then the global ones; a
//! row is the command's name with the command after it in the mono face, or
//! the command alone, a `⇉` for one that goes to every pane, and its
//! `command.runN` keys when the person bound them (they ship unbound, and
//! promising a key nobody set would be worse than saying nothing). The edit
//! and remove buttons take the keys' place while a row is highlighted.

use iced::Element;

use hangar_i18n::Key;
use hangar_ui::icon::Icon;
use hangar_ui::{button, layer, menu};

use super::{Msg, State};
use crate::ctx::Ctx;
use crate::message::Message;
use crate::palette::sources::{COMMAND_RUN, workspace_of};
use crate::state::{CommandScope, PaneId};

/// The menu's width bounds, a pane menu's (`.cmd-menu`).
pub const MIN_WIDTH: f32 = 232.0;
pub const MAX_WIDTH: f32 = 320.0;

fn send(msg: Msg) -> Message {
    Message::SavedCommands(msg)
}

/// The lines of a pane's saved-commands menu.
pub fn menu_entries(ctx: Ctx<'_>, pane: &PaneId) -> Vec<menu::Entry<Message>> {
    let Some(ws) = workspace_of(ctx.model, pane) else {
        return Vec::new();
    };
    let settings = ctx.model.settings();
    let index = ctx.keymap;
    let mut entries = Vec::new();
    let mut rank = 0;
    for (scope, list, heading) in [
        (
            CommandScope::Workspace,
            &ws.saved_commands,
            Key::CommandsScopeWorkspace,
        ),
        (
            CommandScope::Global,
            &settings.saved_commands,
            Key::CommandsScopeGlobal,
        ),
    ] {
        if list.is_empty() {
            continue;
        }
        entries.push(menu::heading(ctx.t(heading)));
        for command in list {
            let tag = if command.broadcast { " ⇉" } else { "" };
            // An entry named after its own command says it once, not twice.
            let label = command.label.trim();
            let mut line = if label.is_empty() {
                menu::item(command.command.clone())
                    .mono()
                    .detail(tag.trim())
            } else {
                menu::item(label).detail(format!("{}{tag}", command.command))
            };
            if let Some(binding) = COMMAND_RUN
                .get(rank)
                .and_then(|id| index.first_binding(*id))
            {
                line = line.shortcut(binding);
            }
            let id = command.id.clone();
            line = line
                .on_select(send(Msg::Run {
                    pane: pane.clone(),
                    scope,
                    id: id.clone(),
                }))
                .action(
                    Icon::Pencil,
                    ctx.t(Key::CommandsEdit),
                    send(Msg::Edit {
                        pane: pane.clone(),
                        id: id.clone(),
                    }),
                )
                .danger_action(
                    Icon::Close,
                    ctx.t(Key::CommandsRemove),
                    send(Msg::Remove {
                        pane: pane.clone(),
                        scope,
                        id,
                    }),
                );
            entries.push(line.into());
            rank += 1;
        }
    }
    if entries.is_empty() {
        // With nothing saved yet, a line says what the menu is for, rather
        // than leaving a lone « New command… » to explain itself.
        entries.push(menu::item(ctx.t(Key::CommandsEmpty)).disabled().into());
    }
    entries.push(menu::separator());
    entries.push(
        menu::item(ctx.t(Key::CommandsAdd))
            .icon(Icon::Plus)
            .on_select(send(Msg::Add(pane.clone())))
            .into(),
    );
    entries
}

/// The menu as a layer over `anchor`, open while `open`.
pub fn dropdown<'a>(
    ctx: Ctx<'a>,
    anchor: impl Into<Element<'a, Message>>,
    pane: &PaneId,
    open: bool,
) -> Element<'a, Message> {
    let content = open.then(|| {
        menu::menu(ctx.theme, menu_entries(ctx, pane))
            .width_between(MIN_WIDTH, MAX_WIDTH)
            .into()
    });
    layer::dropdown(anchor, content)
        .align_end()
        .on_dismiss(send(Msg::CloseMenu))
        .into()
}

/// The ⚡ button of a pane header, with its menu under it.
pub fn bolt<'a>(st: &'a State, ctx: Ctx<'a>, pane: &'a PaneId) -> Element<'a, Message> {
    let open = st
        .menu
        .as_ref()
        .is_some_and(|open| &open.pane == pane && !open.from_more);
    let anchor = button::icon_button(ctx.theme, Icon::Bolt)
        .tip(ctx.t(Key::CommandsMenu))
        .toggled(open)
        .small()
        .on_press(send(Msg::ToggleMenu(pane.clone())));
    dropdown(ctx, anchor, pane, open)
}
