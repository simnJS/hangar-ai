//! Writing a saved command down without a trip through the settings
//! (`CommandDialog.tsx`).
//!
//! Typed by hand rather than picked out of the scrollback: the last line read
//! back from a terminal also holds the prompt, the agent's own output and
//! whatever the shell redrew, so what it would save is often not what ran.

use iced::widget::{Space, column, container, row};
use iced::{Alignment, Element, Length, Task};

use hangar_i18n::{Key, Translator};
use hangar_theme::tokens::{font_size, space};
use hangar_ui::{button, dialog, input, text as typography};

use super::{Msg, locate};
use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::shell::Overlay;
use crate::state::store::SavedCommandDraft;
use crate::state::types::SavedCommand;
use crate::state::{CommandScope, Model, PaneId, Patch, StoreAction, WsId};

/// The width of the labels' column.
const LABEL_WIDTH: f32 = 112.0;

/// The dialog's fields, while `Overlay::CommandDialog` is open.
#[derive(Debug, Clone, PartialEq)]
pub struct Form {
    ws: WsId,
    pane: PaneId,
    /// What the overlay asked to edit, as it named it.
    edit: Option<String>,
    /// The command being edited and where it is kept; `None` makes one.
    editing: Option<(String, CommandScope)>,
    label: String,
    command: String,
    auto_run: bool,
    broadcast: bool,
    scope: CommandScope,
    words: Words,
}

/// The texts the fields borrow, translated once when the dialog opens.
#[derive(Debug, Clone, PartialEq)]
struct Words {
    command_placeholder: String,
    label_placeholder: String,
    scope_workspace: String,
    scope_global: String,
}

#[derive(Debug, Clone)]
pub enum DialogMsg {
    Command(String),
    Label(String),
    AutoRun(bool),
    Broadcast(bool),
    Scope(CommandScope),
    Submit,
    Cancel,
}

impl Form {
    fn new(m: &Model, ws: &WsId, pane: &PaneId, edit: Option<&String>) -> Form {
        let i18n = Translator::new(m.locale());
        let found = edit.and_then(|id| {
            let workspace = m.workspace(ws)?;
            locate(workspace, m.settings(), id)
        });
        let words = Words {
            command_placeholder: i18n.t(Key::CommandsCommandPlaceholder).into_owned(),
            label_placeholder: i18n.t(Key::CommandsLabelPlaceholder).into_owned(),
            scope_workspace: i18n.t(Key::CommandsScopeWorkspace).into_owned(),
            scope_global: i18n.t(Key::CommandsScopeGlobal).into_owned(),
        };
        match found {
            Some((scope, command)) => Form {
                ws: ws.clone(),
                pane: pane.clone(),
                edit: edit.cloned(),
                editing: Some((command.id.clone(), scope)),
                label: command.label,
                command: command.command,
                auto_run: command.auto_run,
                broadcast: command.broadcast,
                scope,
                words,
            },
            // Saving a command is asking for it to be run; the ones finished
            // by hand are the exception.
            None => Form {
                ws: ws.clone(),
                pane: pane.clone(),
                edit: edit.cloned(),
                editing: None,
                label: String::new(),
                command: String::new(),
                auto_run: true,
                broadcast: false,
                scope: CommandScope::Workspace,
                words,
            },
        }
    }

    fn is_for(&self, ws: &WsId, pane: &PaneId, edit: Option<&String>) -> bool {
        &self.ws == ws && &self.pane == pane && self.edit.as_ref() == edit
    }

    /// The command as it is saved, or `None` while there is none.
    fn draft(&self) -> Option<SavedCommandDraft> {
        let command = self.command.trim();
        // An empty label leaves the command itself to name the entry, which
        // for a short one reads better than anything a placeholder suggests.
        (!command.is_empty()).then(|| SavedCommandDraft {
            label: self.label.trim().to_string(),
            command: command.to_string(),
            auto_run: self.auto_run,
            broadcast: self.broadcast,
        })
    }
}

/// Sets the form up when the dialog opens, or opens on another command, and
/// drops it when the dialog goes.
pub fn follow(form: &mut Option<Form>, m: &Model) {
    let wanted =
        m.ui.overlays
            .iter()
            .rev()
            .find_map(|overlay| match overlay {
                Overlay::CommandDialog { ws, pane, edit } => Some((ws, pane, edit.as_ref())),
                _ => None,
            });
    match wanted {
        None => *form = None,
        Some((ws, pane, edit)) => {
            if form
                .as_ref()
                .is_none_or(|form| !form.is_for(ws, pane, edit))
            {
                *form = Some(Form::new(m, ws, pane, edit));
            }
        }
    }
}

pub fn update(form: &mut Option<Form>, m: &mut Model, msg: DialogMsg) -> Task<Message> {
    let Some(open) = form else {
        return Task::none();
    };
    match msg {
        DialogMsg::Command(text) => open.command = text,
        DialogMsg::Label(text) => open.label = text,
        DialogMsg::AutoRun(on) => open.auto_run = on,
        DialogMsg::Broadcast(on) => open.broadcast = on,
        DialogMsg::Scope(scope) => open.scope = scope,
        DialogMsg::Submit => {
            let Some(draft) = open.draft() else {
                return Task::none();
            };
            save(m, open, draft);
            *form = None;
            return close(m);
        }
        DialogMsg::Cancel => {
            *form = None;
            return close(m);
        }
    }
    Task::none()
}

fn save(m: &mut Model, form: &Form, draft: SavedCommandDraft) {
    let ws = form.ws.clone();
    match &form.editing {
        None => {
            m.apply(StoreAction::AddSavedCommand {
                scope: form.scope,
                ws,
                draft,
            });
        }
        Some((id, kept)) => {
            // Patched where it is, then moved if the scope changed: the move
            // carries the text the patch just wrote.
            m.apply(StoreAction::UpdateSavedCommand {
                scope: *kept,
                ws: ws.clone(),
                id: id.clone(),
                patch: Patch::new(move |command: &mut SavedCommand| {
                    command.label = draft.label.clone();
                    command.command = draft.command.clone();
                    command.auto_run = draft.auto_run;
                    command.broadcast = draft.broadcast;
                }),
            });
            if form.scope != *kept {
                m.apply(StoreAction::MoveSavedCommand {
                    from: *kept,
                    to: form.scope,
                    ws,
                    id: id.clone(),
                });
            }
        }
    }
}

/// Closes the dialog, if it is still the overlay on top: Escape may have
/// closed it already.
fn close(m: &Model) -> Task<Message> {
    match m.ui.overlays.last() {
        Some(Overlay::CommandDialog { .. }) => Task::done(Message::Intent(Intent::CloseOverlay)),
        _ => Task::none(),
    }
}

fn send(msg: DialogMsg) -> Message {
    Message::SavedCommands(Msg::Dialog(msg))
}

/// A row of the form: its label at the left, the field and its hint at the
/// right (`.form-grid`).
fn field_row<'a>(
    ctx: Ctx<'a>,
    label: Key,
    field: Element<'a, Message>,
    hint: Option<Key>,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let mut right = column![field].spacing(space::XS).width(Length::Fill);
    if let Some(hint) = hint {
        right = right.push(typography::hint(t, ctx.t(hint)));
    }
    row![
        container(typography::label(t, ctx.t(label)).size(font_size::UI))
            .width(LABEL_WIDTH)
            .padding(iced::Padding {
                top: space::XS,
                ..iced::Padding::ZERO
            }),
        right,
    ]
    .spacing(space::LG)
    .align_y(Alignment::Start)
    .into()
}

pub fn view<'a>(form: Option<&'a Form>, ctx: Ctx<'a>) -> Element<'a, Message> {
    let Some(form) = form else {
        return Space::new().into();
    };
    let t = ctx.theme;
    let trimmed = form.command.trim();

    let command = input::text_field(t, &form.command)
        .placeholder(&form.words.command_placeholder)
        .mono()
        .on_input(|text| send(DialogMsg::Command(text)))
        .on_submit(send(DialogMsg::Submit));
    let label_placeholder = if trimmed.is_empty() {
        form.words.label_placeholder.as_str()
    } else {
        trimmed
    };
    let label = input::text_field(t, &form.label)
        .placeholder(label_placeholder)
        .on_input(|text| send(DialogMsg::Label(text)))
        .on_submit(send(DialogMsg::Submit));
    let auto_run = input::switch(t, form.auto_run).on_toggle(|on| send(DialogMsg::AutoRun(on)));
    let broadcast = input::switch(t, form.broadcast).on_toggle(|on| send(DialogMsg::Broadcast(on)));
    let scope = input::segmented(
        t,
        &[
            (CommandScope::Workspace, form.words.scope_workspace.as_str()),
            (CommandScope::Global, form.words.scope_global.as_str()),
        ],
        &form.scope,
        |scope| send(DialogMsg::Scope(scope)),
    );

    let body = column![
        // The keymap goes first: Ctrl+Shift+Enter in a field splits the
        // pane behind the dialog and must not also save it.
        field_row(
            ctx,
            Key::CommandsCommand,
            crate::keymap::guard(ctx, command),
            None
        ),
        field_row(
            ctx,
            Key::CommandsLabel,
            crate::keymap::guard(ctx, label),
            Some(Key::CommandsLabelHint)
        ),
        field_row(
            ctx,
            Key::CommandsAutoRun,
            auto_run.into(),
            Some(Key::CommandsAutoRunHint)
        ),
        field_row(
            ctx,
            Key::CommandsBroadcast,
            broadcast.into(),
            Some(Key::CommandsBroadcastHint)
        ),
        field_row(ctx, Key::CommandsScope, scope, Some(Key::CommandsScopeHint)),
    ]
    .spacing(space::XL);

    let title = if form.editing.is_some() {
        Key::CommandsTitleEdit
    } else {
        Key::CommandsTitle
    };
    let save = button::primary(t, ctx.t(Key::CommandsSave))
        .on_press_maybe((!trimmed.is_empty()).then(|| send(DialogMsg::Submit)));
    let panel = dialog::dialog(t, ctx.t(title), body)
        .action(button::ghost(t, ctx.t(Key::CreateCancel)).on_press(send(DialogMsg::Cancel)))
        .action(save)
        .close_tip(ctx.t(Key::ModalClose))
        .on_close(send(DialogMsg::Cancel));
    dialog::over(
        t,
        Space::new().width(Length::Fill).height(Length::Fill),
        Some(panel),
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> Form {
        Form {
            ws: "w".into(),
            pane: "p".into(),
            edit: None,
            editing: None,
            label: String::new(),
            command: String::new(),
            auto_run: true,
            broadcast: false,
            scope: CommandScope::Workspace,
            words: Words {
                command_placeholder: String::new(),
                label_placeholder: String::new(),
                scope_workspace: String::new(),
                scope_global: String::new(),
            },
        }
    }

    #[test]
    fn nothing_is_saved_while_the_command_is_empty() {
        let mut open = form();
        open.command = "   ".into();
        assert!(open.draft().is_none());
    }

    #[test]
    fn what_is_saved_is_trimmed() {
        let mut open = form();
        open.command = "  pnpm test  ".into();
        open.label = " Tests ".into();
        let draft = open.draft().unwrap();
        assert_eq!(draft.command, "pnpm test");
        assert_eq!(draft.label, "Tests");
        assert!(draft.auto_run);
        assert!(!draft.broadcast);
    }
}
