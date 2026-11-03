//! The panel itself: a header with the tab strip and the panel's own buttons,
//! the body under it, a grip on its left edge (`ModelPanel`, `.model-panel*`).
//!
//! Beside the terminals, over them as a drawer when there is no room, or over
//! the whole work area, expanded; the shell decides which and hands the mode
//! in. Each workspace has its own preview, and the panel draws the one of the
//! workspace its window shows.

use iced::widget::{Space, column, container, row, rule, stack};
use iced::{Alignment, Element, Length, Shadow, Vector};

use hangar_i18n::Key;
use hangar_theme::{style, tokens};
use hangar_ui::{Icon, button};

use super::grip::{self, Grip};
use super::store::{Body, EMPTY, Entry};
use super::{Msg, State, tabs, viewer};
use crate::ctx::Ctx;
use crate::message::Message;
use crate::shell::{PanelMode, WindowKind};
use crate::state::WsId;

/// The workspace a window shows: its own for a workspace window, else the one
/// the main window has on screen.
pub fn window_workspace<'a>(ctx: Ctx<'a>) -> Option<&'a WsId> {
    match ctx.model.live.windows.get(&ctx.window) {
        Some(WindowKind::Workspace(id)) => Some(id),
        _ => ctx.model.state().active_workspace_id.as_ref(),
    }
}

/// The panel for `mode`.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>, mode: PanelMode) -> Element<'a, Message> {
    let t = ctx.theme;
    let Some(ws) = window_workspace(ctx) else {
        return nothing(ctx);
    };
    let scope = ctx.model.live.preview.scope(ws);
    let expanded = matches!(mode, PanelMode::Expanded);
    let scope = scope.unwrap_or(&EMPTY);

    let head = row![
        container(tabs::view(ctx, ws, scope, st.spin)).width(Length::Fill),
        container(
            row![
                button::toggle_button(
                    t,
                    if scope.expanded {
                        Icon::Collapse
                    } else {
                        Icon::Expand
                    },
                    scope.expanded,
                )
                .tip(if scope.expanded {
                    ctx.t(Key::ModelCollapse)
                } else {
                    ctx.t(Key::ModelExpand)
                })
                .small()
                .on_press(Message::Preview(Msg::SetPanel(
                    ws.clone(),
                    Some(true),
                    Some(!scope.expanded),
                ))),
                button::icon_button(t, Icon::Close)
                    .tip(ctx.t(Key::ModelHide))
                    .small()
                    .on_press(Message::Preview(Msg::Hide(ws.clone()))),
            ]
            .spacing(2)
            .align_y(Alignment::Center)
        )
        .padding([0.0, tokens::space::XS]),
    ]
    .align_y(Alignment::Center)
    .height(tokens::size::PANE_HEAD_HEIGHT);
    let head = container(head)
        .width(Length::Fill)
        .style(style::container::pane_head(t, false));

    let body: Element<'a, Message> = match scope.active() {
        Some(entry) => entry_body(st, ctx, ws, entry),
        None => empty_state(ctx),
    };

    let panel = column![
        head,
        rule::horizontal(tokens::BORDER_WIDTH).style(style::rule::hairline(t)),
        container(body).width(Length::Fill).height(Length::Fill),
    ];
    let surface = container(panel)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| {
            let mut look = style::container::panel(t)(&iced::Theme::Dark);
            if matches!(mode, PanelMode::Drawer { .. }) {
                // Over the terminals: lifted off them.
                look.shadow = Shadow {
                    color: t.colors.shadow,
                    offset: Vector::new(-18.0, 0.0),
                    blur_radius: 44.0,
                };
            }
            look
        });
    if expanded {
        return surface.into();
    }
    stack![
        surface,
        row![
            Grip::new(t, |event| Message::Preview(Msg::Grip(event))),
            Space::new().width(Length::Fill)
        ]
        .width(grip::WIDTH)
        .height(Length::Fill)
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn nothing<'a>(ctx: Ctx<'a>) -> Element<'a, Message> {
    container(Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::container::panel(ctx.theme))
        .into()
}

fn empty_state<'a>(ctx: Ctx<'a>) -> Element<'a, Message> {
    hangar_ui::empty::empty(ctx.theme, Icon::Cube, ctx.t(Key::ModelEmpty).into_owned())
        .description(ctx.t(Key::ModelEmptyHint).into_owned())
        .into()
}

/// The body of the tab in front: a 3D model (P17), a kit's tab (P16), or a
/// file.
pub fn entry_body<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ws: &'a WsId,
    entry: &'a Entry,
) -> Element<'a, Message> {
    match &entry.body {
        Body::Model { .. } => crate::model3d_view::view(&ctx.pkg.model3d_view, ctx, ws, &entry.id),
        Body::Kit(_) => crate::kit_tabs::view(&ctx.pkg.kit_tabs, ctx, ws, &entry.id),
        Body::File { .. } => viewer::view(st, ctx, ws, entry),
    }
}
