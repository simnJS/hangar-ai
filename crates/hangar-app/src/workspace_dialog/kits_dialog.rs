//! The kits of a workspace that already exists (`WorkspaceKitsDialog`): the
//! same section as in the creation dialog, on its own, with what only an
//! existing workspace allows (running a kit's setup commands in a pane of it,
//! restarting its Claude Code panes), then the worktree hooks. Changes are
//! saved as they are accepted.
//!
//! Opened from the navigator's menu and from Settings › Kits, "Kits per
//! workspace", over the settings when they are open: Escape closes this one
//! only (the shell closes the overlay in front), and the activation view,
//! which sits above it, closes first.

use hangar_i18n::{Key, args};
use hangar_theme::tokens;
use hangar_ui::{button, dialog};
use iced::widget::{Space, column};
use iced::{Element, Length};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::state::{Model, WsId};

use super::Msg;
use super::hooks::{self, HooksDraft};

/// The dialog's own state: the fields as typed.
#[derive(Debug)]
pub struct KitsDialog {
    pub ws: WsId,
    pub hooks: HooksDraft,
}

impl KitsDialog {
    /// The dialog for `ws`, its hooks in the fields. `None` when the
    /// workspace is gone.
    pub fn open(m: &Model, ws: &WsId) -> Option<KitsDialog> {
        let workspace = m.workspace(ws)?;
        Some(KitsDialog {
            ws: ws.clone(),
            hooks: HooksDraft::of(workspace.worktree_hooks.as_ref()),
        })
    }
}

/// What the fields hold before the dialog's state is made.
static NO_HOOKS: HooksDraft = HooksDraft {
    post_create: String::new(),
    pre_remove: String::new(),
};

pub fn view<'a>(state: Option<&'a KitsDialog>, ctx: Ctx<'a>, ws: &'a WsId) -> Element<'a, Message> {
    let t = ctx.theme;
    let Some(workspace) = ctx.model.workspace(ws) else {
        return Space::new().width(0).height(0).into();
    };
    let draft = state.map_or(&NO_HOOKS, |state| &state.hooks);
    let title = ctx
        .i18n
        .t_with(
            Key::KitsManageEditTitle,
            &args!("name" => workspace.name.as_str()),
        )
        .into_owned();

    let body = column![
        crate::kits::workspace_section(&ctx.pkg.kits, ctx, Some(ws)),
        hangar_ui::text::hint(t, ctx.t(Key::WorktreesHooksTitle)),
        hooks::view(ctx, draft, None, |field, text| {
            Message::WorkspaceDialog(Msg::KitsHook(field, text))
        }),
    ]
    .spacing(tokens::space::LG)
    .width(Length::Fill);

    let close = Message::Intent(Intent::CloseOverlay);
    let panel = dialog::dialog(t, title, body)
        .width(640.0)
        .body_max_height(460.0)
        .close_tip(ctx.t(Key::ModalClose))
        .action(button::primary(t, ctx.t(Key::KitsManageEditDone)).on_press(close.clone()))
        .on_close(close);
    super::view::modal(ctx, panel)
}
