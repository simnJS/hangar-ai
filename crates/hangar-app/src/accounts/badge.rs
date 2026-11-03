//! A discreet label naming the Claude account a workspace or a pane runs on
//! (`AccountBadge.tsx`, `PaneAccountBadge.tsx`, and the navigator's
//! `accountBadge`). It shows the account's label, never its email, and it is
//! a label, not a control.

use hangar_i18n::{Key, args};
use hangar_theme::{style, tokens};
use hangar_ui::tip;
use iced::Element;
use iced::widget::text::Wrapping;
use iced::widget::{Space, container, text};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::types::AgentId;
use crate::state::{PaneId, WsId};

use super::live;
use super::model::{AccountSource, ClaudeAccount, resolve_account};
use super::switch::account_name;

/// How wide a label grows before it is cut (`max-width: 10em` at 10 px).
const MAX_WIDTH: f32 = 100.0;

fn nothing<'a>() -> Element<'a, Message> {
    Space::new().width(0).height(0).into()
}

/// With a single account there is nothing to tell apart, so nothing is drawn.
fn chip<'a>(ctx: Ctx<'a>, accounts: &[ClaudeAccount], id: &str) -> Element<'a, Message> {
    if accounts.len() < 2 {
        return nothing();
    }
    let unknown = ctx.t(Key::AccountsPickerUnknown);
    let name = account_name(accounts, Some(id), &unknown);
    let title = ctx
        .i18n
        .t_with(Key::AccountsBadgeTitle, &args!("name" => name.as_str()))
        .into_owned();
    let label = container(
        text(name)
            .size(tokens::font_size::MICRO)
            .wrapping(Wrapping::None)
            .style(style::text::secondary(ctx.theme)),
    )
    .max_width(MAX_WIDTH);
    tip::over(ctx.theme, label, title)
}

/// A navigator row's badge: only a workspace on an account other than the
/// default is worth a label.
pub fn workspace<'a>(ctx: Ctx<'a>, ws: &'a WsId) -> Element<'a, Message> {
    let Some(accounts) = live(ctx).accounts.as_deref() else {
        return nothing();
    };
    let Some(workspace) = ctx.model.workspace(ws) else {
        return nothing();
    };
    let default = ctx.model.settings().claude_account.as_deref();
    let own = resolve_account(accounts, default, workspace.claude_account.as_deref(), None);
    if own.source != AccountSource::Workspace
        || own.id == resolve_account(accounts, default, None, None).id
    {
        return nothing();
    }
    chip(ctx, accounts, &own.id)
}

/// A pane header's badge: it names the account of a pane whose agent runs on
/// another one than its workspace's (a pane-level override). Nothing
/// otherwise.
pub fn pane<'a>(ctx: Ctx<'a>, ws: &'a WsId, pane: &'a PaneId) -> Element<'a, Message> {
    let Some(accounts) = live(ctx).accounts.as_deref() else {
        return nothing();
    };
    let Some(workspace) = ctx.model.workspace(ws) else {
        return nothing();
    };
    let Some(pane) = workspace
        .panes
        .iter()
        .find(|candidate| candidate.id == *pane)
    else {
        return nothing();
    };
    if pane.agent != AgentId::Claude {
        return nothing();
    }
    let default = ctx.model.settings().claude_account.as_deref();
    let own = resolve_account(
        accounts,
        default,
        workspace.claude_account.as_deref(),
        pane.claude_account.as_deref(),
    );
    if own.source != AccountSource::Pane {
        return nothing();
    }
    let base = resolve_account(accounts, default, workspace.claude_account.as_deref(), None);
    if own.id == base.id {
        return nothing();
    }
    chip(ctx, accounts, &own.id)
}
