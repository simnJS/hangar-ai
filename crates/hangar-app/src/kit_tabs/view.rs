//! A kit's tab in the preview panel (`KitTabView`): its bar (reload, and the
//! chip saying it runs isolated, with what the workspace granted it in
//! words), then its page: drawn here for a native tab, the card of its
//! window for an HTML one, or why it cannot run.

use iced::widget::{Space, column, container, row, text};
use iced::{Alignment, Element, Length};

use hangar_i18n::{Key, Translator, args};
use hangar_kit_host::KitStyle;
use hangar_theme::{Theme, style, tokens};
use hangar_ui::{Icon, icon};

use super::tabs::{KitTabState, tab_state_on};
use super::{Msg, State};
use crate::ctx::Ctx;
use crate::kits::model::{kit_command_name, kit_loopback_port};
use crate::message::Message;
use crate::preview::parts::{self, Feel, Mark};
use crate::preview::store::Entry;
use crate::preview::webview::WinState;
use crate::state::WsId;

fn send(msg: Msg) -> Message {
    Message::KitTabs(msg)
}

/// What a tab was granted, for its bar's hint (`permissionSaid`).
pub fn permission_said(tr: Translator, permission: &str) -> String {
    if let Some(name) = kit_command_name(permission) {
        return tr
            .t_with(Key::KitsTabPermissionCommand, &args!("name" => name))
            .into_owned();
    }
    if let Some(port) = kit_loopback_port(permission) {
        // As text: a port has no thousands.
        return tr
            .t_with(
                Key::KitsTabPermissionLoopback,
                &args!("port" => port.to_string()),
            )
            .into_owned();
    }
    let key = match permission {
        "workspace.read" => Key::KitsTabPermissionWorkspaceRead,
        "panes.read" => Key::KitsTabPermissionPanesRead,
        "panes.send" => Key::KitsTabPermissionPanesSend,
        "files.read" => Key::KitsTabPermissionFilesRead,
        "files.write" => Key::KitsTabPermissionFilesWrite,
        "data.read" => Key::KitsTabPermissionDataRead,
        "data.write" => Key::KitsTabPermissionDataWrite,
        "preview.show" => Key::KitsTabPermissionPreviewShow,
        "notify" => Key::KitsTabPermissionNotify,
        other => return other.to_owned(),
    };
    tr.t(key).into_owned()
}

/// "Runs isolated", and on hover what it reaches.
fn isolated<'a>(
    ctx: Ctx<'a>,
    kit: &str,
    permissions: &[String],
    native: bool,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let said: Vec<String> = permissions
        .iter()
        .map(|permission| permission_said(ctx.i18n, permission))
        .collect();
    let hint = match (said.is_empty(), native) {
        (true, false) => ctx
            .i18n
            .t_with(Key::KitsTabIsolatedNothing, &args!("kit" => kit)),
        (true, true) => ctx
            .i18n
            .t_with(Key::KitsTabNativeIsolatedNothing, &args!("kit" => kit)),
        (false, false) => ctx.i18n.t_with(
            Key::KitsTabIsolatedHint,
            &args!("kit" => kit, "permissions" => said.join(", ")),
        ),
        (false, true) => ctx.i18n.t_with(
            Key::KitsTabNativeIsolatedHint,
            &args!("kit" => kit, "permissions" => said.join(", ")),
        ),
    }
    .into_owned();
    let chip = container(
        row![
            icon(Icon::Lock, 12.0).color(t.colors.accent_hi),
            text(ctx.t(Key::KitsTabIsolated).into_owned())
                .size(tokens::font_size::CAPTION)
                .color(t.colors.accent_hi),
        ]
        .spacing(5)
        .align_y(Alignment::Center),
    )
    .padding([2.0, 8.0])
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(t.colors.accent_soft)),
        border: iced::Border {
            radius: tokens::radius::FULL.into(),
            ..Default::default()
        },
        ..Default::default()
    });
    hangar_ui::tip::bottom(t, chip, hint)
}

/// The tab in the panel's body.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>, ws: &'a WsId, id: &'a str) -> Element<'a, Message> {
    let t = ctx.theme;
    let Some(entry) = ctx
        .model
        .live
        .preview
        .scope(ws)
        .and_then(|scope| scope.entry(id))
    else {
        return parts::loading(ctx, id, None);
    };
    let Some(kit) = entry.kit() else {
        return parts::loading(ctx, &entry.title, None);
    };
    let state = match ctx.model.workspace(ws) {
        Some(workspace) => tab_state_on(
            workspace,
            ctx.model.settings(),
            ctx.model.live.kits.installed.as_deref(),
            &kit.kit_id,
            &kit.tab_id,
        ),
        None => KitTabState::Disabled,
    };
    let (kit_name, permissions) = match &state {
        KitTabState::Ready { choice, .. } => (choice.kit_name.clone(), choice.permissions.clone()),
        _ => (kit.kit_id.clone(), Vec::new()),
    };
    let native = st.natives.contains_key(id);
    let fallback = st.unloadable.get(id).filter(|_| st.pages.contains_key(id));
    let mut aside = Vec::new();
    if let Some((_, reason)) = fallback {
        aside.push(hangar_ui::tip::bottom(
            t,
            parts::chip(t, ctx.t(Key::KitsTabFallback).into_owned(), false),
            ctx.i18n
                .t_with(
                    Key::KitsTabFallbackHint,
                    &args!("reason" => reason.as_str()),
                )
                .into_owned(),
        ));
    }
    aside.push(isolated(ctx, &kit_name, &permissions, native));
    let bar = parts::bar(
        t,
        vec![parts::icon_tool(
            t,
            Icon::Restart,
            ctx.t(Key::KitsTabReload),
            None,
            Some(send(Msg::Reload(id.to_owned()))),
        )],
        aside,
    );
    let body: Element<'a, Message> = match state {
        KitTabState::Loading => parts::stage_message(
            t,
            Mark::Icon(Icon::Window),
            Feel::Calm,
            ctx.t(Key::KitsTabLoading),
            None,
            None,
        ),
        KitTabState::Disabled => parts::stage_message(
            t,
            Mark::Icon(Icon::Lock),
            Feel::Calm,
            ctx.t(Key::KitsTabDisabled),
            Some(ctx.t(Key::KitsTabDisabledHint).into_owned()),
            None,
        ),
        KitTabState::Missing => parts::stage_message(
            t,
            Mark::Icon(Icon::Alert),
            Feel::Warning,
            ctx.t(Key::KitsTabMissing),
            Some(ctx.t(Key::KitsTabMissingHint).into_owned()),
            None,
        ),
        KitTabState::Broken(error) => parts::stage_message(
            t,
            Mark::Icon(Icon::Alert),
            Feel::Danger,
            ctx.t(Key::KitsTabBroken),
            Some(error),
            None,
        ),
        KitTabState::Ready { .. } => ready(st, ctx, entry),
    };
    column![
        bar,
        container(body).width(Length::Fill).height(Length::Fill)
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// A tab that can run: its page, or where it is.
fn ready<'a>(st: &'a State, ctx: Ctx<'a>, entry: &'a Entry) -> Element<'a, Message> {
    let t = ctx.theme;
    let id = entry.id.as_str();
    if let Some(native) = st.natives.get(id) {
        let style = KitStyle::new(t, ctx.model.locale());
        let owner = id.to_owned();
        let page: Element<'a, Message> = native
            .tab
            .draw(style)
            .map(move |message| send(Msg::Native(owner.clone(), message)));
        let owner = id.to_owned();
        let page = hangar_ui::toast::over(t, page, &native.asked.toasts, move |shown| {
            send(Msg::ToastDismiss(owner.clone(), shown))
        });
        let question = native.asked.confirm.as_ref().map(|(asked, _)| {
            let cancel = asked
                .cancel
                .clone()
                .unwrap_or_else(|| ctx.t(Key::KitsTabNativeCancel).into_owned());
            let confirm = asked
                .confirm
                .clone()
                .unwrap_or_else(|| ctx.t(Key::KitsTabNativeConfirm).into_owned());
            let yes = if asked.danger {
                hangar_ui::button::danger(t, confirm)
            } else {
                hangar_ui::button::primary(t, confirm)
            };
            // Cancel first, as the person reads it: the safe answer.
            hangar_ui::dialog::dialog(
                t,
                entry.title.clone(),
                text(asked.message.clone())
                    .size(tokens::font_size::BODY)
                    .style(style::text::primary(t)),
            )
            .action(
                hangar_ui::button::secondary(t, cancel)
                    .on_press(send(Msg::Confirmed(id.to_owned(), false))),
            )
            .action(yes.on_press(send(Msg::Confirmed(id.to_owned(), true))))
            .on_close(send(Msg::Confirmed(id.to_owned(), false)))
        });
        return hangar_ui::dialog::over(t, page, question).into();
    }
    if let Some(page) = st.pages.get(id) {
        let reload = || {
            Some((
                ctx.t(Key::KitsTabReload).into_owned(),
                send(Msg::Reload(id.to_owned())),
            ))
        };
        let open = || {
            Some((
                ctx.t(Key::PreviewWindowOpen).into_owned(),
                send(Msg::OpenWindow(id.to_owned())),
            ))
        };
        if page.left {
            return parts::stage_message(
                t,
                Mark::Icon(Icon::Alert),
                Feel::Warning,
                ctx.t(Key::KitsTabLeft),
                Some(ctx.t(Key::KitsTabLeftHint).into_owned()),
                reload(),
            );
        }
        return match &page.state {
            WinState::Waiting | WinState::Opening => parts::loading(
                ctx,
                &entry.title,
                Some(ctx.t(Key::PreviewWindowStarting).into_owned()),
            ),
            WinState::Open => shown_apart(
                ctx,
                entry,
                Some((
                    ctx.t(Key::PreviewWindowFocus).into_owned(),
                    send(Msg::FocusWindow(id.to_owned())),
                )),
            ),
            WinState::Closed | WinState::NoPdfViewer => parts::stage_message(
                t,
                Mark::Icon(Icon::Window),
                Feel::Calm,
                ctx.t(Key::PreviewWindowClosed),
                None,
                open(),
            ),
            WinState::Missing => parts::stage_message(
                t,
                Mark::Icon(Icon::Alert),
                Feel::Warning,
                ctx.t(Key::PreviewWindowMissing),
                Some(ctx.t(Key::PreviewWindowMissingHint).into_owned()),
                None,
            ),
            WinState::Failed(why) => parts::stage_message(
                t,
                Mark::Icon(Icon::Alert),
                Feel::Danger,
                ctx.t(Key::PreviewWindowFailed),
                Some(why.clone()),
                open(),
            ),
        };
    }
    if let Some((_, reason)) = st.unloadable.get(id) {
        return parts::stage_message(
            t,
            Mark::Icon(Icon::Alert),
            Feel::Danger,
            ctx.t(Key::KitsTabNativeNotStarted),
            Some(reason.clone()),
            Some((
                ctx.t(Key::KitsTabReload).into_owned(),
                send(Msg::Reload(id.to_owned())),
            )),
        );
    }
    // Brought back at startup and not loaded yet, or on its way.
    stage(t)
}

fn stage<'a>(t: &'a Theme) -> Element<'a, Message> {
    container(Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::container::panel(t))
        .into()
}

/// "Shown in its own window", with the way to it.
fn shown_apart<'a>(
    ctx: Ctx<'a>,
    entry: &'a Entry,
    action: Option<(String, Message)>,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let mut body = column![
        icon(Icon::Window, 40.0).color(t.colors.fg_subtle),
        text(entry.title.as_str())
            .size(tokens::font_size::BODY)
            .font(hangar_theme::fonts::ui_semibold())
            .style(style::text::primary(t))
            .wrapping(text::Wrapping::None),
        text(ctx.t(Key::PreviewWindowTitle).into_owned())
            .size(tokens::font_size::CAPTION_LG)
            .style(style::text::secondary(t)),
        text(ctx.t(Key::KitsTabWindowHint).into_owned())
            .size(tokens::font_size::CAPTION)
            .style(style::text::muted(t))
            .align_x(iced::alignment::Horizontal::Center),
    ]
    .spacing(tokens::space::MD)
    .align_x(Alignment::Center);
    if let Some((label, on_press)) = action {
        body = body.push(hangar_ui::button::secondary(t, label).on_press(on_press));
    }
    container(container(body).max_width(360.0))
        .width(Length::Fill)
        .height(Length::Fill)
        .center(Length::Fill)
        .padding(tokens::space::XXXL)
        .into()
}

#[cfg(test)]
mod tests {
    use hangar_i18n::Locale;

    use super::*;

    #[test]
    fn says_what_a_tab_was_granted_in_words_a_port_without_its_thousands() {
        let en = Translator::from(Locale::En);
        assert_eq!(permission_said(en, "panes.read"), "see the panes");
        assert_eq!(
            permission_said(en, "loopback:8765"),
            "talk to the local server on port 8765"
        );
        assert_eq!(
            permission_said(en, "command:ci-run"),
            "run the command ci-run"
        );
        let fr = Translator::from(Locale::Fr);
        assert_eq!(permission_said(fr, "notify"), "envoyer des notifications");
    }
}
