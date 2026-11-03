//! The window's layout (App.tsx:1516, inventory §1.11):
//!
//! ```text
//! navigator │ toolbar
//!           │ ───────────────────────────────────────────────────────────
//!           │ main: terminals · board · memory · stats │ preview │ dock │ inspector
//!           │ ───────────────────────────────────────────────────────────
//!           │ status bar
//! + drawers over the right of the work area, the preview expanded over all of it
//! + floating: update banner, consent card, voice HUD, notice, chord hint
//! + overlays, top one last
//! ```
//!
//! The frame gives each region its size and its surface (the theme's chrome
//! colours, the hairlines between them); the region's content is its owner's
//! view. A workspace window is the same frame without the navigator. Where
//! each side panel goes (docked, drawer, hidden) is `ctx.panels()`
//! (side_panels.rs).

use iced::widget::{Space, column, container, mouse_area, opaque, row, rule, stack};
use iced::{Alignment, Element, Length};

use hangar_theme::{style, tokens};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::shell::title_bar::{self, Corners};
use crate::shell::{Action, BoardVariant, PanelMode, View, WindowKind, overlay, placeholder};
use crate::state::Workspace;

/// The main window.
pub fn main<'a>(ctx: Ctx<'a>) -> Element<'a, Message> {
    if !ctx.model.live.hydrated {
        return window_surface(ctx, placeholder::boot(ctx));
    }
    let p = ctx.pkg;
    let body: Element<'a, Message> = match (ctx.workspace(), ctx.model.active_workspace()) {
        (Some(ws), _) => workspace_body(ctx, ws),
        (None, Some(ws)) if ws.detached == Some(true) => {
            with_bare_band(ctx, placeholder::elsewhere(ctx, ws))
        }
        _ => with_bare_band(ctx, placeholder::no_workspace(ctx)),
    };
    let mut frame = row![].height(Length::Fill);
    if ctx.panels().navigator {
        frame = frame
            .push(
                container(crate::navigator::view(&p.navigator, ctx))
                    .width(tokens::size::SIDEBAR_WIDTH)
                    .height(Length::Fill)
                    .style(title_bar::rounded_style(
                        ctx,
                        Corners::LEFT,
                        style::container::sidebar(ctx.theme),
                    )),
            )
            .push(vertical_sep(ctx));
    }
    frame = frame.push(container(body).width(Length::Fill).height(Length::Fill));
    title_bar::outline(ctx, layered(ctx, window_surface(ctx, frame.into())))
}

/// A placeholder under the band Hangar's title bar needs where there is no
/// toolbar to be it.
fn with_bare_band<'a>(ctx: Ctx<'a>, body: Element<'a, Message>) -> Element<'a, Message> {
    match title_bar::bare_band(ctx) {
        Some(band) => column![band, horizontal_sep(ctx), body].into(),
        None => body,
    }
}

/// A workspace's own window: the main window's frame for that workspace,
/// without the navigator.
pub fn workspace_window<'a>(ctx: Ctx<'a>, ws: &'a Workspace) -> Element<'a, Message> {
    if !ctx.model.live.hydrated {
        return window_surface(ctx, placeholder::boot(ctx));
    }
    title_bar::outline(
        ctx,
        layered(ctx, window_surface(ctx, workspace_body(ctx, ws))),
    )
}

/// The width of the row the terminals share with the side panels, in a
/// window `width` wide: what `side_panels::place` places in.
pub fn body_width(width: f32, navigator: bool) -> f32 {
    if navigator {
        (width - tokens::size::SIDEBAR_WIDTH - tokens::BORDER_WIDTH).max(0.0)
    } else {
        width
    }
}

/// Toolbar, main area with its side panels, status bar.
fn workspace_body<'a>(ctx: Ctx<'a>, ws: &'a Workspace) -> Element<'a, Message> {
    let p = ctx.pkg;
    let panels = ctx.panels();
    // The toolbar's buttons open and close the drawers themselves: the
    // pointer over it is not "outside" a drawer (shell/drawer.rs). It is
    // the title bar too, when Hangar draws that.
    let toolbar = mouse_area(title_bar::band(
        ctx,
        container(crate::toolbar::view(&p.toolbar, ctx))
            .width(Length::Fill)
            .height(tokens::size::TOOLBAR_HEIGHT)
            .style(title_bar::rounded_style(
                ctx,
                Corners::top(ctx),
                style::container::toolbar(ctx.theme),
            )),
    ))
    .on_enter(Message::Shell(Action::PointerOverToolbar(true)))
    .on_exit(Message::Shell(Action::PointerOverToolbar(false)));
    let status = container(crate::status_bar::view(&p.status_bar, ctx))
        .width(Length::Fill)
        .height(tokens::size::STATUS_BAR_HEIGHT)
        .style(title_bar::rounded_style(
            ctx,
            Corners::bottom(ctx),
            style::container::statusbar(ctx.theme),
        ));

    let mut body = row![work_area(ctx, ws)].height(Length::Fill);
    let inspector = panels.inspector;
    if inspector.shown && !inspector.drawer {
        body = body.push(vertical_sep(ctx)).push(side(
            ctx,
            crate::inspector::view(&p.inspector, ctx),
            inspector.width,
        ));
    }
    let mut layers = stack![body].width(Length::Fill).height(Length::Fill);
    if inspector.shown && inspector.drawer {
        layers = layers.push(drawer(
            ctx,
            crate::inspector::view(&p.inspector, ctx),
            inspector.width,
        ));
    }

    column![
        toolbar,
        horizontal_sep(ctx),
        layers,
        horizontal_sep(ctx),
        status
    ]
    .into()
}

/// Region `main`, with the preview panel and the board dock beside it, or
/// over it as drawers.
fn work_area<'a>(ctx: Ctx<'a>, ws: &'a Workspace) -> Element<'a, Message> {
    let p = ctx.pkg;
    let panels = ctx.panels();
    let terminals = ctx.view() == View::Terminals;

    // Every workspace opened once stays drawn, hidden, whatever the view:
    // the grids keep their place in the tree, so their widgets keep their
    // state, and their terminals keep consuming output (PACKAGES.md, P5). A
    // workspace window draws its own workspace's alone; the main window
    // never draws one shown in a window of its own.
    let own_window = matches!(
        ctx.model.live.windows.get(&ctx.window),
        Some(WindowKind::Workspace(_))
    );
    let mut main = stack![].width(Length::Fill).height(Length::Fill);
    if own_window {
        main = main.push(crate::panes::view_workspace(
            &p.panes, ctx, &ws.id, !terminals,
        ));
    } else {
        for id in &ctx.model.ui.opened_ids {
            let Some(opened) = ctx.model.workspace(id) else {
                continue;
            };
            if opened.detached == Some(true) {
                continue;
            }
            let hidden = opened.id != ws.id || !terminals;
            main = main.push(crate::panes::view_workspace(
                &p.panes, ctx, &opened.id, hidden,
            ));
        }
    }
    match ctx.view() {
        View::Terminals => {}
        View::Board => main = main.push(crate::board::view(&p.board, ctx, BoardVariant::Full)),
        View::Memory => main = main.push(crate::memory::view(&p.memory, ctx)),
        View::Stats => main = main.push(crate::stats::view(&p.stats, ctx)),
    }

    let mut area = row![container(main).width(Length::Fill).height(Length::Fill)];
    let model_mode = panels.model_mode();
    if let Some(PanelMode::Docked { width }) = model_mode {
        area = area.push(vertical_sep(ctx)).push(side(
            ctx,
            crate::preview::view_panel(&p.preview, ctx, PanelMode::Docked { width }),
            width,
        ));
    }
    let dock = panels.dock;
    if dock.shown && !dock.drawer {
        let board = crate::board::view(
            &p.board,
            ctx,
            BoardVariant::Dock {
                width: dock.width,
                drawer: false,
            },
        );
        area = area
            .push(vertical_sep(ctx))
            .push(side(ctx, board, dock.width));
    }

    let mut layers = stack![area].width(Length::Fill).height(Length::Fill);
    match model_mode {
        Some(PanelMode::Expanded) => {
            layers = layers.push(opaque(
                container(crate::preview::view_panel(
                    &p.preview,
                    ctx,
                    PanelMode::Expanded,
                ))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(style::container::panel(ctx.theme)),
            ));
        }
        Some(PanelMode::Drawer { width }) => {
            let panel = crate::preview::view_panel(&p.preview, ctx, PanelMode::Drawer { width });
            layers = layers.push(drawer(ctx, panel, width));
        }
        _ => {}
    }
    if dock.shown && dock.drawer {
        let board = crate::board::view(
            &p.board,
            ctx,
            BoardVariant::Dock {
                width: dock.width,
                drawer: true,
            },
        );
        layers = layers.push(drawer(ctx, board, dock.width));
    }
    layers.into()
}

/// A docked side panel, `width` wide.
fn side<'a>(ctx: Ctx<'a>, content: Element<'a, Message>, width: f32) -> Element<'a, Message> {
    container(content)
        .width(width)
        .height(Length::Fill)
        .style(style::container::panel(ctx.theme))
        .into()
}

/// A side panel as a drawer over the right of what is under it. Opaque, so a
/// press on it never reaches the terminal underneath; its mouse area tells
/// the shell when the pointer is over it, for "a press outside closes it".
fn drawer<'a>(ctx: Ctx<'a>, content: Element<'a, Message>, width: f32) -> Element<'a, Message> {
    let panel = mouse_area(
        container(content)
            .width(width)
            .height(Length::Fill)
            .style(style::container::popover(ctx.theme)),
    )
    .on_enter(Message::Shell(Action::PointerOverDrawer(true)))
    .on_exit(Message::Shell(Action::PointerOverDrawer(false)));
    container(opaque(panel))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::End)
        .into()
}

/// The floating pieces and the overlays over a window's frame.
fn layered<'a>(ctx: Ctx<'a>, frame: Element<'a, Message>) -> Element<'a, Message> {
    let p = ctx.pkg;
    let mut layers = stack![frame];
    let focused = ctx.model.is_focused_window(ctx.window);

    // The update banner and the consent card are the main window's only.
    if ctx.model.live.windows.get(&ctx.window) == Some(&WindowKind::Main) {
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
    // above the status bar, in the window the person is in.
    if focused {
        let mut bottom = column![crate::voice::hud(&p.voice, ctx)]
            .spacing(tokens::space::SM)
            .align_x(Alignment::Center);
        if let Some(notice) = crate::shell::notice::view(ctx) {
            bottom = bottom.push(notice);
        }
        if let Some(hint) = crate::keymap::chord_hint(ctx) {
            bottom = bottom.push(hint);
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
    }

    for overlay in ctx.model.overlays_in(ctx.window) {
        layers = layers.push(overlay::view(ctx, overlay));
    }
    // Over everything, an open sheet's backdrop included: a window is
    // resized from its edges whatever it shows.
    if let Some(edges) = title_bar::edges(ctx) {
        layers = layers.push(edges);
    }
    layers.into()
}

/// The page under everything, in the theme's colours, rounded at the
/// window's corners when they are.
fn window_surface<'a>(ctx: Ctx<'a>, content: Element<'a, Message>) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(title_bar::rounded_style(
            ctx,
            Corners::ALL,
            style::container::window(ctx.theme),
        ))
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
