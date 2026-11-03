//! The toolbar button of « Comptes Claude » (`AccountSwitcher.tsx`,
//! `ConnectedAccountSwitcher.tsx`): it shows the account of the workspace on
//! screen, and its menu switches it in one click.
//!
//! The menu has one row per account: its main part uses the account in the
//! workspace, with the figures of its limits under the name, and a second part
//! makes it the default of every workspace. With agents running in that scope,
//! the click first asks whether to restart them now (their conversations are
//! kept) or on their next launch.
//!
//! Switching is always this click: nothing here rotates accounts by itself.
//! The email of an account is in the tooltip of its row and appears nowhere
//! else.

use hangar_i18n::{Key, Translator, args};
use hangar_theme::{style, tokens};
use hangar_ui::menu;
use hangar_ui::{Icon, button as ui_button, icon, layer, text as typography, tip};
use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, row, text};
use iced::{Alignment, Element, Length, Padding};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::WsId;

use super::model::{AccountId, ClaudeAccount, resolve_account};
use super::switch::{AccountUsage, account_name, usage_percent};
use super::{Msg, State, live};

/// What a click on a row changes: the workspace on screen (the main action),
/// or the default every workspace without an account of its own follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchScope {
    Workspace,
    Default,
}

/// What the switcher's popover shows.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum Menu {
    #[default]
    Closed,
    /// The list of accounts.
    Open,
    /// The question asked before a switch that restarts running agents.
    Ask {
        ws: WsId,
        id: AccountId,
        scope: SwitchScope,
    },
}

/// The workspace the toolbar belongs to: its own window's, else the one in
/// front. Nothing when the one in front is shown in a window of its own, as
/// for every other part of the toolbar: the bar is about the workspace its
/// window draws.
fn toolbar_workspace(ctx: Ctx<'_>) -> Option<&WsId> {
    ctx.workspace().map(|ws| &ws.id)
}

fn press(msg: Msg) -> Message {
    Message::Accounts(msg)
}

pub fn view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let nothing = || Space::new().width(0).height(0).into();
    let t = ctx.theme;
    let Some(accounts) = live(ctx).accounts.as_deref() else {
        return nothing();
    };
    let Some(ws) = toolbar_workspace(ctx) else {
        return nothing();
    };
    let Some(workspace) = ctx.model.workspace(ws) else {
        return nothing();
    };
    let default = ctx.model.settings().claude_account.as_deref();
    let current = resolve_account(accounts, default, workspace.claude_account.as_deref(), None).id;
    let default_id = resolve_account(accounts, default, None, None).id;

    let unknown = ctx.t(Key::AccountsPickerUnknown);
    let current_name = account_name(accounts, Some(&current), &unknown);
    let title = ctx
        .i18n
        .t_with(
            Key::AccountsSwitcherButton,
            &args!("name" => current_name.as_str()),
        )
        .into_owned();

    let label = container(
        text(current_name)
            .size(tokens::font_size::UI)
            .wrapping(Wrapping::None),
    )
    .max_width(120.0);
    let anchor = button(
        row![
            icon(Icon::Bolt, tokens::size::ICON),
            label,
            icon(Icon::ChevronDown, 12.0),
        ]
        .spacing(tokens::space::SM)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([0.0, tokens::space::MD]))
    .height(tokens::size::CONTROL_HEIGHT)
    .style(style::button::ghost(t))
    .on_press(press(Msg::ToggleMenu));
    let anchor = tip::bottom(t, anchor, title);

    // The menu is open for the app, not for a window (its state is one value
    // and its messages carry no window): it hangs off the button of the window
    // the person is in, where they pressed it, and not off every window's.
    let content = match &st.switcher {
        _ if !ctx.model.is_focused_window(ctx.window) => None,
        Menu::Closed => None,
        Menu::Open => Some(menu::panel(
            t,
            list(ctx, accounts, ws, &current, &default_id),
        )),
        Menu::Ask {
            ws: asked,
            id,
            scope,
        } => Some(menu::panel(t, ask(ctx, accounts, asked, id, *scope))),
    };
    layer::dropdown(anchor, content)
        .on_dismiss(press(Msg::CloseMenu))
        .align_end()
        .into()
}

fn usage_line(t: Translator, usage: Option<&AccountUsage>) -> String {
    let show = |value: Option<u8>| match value {
        None => t.t(Key::AccountsSwitcherUnknown).into_owned(),
        Some(n) => t
            .t_with(Key::AccountsSwitcherPercent, &args!("n" => n))
            .into_owned(),
    };
    let five = t.t(Key::AccountsSwitcherFive);
    let week = t.t(Key::AccountsSwitcherWeek);
    t.t_with(
        Key::AccountsSwitcherUsage,
        &args!(
            "five" => five.as_ref(),
            "fiveValue" => show(usage_percent(usage.and_then(|u| u.five_hour.as_ref()))),
            "week" => week.as_ref(),
            "weekValue" => show(usage_percent(usage.and_then(|u| u.weekly.as_ref()))),
        ),
    )
    .into_owned()
}

/// The list: a heading and a row per account.
fn list<'a>(
    ctx: Ctx<'a>,
    accounts: &'a [ClaudeAccount],
    ws: &'a WsId,
    current: &str,
    default_id: &str,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let mut menu = column![
        container(typography::hint(t, ctx.t(Key::AccountsSwitcherHeading)))
            .padding(Padding::from([tokens::space::XS, tokens::space::MD]))
    ]
    .spacing(tokens::space::XXS)
    .width(Length::Fixed(320.0));
    for account in accounts {
        menu = menu.push(account_row(
            ctx,
            account,
            ws,
            account.id == current,
            account.id == default_id,
        ));
    }
    menu.into()
}

fn account_row<'a>(
    ctx: Ctx<'a>,
    account: &'a ClaudeAccount,
    ws: &'a WsId,
    is_current: bool,
    is_default: bool,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let usage = live(ctx).usage.get(&account.id);

    let mark: Element<'a, Message> = if is_current {
        icon(Icon::Check, tokens::size::ICON).into()
    } else {
        Space::new().width(tokens::size::ICON).into()
    };
    let mut name = row![text(account.label.as_str()).size(tokens::font_size::BODY)];
    if !account.logged_in {
        name = name.push(
            text(format!(" · {}", ctx.t(Key::AccountsSwitcherNotLoggedIn)))
                .size(tokens::font_size::BODY)
                .style(style::text::muted(t)),
        );
    }
    let lines = column![
        name,
        text(usage_line(ctx.i18n, usage))
            .size(tokens::font_size::LABEL)
            .style(style::text::muted(t)),
    ];
    let main = button(
        row![mark, lines]
            .spacing(tokens::space::MD)
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(Padding::from([tokens::space::XS, tokens::space::MD]))
    .style(style::button::menu_item(t))
    .on_press(press(Msg::Choose {
        ws: ws.clone(),
        id: account.id.clone(),
        scope: SwitchScope::Workspace,
    }));
    // The email is the tooltip of the row, and shown nowhere else.
    let main: Element<'a, Message> = match account.email.as_deref() {
        Some(email) => tip::over(t, main, email.to_string()),
        None => main.into(),
    };

    let every_label = ctx
        .i18n
        .t_with(
            Key::AccountsSwitcherUseEverywhereLabel,
            &args!("name" => account.label.as_str()),
        )
        .into_owned();
    let every_text =
        text(ctx.t(Key::AccountsSwitcherUseEverywhere)).size(tokens::font_size::CAPTION);
    let every_text = if is_default {
        every_text.style(style::text::accent(t))
    } else {
        every_text
    };
    let every = button(every_text)
        .padding(Padding::from([tokens::space::XS, tokens::space::MD]))
        .style(style::button::menu_item(t))
        .on_press(press(Msg::Choose {
            ws: ws.clone(),
            id: account.id.clone(),
            scope: SwitchScope::Default,
        }));
    let every = tip::over(t, every, every_label);

    row![main, every]
        .spacing(tokens::space::XS)
        .align_y(Alignment::Center)
        .into()
}

/// The question: switching restarts running agents, now or on their next
/// launch, and the conversations are kept either way.
fn ask<'a>(
    ctx: Ctx<'a>,
    accounts: &'a [ClaudeAccount],
    ws: &'a WsId,
    id: &AccountId,
    scope: SwitchScope,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let unknown = ctx.t(Key::AccountsPickerUnknown);
    let name = account_name(accounts, Some(id), &unknown);
    let (title_key, restart_key) = match scope {
        SwitchScope::Workspace => (
            Key::AccountsSwitcherAskHere,
            Key::AccountsSwitcherRestartHere,
        ),
        SwitchScope::Default => (
            Key::AccountsSwitcherAskEverywhere,
            Key::AccountsSwitcherRestartEverywhere,
        ),
    };
    let running = super::running_in(ctx.model, ws, scope);
    let title = ctx
        .i18n
        .t_with(title_key, &args!("name" => name.as_str()))
        .into_owned();
    let body = ctx
        .i18n
        .tn(Key::AccountsSwitcherRunning, running, &[])
        .into_owned();
    column![
        typography::body(t, title).font(hangar_theme::fonts::ui_semibold()),
        typography::muted(t, body),
        ui_button::primary(t, ctx.t(restart_key))
            .on_press(press(Msg::Decide(true)))
            .width(Length::Fill),
        ui_button::secondary(t, ctx.t(Key::AccountsSwitcherNextLaunch))
            .on_press(press(Msg::Decide(false)))
            .width(Length::Fill),
        ui_button::ghost(t, ctx.t(Key::AccountsSwitcherBack))
            .on_press(press(Msg::Back))
            .width(Length::Fill),
    ]
    .spacing(tokens::space::MD)
    .width(Length::Fixed(320.0))
    .into()
}

#[cfg(test)]
mod tests {
    //! The toolbar's account button is about the workspace of its own window.

    use hangar_i18n::Locale;

    use super::*;
    use crate::navigator::testing::{ctx_in, detach, simulate};
    use crate::workspace_dialog::testing::Harness;

    /// `w1` follows the default account, `w2` is on « Perso ».
    fn harness() -> Harness {
        Harness::new(Locale::En, "graphite")
    }

    #[test]
    fn a_workspace_windows_button_names_the_account_of_its_own_workspace() {
        let mut h = harness();
        let own = detach(&mut h.model, "w2");
        let st = State::default();

        let mut mine = simulate(view(&st, ctx_in(&h.model, &h.pkg, own)));
        assert!(mine.find("Perso").is_ok());
        assert!(mine.find("Principal").is_err());
        drop(mine);

        let mut main = simulate(view(&st, ctx_in(&h.model, &h.pkg, h.window)));
        assert!(main.find("Principal").is_ok());
        assert!(main.find("Perso").is_err());
    }

    #[test]
    fn the_main_window_has_no_button_while_its_workspace_is_in_a_window_of_its_own() {
        let mut h = harness();
        let mut state = h.model.state().clone();
        state.active_workspace_id = Some("w2".into());
        h.model.hydrate(state);
        detach(&mut h.model, "w2");
        let st = State::default();
        let mut main = simulate(view(&st, ctx_in(&h.model, &h.pkg, h.window)));
        assert!(main.find("Perso").is_err());
        assert!(main.find("Principal").is_err());
    }
}
