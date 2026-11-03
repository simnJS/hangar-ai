//! The account choice of the workspace dialog (`WorkspaceAccountPicker.tsx`):
//! « Default (Perso) » or one of the accounts. It shows the account's label
//! only, never its email.
//!
//! When the value names an account that no longer exists, a plain note says
//! the default applies; picking anything replaces the dead id.

use std::fmt;

use hangar_i18n::{Key, args};
use hangar_ui::{input, text};
use iced::widget::{Space, column};
use iced::Element;

use crate::ctx::Ctx;
use crate::message::Message;
use crate::workspace_dialog;

use super::live;
use super::model::read_account_choice;
use super::switch::default_label;

/// One line of the select: the account it picks (`None`: the default) and
/// what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Choice {
    id: Option<String>,
    label: String,
}

impl fmt::Display for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

pub fn view<'a>(ctx: Ctx<'a>, value: Option<&'a str>) -> Element<'a, Message> {
    let Some(accounts) = live(ctx).accounts.as_deref() else {
        return Space::new().width(0).height(0).into();
    };
    let chosen = read_account_choice(value);
    let missing = chosen
        .clone()
        .filter(|id| !accounts.iter().any(|account| account.id == *id));

    let default = Choice {
        id: None,
        label: default_label(
            accounts,
            ctx.model.settings().claude_account.as_deref(),
            &ctx.t(Key::AccountsPickerDefault),
            &ctx.t(Key::AccountsPickerUnknown),
        ),
    };
    let mut options = vec![default.clone()];
    options.extend(accounts.iter().map(|account| Choice {
        id: Some(account.id.clone()),
        label: account.label.clone(),
    }));
    // A dead id selects nothing of its own: the default line stands for it.
    let selected = match (&missing, &chosen) {
        (None, Some(id)) => options
            .iter()
            .find(|option| option.id.as_deref() == Some(id.as_str()))
            .cloned(),
        _ => Some(default),
    };

    let select = input::select(ctx.theme, options, selected, |choice: Choice| {
        Message::WorkspaceDialog(workspace_dialog::Msg::Account(choice.id))
    });
    let mut field = column![select].spacing(4);
    if let Some(id) = missing {
        field = field.push(text::hint(
            ctx.theme,
            ctx.i18n
                .t_with(Key::AccountsPickerMissing, &args!("id" => id.as_str()))
                .into_owned(),
        ));
    }
    field.into()
}
