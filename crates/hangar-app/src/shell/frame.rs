//! The window's layout (App.tsx:1516, inventory §1.11):
//!
//! ```text
//! navigator │ toolbar
//!           │ ─────────────────────────────────────────
//!           │ main: terminals · board · memory · stats │ preview │ dock │ inspector
//!           │ ─────────────────────────────────────────
//!           │ status bar
//! + floating: update banner, consent card, voice HUD, notice, chord hint
//! + overlays, top one last
//! ```
//!
//! The frame gives each region its size and its surface (the theme's chrome
//! colours, the hairlines between them); the region's content is its owner's
//! view. A workspace window is the same frame without the navigator.

use iced::widget::{Space, column, container, row, rule, stack, text};
use iced::{Alignment, Element, Length};

use hangar_theme::{style, tokens};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::shell::{BoardVariant, View, overlay, placeholder};
use crate::state::Workspace;

/// The main window.
pub fn main<'a>(ctx: Ctx<'a>) -> Element<'a, Message> {
    if !ctx.model.live.hydrated {
        return window_surface(ctx, placeholder::boot(ctx));
    }
    let p = ctx.pkg;
    let body: Element<'a, Message> = match ctx.model.active_workspace() {
        None => placeholder::no_workspace(ctx),
        Some(ws) if ws.detached == Some(true) => placeholder::elsewhere(ctx, ws),
        Some(ws) => workspace_body(ctx, ws),
    };
    let mut frame = row![].height(Length::Fill);
    if ctx.model.settings().navigator_open {
        frame = frame
            .push(
                container(crate::navigator::view(&p.navigator, ctx))
                    .width(tokens::size::SIDEBAR_WIDTH)
                    .height(Length::Fill)
                    .style(style::container::sidebar(ctx.theme)),
            )
            .push(vertical_sep(ctx));
    }
    frame = frame.push(container(body).width(Length::Fill).height(Length::Fill));
    layered(ctx, window_surface(ctx, frame.into()))
}

/// A workspace's own window: the main window's frame for that workspace,
/// without the navigator.
pub fn workspace_window<'a>(ctx: Ctx<'a>, ws: &'a Workspace) -> Element<'a, Message> {
    if !ctx.model.live.hydrated {
        return window_surface(ctx, placeholder::boot(ctx));
    }
    layered(ctx, window_surface(ctx, workspace_body(ctx, ws)))
}

/// Toolbar, main area with its side panels, status bar.
fn workspace_body<'a>(ctx: Ctx<'a>, ws: &'a Workspace) -> Element<'a, Message> {
    let p = ctx.pkg;
    let settings = ctx.model.settings();
    let toolbar = container(crate::toolbar::view(&p.toolbar, ctx))
        .width(Length::Fill)
        .height(tokens::size::TOOLBAR_HEIGHT)
        .style(style::container::toolbar(ctx.theme));
    let status = container(crate::status_bar::view(&p.status_bar, ctx))
        .width(Length::Fill)
        .height(tokens::size::STATUS_BAR_HEIGHT)
        .style(style::container::statusbar(ctx.theme));

    let mut body = row![work_area(ctx, ws)].height(Length::Fill);
    // TODO(P0b): `side_panels::place` decides docked or drawer from the
    // measured body width; every panel is docked at its setting's width
    // until then.
    if settings.inspector_open {
        body = body.push(vertical_sep(ctx)).push(
            container(crate::inspector::view(&p.inspector, ctx))
                .width(settings.inspector_width as f32)
                .height(Length::Fill)
                .style(style::container::panel(ctx.theme)),
        );
    }

    column![
        toolbar,
        horizontal_sep(ctx),
        body,
        horizontal_sep(ctx),
        status
    ]
    .into()
}

/// Region `main`, with the preview panel and the board dock beside it.
fn work_area<'a>(ctx: Ctx<'a>, ws: &'a Workspace) -> Element<'a, Message> {
    let p = ctx.pkg;
    let ui = &ctx.model.ui;
    let main: Element<'a, Message> = match ui.view {
        View::Terminals => {
            // Every workspace opened once stays alive while hidden: its
            // terminals keep consuming output (PACKAGES.md, P5).
            let mut layers = stack![].width(Length::Fill).height(Length::Fill);
            for id in &ui.opened_ids {
                let Some(opened) = ctx.model.workspace(id) else {
                    continue;
                };
                if opened.detached == Some(true) && opened.id != ws.id {
                    continue;
                }
                let hidden = opened.id != ws.id;
                layers = layers.push(crate::panes::view_workspace(
                    &p.panes, ctx, &opened.id, hidden,
                ));
            }
            layers.into()
        }
        View::Board => crate::board::view(&p.board, ctx, BoardVariant::Full),
        View::Memory => crate::memory::view(&p.memory, ctx),
        View::Stats => crate::stats::view(&p.stats, ctx),
    };
    let mut area = row![container(main).width(Length::Fill).height(Length::Fill)];
    // TODO(P0b, P15): the preview panel (region `model`), shown while the
    // workspace has a preview open: `preview::view_panel(st, ctx, mode)`.
    let settings = ctx.model.settings();
    if settings.board_dock_open && ui.view == View::Terminals {
        let width = settings.board_dock_width as f32;
        area = area.push(vertical_sep(ctx)).push(
            container(crate::board::view(
                &p.board,
                ctx,
                BoardVariant::Dock {
                    width,
                    drawer: false,
                },
            ))
            .width(width)
            .height(Length::Fill)
            .style(style::container::panel(ctx.theme)),
        );
    }
    area.into()
}

/// The floating pieces and the overlays over a window's frame.
fn layered<'a>(ctx: Ctx<'a>, frame: Element<'a, Message>) -> Element<'a, Message> {
    let p = ctx.pkg;
    let mut layers = stack![frame];

    // The update banner and the consent card are the main window's only.
    if ctx.model.live.windows.get(&ctx.window) == Some(&crate::shell::WindowKind::Main) {
        layers = layers
            .push(
                container(crate::update_banner::view(&p.update_banner, ctx))
                    .width(Length::Fill)
                    .align_x(Alignment::Center),
            )
            .push(
                container(crate::consent_card::view(&p.consent_card, ctx))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .padding(tokens::space::XXL)
                    .align_x(Alignment::End)
                    .align_y(Alignment::End),
            );
    }

    // The voice HUD, the notice and the chord hint share the bottom centre,
    // above the status bar.
    let mut bottom = column![crate::voice::hud(&p.voice, ctx)]
        .spacing(tokens::space::SM)
        .align_x(Alignment::Center);
    if let Some(notice) = crate::shell::notice::view(ctx) {
        bottom = bottom.push(notice);
    }
    if !ctx.model.ui.pending_chords.is_empty() {
        // TODO(P7): `keymap.pending` with the chords as the keymap shows them.
        bottom = bottom.push(
            container(text(ctx.model.ui.pending_chords.join(" ")).size(tokens::font_size::UI))
                .padding([tokens::space::XS, tokens::space::MD])
                .style(style::container::tooltip(ctx.theme)),
        );
    }
    layers = layers.push(
        container(column![
            bottom,
            Space::new().height(tokens::size::STATUS_BAR_HEIGHT + 12.0)
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::End),
    );

    for overlay in &ctx.model.ui.overlays {
        layers = layers.push(overlay::view(ctx, overlay));
    }
    layers.into()
}

/// The page under everything, in the theme's colours.
fn window_surface<'a>(ctx: Ctx<'a>, content: Element<'a, Message>) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::container::window(ctx.theme))
        .into()
}

fn vertical_sep<'a>(ctx: Ctx<'a>) -> Element<'a, Message> {
    rule::vertical(tokens::BORDER_WIDTH)
        .style(style::rule::sep(ctx.theme))
        .into()
}

fn horizontal_sep<'a>(ctx: Ctx<'a>) -> Element<'a, Message> {
    rule::horizontal(tokens::BORDER_WIDTH)
        .style(style::rule::sep(ctx.theme))
        .into()
}
