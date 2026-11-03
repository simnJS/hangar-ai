//! The navigator as it is drawn (`Sidebar.tsx`'s markup, `styles.css`'s
//! `.nav-*`): the top, the project manager's door, the heading, the tree, the
//! plan, the filter.
//!
//! Every row has a fixed height, so a drag needs no layout to ask: a row says
//! where inside it the pointer is, and the half of it decides before or after.

use std::collections::HashMap;

use iced::widget::{
    Space, button, column, container, mouse_area, row, scrollable, stack, text, text_input,
};
use iced::{Alignment, Background, Element, Length, Padding, Vector};

use hangar_i18n::{Key, args};
use hangar_theme::tokens::{font_size, radius, size, space};
use hangar_theme::{Theme, fonts, style};
use hangar_ui::logo::logo;
use hangar_ui::menu as ui_menu;
use hangar_ui::{Icon, button as ui_button, glyph, icon, keys, tip};

use super::drag::{self, Drag, DragItem, DragKind, DropTarget, Hit};
use super::model::{NavFolder, NavInput, RowKind, build_nav_tree, namesake_hints, row_key};
use super::{
    MenuState, Msg, State, filter_id, geometry, list_id, menu, plan, reads, rename_id, skin, status,
};
use crate::commands::CommandId;
use crate::ctx::Ctx;
use crate::inspector::format::prose_time;
use crate::message::{Intent, Message};
use crate::shell::{Focus, Region};
use crate::state::roles::{orchestrator_of, workers_of};
use crate::state::types::Workspace;
use crate::state::{AgentActivity, WsId};

/// A workspace's row, and a folder's heading (`--nav-row`).
pub const ROW_HEIGHT: f32 = geometry::ROW_HEIGHT;
/// Where a workspace's name starts: the padding, the icon and the gap, which
/// a drop marker starts at too (`--nav-name-x`).
const NAME_X: f32 = 32.0;
/// The padding from a row's edge to its icon, and to a heading's first
/// letter: they line up, so the eye runs straight down the left edge.
const PAD: f32 = 8.0;

fn msg(msg: Msg) -> Message {
    Message::Navigator(msg)
}

/// What the tree is drawn from, worked out once per frame.
struct Frame<'a> {
    st: &'a State,
    ctx: Ctx<'a>,
    active: Option<&'a str>,
    hints: HashMap<WsId, String>,
    filtering: bool,
    /// The keyboard is in the tree.
    focused: bool,
    drag: Option<&'a Drag>,
}

pub fn sidebar<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    let model = ctx.model;
    let state = model.state();
    let projects: Vec<&Workspace> = state
        .workspaces
        .iter()
        .filter(|ws| ws.hq != Some(true))
        .collect();
    let all: Vec<&Workspace> = state.workspaces.iter().collect();
    let tree = build_nav_tree(&NavInput {
        workspaces: &projects,
        folders: &state.folders,
        query: &st.query,
        attention_only: st.attention_only,
        wants_you: &|pane| reads::wants_you(model, &pane.to_string()),
    });
    let frame = Frame {
        st,
        ctx,
        active: model.active_workspace().map(|ws| ws.id.as_str()),
        hints: namesake_hints(&all),
        filtering: tree.filtering,
        focused: model.ui.focus == Some(Focus::Region(Region::Navigator)),
        drag: st.drag.as_ref(),
    };

    let mut list = column![].spacing(geometry::GAP).width(Length::Fill);
    for ws in &tree.loose {
        list = list.push(workspace_row(&frame, ws));
    }
    for entry in &tree.folders {
        list = list.push(folder_block(&frame, entry));
    }

    // The empty states, under the list.
    let tr = ctx.i18n;
    let mut below = column![].spacing(4).width(Length::Fill);
    if let Some((_, note)) = &st.note {
        below = below.push(message(t, note.clone()));
    }
    if projects.is_empty() {
        below = below.push(message(t, tr.t(Key::SidebarEmpty).into_owned()));
    }
    if tree.filtering && !projects.is_empty() && tree.rows.is_empty() {
        let said = st.query.trim();
        let words = if said.is_empty() {
            tr.t(Key::SidebarNoneWaiting).into_owned()
        } else {
            tr.t_with(Key::SidebarNoMatch, &args!("query" => said))
                .into_owned()
        };
        below = below.push(
            column![
                text(words).size(font_size::UI).color(t.colors.fg_muted),
                ui_button::link(
                    t,
                    tr.t(Key::SidebarClearFilter).into_owned(),
                    msg(Msg::ClearFilter)
                ),
            ]
            .spacing(4),
        );
    }

    // The space under the list: dropping there takes a workspace out of its
    // folder too, back with the loose ones at the top.
    let dragging_workspace = matches!(frame.drag, Some(d) if d.item.kind == DragKind::Workspace);
    let root_hot = matches!(
        frame.drag.and_then(|d| d.target.as_ref()),
        Some(DropTarget::Root { heading: false })
    );
    let root_label: Element<'a, Message> = if dragging_workspace {
        text(tr.t(Key::SidebarNoFolder).into_owned())
            .size(font_size::CAPTION)
            .color(t.colors.fg_muted)
            .into()
    } else {
        Space::new().into()
    };
    let root = {
        let zone = container(root_label)
            .width(Length::Fill)
            .height(ROW_HEIGHT)
            .padding(Padding {
                top: 6.0,
                ..Padding::ZERO
            })
            .align_x(Alignment::Center)
            .style(move |theme: &iced::Theme| {
                if root_hot {
                    skin::drop_target(t)(theme)
                } else {
                    container::Style::default()
                }
            });
        let zone = if frame.drag.is_some() {
            mouse_area(zone).on_move(|point| {
                msg(Msg::RowMove {
                    row: "root".into(),
                    point,
                    hit: Hit::Root { heading: false },
                })
            })
        } else {
            mouse_area(zone)
        };
        container(zone).padding(Padding {
            top: 4.0,
            ..Padding::ZERO
        })
    };

    let scroll = scrollable(
        column![list, below, root]
            .spacing(space::XS)
            .padding(Padding {
                top: geometry::LIST_PAD_TOP,
                right: PAD,
                bottom: 6.0,
                left: PAD,
            }),
    )
    .height(Length::Fill)
    .id(list_id())
    .on_scroll(|viewport| {
        msg(Msg::Scrolled {
            offset: viewport.absolute_offset().y,
            height: viewport.bounds().height,
        })
    })
    .style(style::scrollable::default(t));
    // The pointer leaving the list ends the target a drag would land on.
    let scroll = mouse_area(scroll).on_exit(msg(Msg::ListExit));

    let mut body = column![
        top(&frame),
        manager_door(&frame),
        heading(&frame, &tree.folders),
        scroll,
    ]
    .width(Length::Fill)
    .height(Length::Fill);
    if let Some(block) = plan::view(reads::plan(model), ctx) {
        body = body.push(block);
    }
    body = body.push(filter_bar(&frame));
    body.into()
}

/// A line of the kind `.nav-empty` is.
fn message<'a>(t: &'a Theme, words: String) -> Element<'a, Message> {
    container(text(words).size(font_size::UI).color(t.colors.fg_muted))
        .padding([12.0, PAD])
        .into()
}

// ---- The top ----------------------------------------------------------------------------------

/// The mark, the name, the version and the button that hides the navigator,
/// level with the toolbar beside it so the window reads as one band.
fn top<'a>(f: &Frame<'a>) -> Element<'a, Message> {
    let t = f.ctx.theme;
    let tr = f.ctx.i18n;
    let version = env!("CARGO_PKG_VERSION");
    row![
        logo(t, 15.0),
        text("Hangar.AI")
            .size(font_size::UI_LG)
            .font(fonts::ui_semibold())
            .wrapping(text::Wrapping::None),
        tip::bottom(
            t,
            text(version)
                .size(font_size::MICRO)
                .font(fonts::mono())
                .color(t.colors.fg_subtle),
            tr.t_with(Key::SidebarVersion, &args!("version" => version))
                .into_owned(),
        ),
        Space::new().width(Length::Fill),
        ui_button::icon_button(t, Icon::SidebarLeft)
            .tip(tr.t(Key::SidebarHide).into_owned())
            .keys(reads::binding(&f.ctx, CommandId::ViewNavigator))
            .icon_size(16.0)
            .on_press(msg(Msg::HideNavigator)),
    ]
    .spacing(7)
    .align_y(Alignment::Center)
    .height(size::TOOLBAR_HEIGHT)
    .padding(Padding {
        top: 0.0,
        right: 10.0,
        bottom: 0.0,
        left: 16.0,
    })
    .into()
}

/// The project manager's HQ: one click opens it, made if it was not there.
/// Apart from the list, which is the projects'.
fn manager_door<'a>(f: &Frame<'a>) -> Element<'a, Message> {
    let t = f.ctx.theme;
    let tr = f.ctx.i18n;
    let hq = f
        .ctx
        .model
        .state()
        .workspaces
        .iter()
        .find(|ws| ws.hq == Some(true));
    let current = hq.is_some_and(|hq| Some(hq.id.as_str()) == f.active);
    let door = button(
        row![
            icon(Icon::Orchestrator, 14.0),
            text(tr.t(Key::ManagerOpen).into_owned()).size(font_size::UI),
        ]
        .spacing(PAD)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([6.0, PAD])
    .on_press(Message::Intent(Intent::OpenManager))
    .style(move |_: &iced::Theme, status| {
        let lit = current || matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: lit.then_some(Background::Color(t.colors.surface_hover)),
            text_color: t.colors.accent_hi,
            border: skin::border(t.colors.accent_hi, radius::MD),
            ..button::Style::default()
        }
    });
    container(tip::bottom(
        t,
        door,
        tr.t(Key::ManagerOpenHint).into_owned(),
    ))
    .padding(Padding {
        top: 0.0,
        right: PAD,
        bottom: 6.0,
        left: PAD,
    })
    .into()
}

/// The heading of the workspaces filed nowhere, which sit right under it:
/// dropping one on it takes it out of its folder.
fn heading<'a>(f: &Frame<'a>, folders: &[NavFolder<'a>]) -> Element<'a, Message> {
    let t = f.ctx.theme;
    let tr = f.ctx.i18n;
    let hot = matches!(
        f.drag.and_then(|d| d.target.as_ref()),
        Some(DropTarget::Root { heading: true })
    );
    let dragging_workspace = matches!(f.drag, Some(d) if d.item.kind == DragKind::Workspace);
    let has_folders = !f.ctx.model.state().folders.is_empty() || !folders.is_empty();
    let label = row![
        text(tr.t(Key::SidebarWorkspaces).to_uppercase())
            .size(font_size::LABEL)
            .font(fonts::ui_semibold())
            .color(if hot { t.colors.fg } else { t.colors.fg_muted }),
        Space::new().width(Length::Fill),
        ui_button::icon_button(t, Icon::FolderPlus)
            .small()
            .tip(tr.t(Key::SidebarNewFolder).into_owned())
            .on_press(msg(Msg::NewFolder)),
    ]
    .align_y(Alignment::Center);
    let band = container(label)
        .height(ROW_HEIGHT)
        .padding(Padding {
            top: 0.0,
            right: 2.0,
            bottom: 0.0,
            left: PAD,
        })
        .align_y(Alignment::Center)
        .style(move |theme: &iced::Theme| {
            if hot {
                skin::drop_target(t)(theme)
            } else {
                container::Style::default()
            }
        });
    let band: Element<'a, Message> = if dragging_workspace && has_folders {
        mouse_area(band)
            .on_move(|point| {
                msg(Msg::RowMove {
                    row: "heading".into(),
                    point,
                    hit: Hit::Root { heading: true },
                })
            })
            .into()
    } else {
        band.into()
    };
    container(band)
        .padding(Padding {
            top: 2.0,
            right: PAD,
            bottom: 0.0,
            left: PAD,
        })
        .into()
}

// ---- Rows -------------------------------------------------------------------------------------

/// A row's frame: its content on the row's surface, the insertion marker a
/// drag would leave beside it, and the pointer's doings.
#[allow(clippy::too_many_arguments)]
fn row_shell<'a>(
    f: &Frame<'a>,
    kind: RowKind,
    id: &str,
    content: Element<'a, Message>,
    active: bool,
    marker: Option<bool>,
    marker_x: f32,
    drop_into: bool,
) -> Element<'a, Message> {
    let t = f.ctx.theme;
    let key = row_key(kind, id);
    let hovered = f.st.hover.as_deref() == Some(key.as_str());
    let dragged = f.drag.is_some_and(|d| {
        d.active
            && d.item.id == id
            && matches!(
                (d.item.kind, kind),
                (DragKind::Workspace, RowKind::Ws) | (DragKind::Folder, RowKind::Folder)
            )
    });
    let focused = f.focused && f.st.cursor.as_deref() == Some(key.as_str());
    let surface = container(content)
        .width(Length::Fill)
        .height(ROW_HEIGHT)
        .padding(Padding {
            top: 0.0,
            right: 4.0,
            bottom: 0.0,
            left: PAD,
        })
        .align_y(Alignment::Center)
        .style(move |theme: &iced::Theme| {
            if drop_into {
                skin::drop_target(t)(theme)
            } else {
                skin::nav_row(t, active, hovered, dragged, focused)(theme)
            }
        });

    let marked: Element<'a, Message> = match marker {
        Some(after) => {
            let line = container(Space::new())
                .width(Length::Fill)
                .height(2)
                .style(skin::insertion(t));
            let overlay = container(line)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(Padding {
                    top: 0.0,
                    right: 4.0,
                    bottom: 0.0,
                    left: marker_x,
                })
                .align_y(if after {
                    Alignment::End
                } else {
                    Alignment::Start
                });
            stack![surface, overlay].into()
        }
        None => surface.into(),
    };

    let item = DragItem {
        kind: match kind {
            RowKind::Ws => DragKind::Workspace,
            RowKind::Folder => DragKind::Folder,
        },
        id: id.to_string(),
    };
    let hit = match kind {
        RowKind::Ws => Hit::Workspace(id.to_string()),
        RowKind::Folder => Hit::Folder(id.to_string()),
    };
    let mut area = mouse_area(marked)
        .on_press(msg(Msg::Press(item)))
        .on_release(msg(Msg::Release(key.clone())))
        .on_enter(msg(Msg::Hover(Some(key.clone()))))
        .on_exit(msg(Msg::Hover(None)));
    if kind == RowKind::Ws {
        area = area.on_double_click(msg(Msg::DoublePress(id.to_string())));
    }
    if f.drag.is_some() {
        let moving = key.clone();
        area = area.on_move(move |point| {
            msg(Msg::RowMove {
                row: moving.clone(),
                point,
                hit: hit.clone(),
            })
        });
    }
    context(f, kind, id, area.into())
}

/// The row's context menu: asked for with the right button, from the « … »,
/// or with the menu key (`openMenu`).
fn context<'a>(
    f: &Frame<'a>,
    kind: RowKind,
    id: &str,
    row: Element<'a, Message>,
) -> Element<'a, Message> {
    let t = f.ctx.theme;
    let open: Option<&MenuState> =
        f.st.menu
            .as_ref()
            .filter(|menu| menu.kind == kind && menu.id == id);
    let ctx = f.ctx;
    let owner = id.to_string();
    let filtering = f.filtering;
    let entries = move || match kind {
        RowKind::Folder => menu::folder_entries(ctx, &owner, filtering),
        RowKind::Ws => menu::workspace_entries(ctx, &owner),
    };
    let asked = id.to_string();
    ui_menu::context_menu(t, row, open.map(|menu| menu.at), entries)
        .from_keyboard(open.is_some_and(|menu| menu.from_keyboard))
        .on_open(move |at| {
            msg(Msg::OpenMenu {
                kind,
                id: asked.clone(),
                at,
                from_keyboard: false,
            })
        })
        .on_dismiss(msg(Msg::CloseMenu))
        .into()
}

/// The « … » of a row: the menu a right-click opens, for those who would not
/// think to. It shows under the pointer, or while its menu is open.
fn more_button<'a>(f: &Frame<'a>, kind: RowKind, id: &str) -> Element<'a, Message> {
    let t = f.ctx.theme;
    ui_button::icon_button(t, Icon::More)
        .small()
        .icon_size(14.0)
        .tip(f.ctx.t(Key::SidebarMore).into_owned())
        .on_press(msg(Msg::OpenMenu {
            kind,
            id: id.to_string(),
            // Under the button, at the row's end.
            at: Vector::new(size::SIDEBAR_WIDTH - 2.0 * PAD - 30.0, ROW_HEIGHT + 4.0),
            from_keyboard: false,
        }))
        .into()
}

/// A name typed over a row's: Enter saves, Escape cancels.
fn rename_field<'a>(f: &Frame<'a>, draft: &'a str) -> Element<'a, Message> {
    let t = f.ctx.theme;
    let input = text_input("", draft)
        .id(rename_id())
        .size(font_size::UI)
        .padding([2.0, 5.0])
        .width(Length::Fill)
        .on_input(|draft| msg(Msg::RenameInput(draft)))
        .on_submit(msg(Msg::RenameSubmit))
        .style(style::text_input::field(t));
    keys::on_key(input, |key, _| match key {
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) => {
            Some(msg(Msg::RenameCancel))
        }
        _ => None,
    })
}

fn workspace_row<'a>(f: &Frame<'a>, ws: &'a Workspace) -> Element<'a, Message> {
    let t = f.ctx.theme;
    let tr = f.ctx.i18n;
    let model = f.ctx.model;
    let active = Some(ws.id.as_str()) == f.active;
    let key = row_key(RowKind::Ws, &ws.id);

    let glyph_cell = container(icon(Icon::Workspace, 14.0).color(if active {
        t.colors.accent_hi
    } else {
        t.colors.fg_muted
    }))
    .width(16)
    .align_x(Alignment::Center);

    let st: &'a State = f.st;
    let renaming = st
        .rename
        .as_ref()
        .filter(|rename| rename.kind == RowKind::Ws && rename.id == ws.id);
    let content: Element<'a, Message> = match renaming {
        Some(rename) => row![glyph_cell, rename_field(f, &rename.draft)]
            .spacing(PAD)
            .align_y(Alignment::Center)
            .into(),
        None => {
            let mut label = row![
                text(ws.name.as_str())
                    .size(font_size::UI_LG)
                    .font(if active {
                        fonts::ui_semibold()
                    } else {
                        fonts::ui_medium()
                    })
                    .wrapping(text::Wrapping::None)
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center);
            // After a name another workspace has too, the folder that tells
            // them apart.
            if let Some(hint) = f.hints.get(&ws.id) {
                label = label.push(
                    text(hint.clone())
                        .size(font_size::CAPTION)
                        .color(t.colors.fg_muted)
                        .wrapping(text::Wrapping::None),
                );
            }

            // Whatever ends the row: marks, the agents' summary, the « … ».
            let mut end = row![].spacing(2).align_y(Alignment::Center);
            if ws.claude_account.is_some() {
                end = end.push(crate::accounts::badge(f.ctx, &ws.id));
            }
            if let Some(orchestrator) = orchestrator_of(ws) {
                let workers = workers_of(ws).len();
                let words = if workers > 0 {
                    tr.t_with(
                        Key::OrchNavMarkWorkers,
                        &args!("name" => orchestrator.name.as_str(), "n" => workers),
                    )
                } else {
                    tr.t_with(
                        Key::OrchNavMark,
                        &args!("name" => orchestrator.name.as_str()),
                    )
                };
                end = end.push(mark(t, Icon::Orchestrator, words.into_owned()));
            }
            if ws.detached == Some(true) {
                end = end.push(mark(
                    t,
                    Icon::Window,
                    tr.t(Key::WindowInOwnWindow).into_owned(),
                ));
            }
            if let Some(at) = reads::asleep_at(model, ws) {
                let words = tr.t_with(
                    Key::SidebarAsleep,
                    &args!("time" => prose_time(at, tr.locale())),
                );
                end = end.push(mark(t, Icon::Moon, words.into_owned()));
            }
            if let Some(summary) = status::view(f.ctx, &[ws]) {
                end = end.push(summary);
            }
            let menu_open =
                f.st.menu
                    .as_ref()
                    .is_some_and(|menu| menu.kind == RowKind::Ws && menu.id == ws.id);
            if f.st.hover.as_deref() == Some(key.as_str()) || menu_open {
                end = end.push(more_button(f, RowKind::Ws, &ws.id));
            }
            row![
                glyph_cell,
                container(label).width(Length::Fill).clip(true),
                end
            ]
            .spacing(PAD)
            .align_y(Alignment::Center)
            .into()
        }
    };

    let marker = drag::marker(f.drag, DragKind::Workspace, &ws.id);
    let shell = row_shell(
        f,
        RowKind::Ws,
        &ws.id,
        content,
        active,
        marker,
        NAME_X,
        false,
    );
    // The folder it opens in, which the row does not spell out: only a
    // tooltip, for the rare look it is worth.
    tip::right(t, shell, ws.cwd.clone())
}

/// A small icon at the end of a row with a word in its tooltip (`.nav-mark`).
fn mark<'a>(t: &'a Theme, which: Icon, words: String) -> Element<'a, Message> {
    tip::bottom(
        t,
        container(icon(which, 12.0).color(t.colors.fg_muted))
            .width(18)
            .align_x(Alignment::Center),
        words,
    )
}

fn folder_block<'a>(f: &Frame<'a>, entry: &NavFolder<'a>) -> Element<'a, Message> {
    let t = f.ctx.theme;
    let tr = f.ctx.i18n;
    let folder = entry.folder;
    let key = row_key(RowKind::Folder, &folder.id);
    let filed: Vec<&Workspace> = f
        .ctx
        .model
        .state()
        .workspaces
        .iter()
        .filter(|ws| ws.folder_id.as_deref() == Some(folder.id.as_str()))
        .collect();
    // Folded away, the workspace on screen would otherwise be nowhere to see.
    let holds_active = !entry.expanded && filed.iter().any(|ws| Some(ws.id.as_str()) == f.active);
    let into = matches!(
        f.drag.and_then(|d| d.target.as_ref()),
        Some(DropTarget::Into { folder: target }) if *target == folder.id
    );

    let st: &'a State = f.st;
    let renaming = st
        .rename
        .as_ref()
        .filter(|rename| rename.kind == RowKind::Folder && rename.id == folder.id);
    let ink = if holds_active || into {
        t.colors.fg
    } else if f.st.hover.as_deref() == Some(key.as_str()) {
        t.colors.fg_secondary
    } else {
        t.colors.fg_muted
    };
    let head: Element<'a, Message> = match renaming {
        Some(rename) => rename_field(f, &rename.draft),
        None => {
            let mut head = row![
                text(folder.name.to_uppercase())
                    .size(font_size::LABEL)
                    .font(fonts::ui_semibold())
                    .color(ink)
                    .wrapping(text::Wrapping::None),
                icon(Icon::ChevronRight, 10.0)
                    .color(if holds_active {
                        t.colors.fg
                    } else {
                        t.colors.fg_subtle
                    })
                    .rotate(if entry.expanded {
                        std::f32::consts::FRAC_PI_2
                    } else {
                        0.0
                    }),
                Space::new().width(Length::Fill),
            ]
            .spacing(4)
            .align_y(Alignment::Center);
            // Folded, it still says when one of its agents wants you; its size
            // is for when it does not.
            let summary = (!entry.expanded)
                .then(|| status::view(f.ctx, &filed))
                .flatten();
            match summary {
                Some(summary) => head = head.push(summary),
                None => {
                    head = head.push(tip::bottom(
                        t,
                        hangar_ui::badge::count(t, entry.size),
                        tr.tn(Key::SidebarFolderSize, entry.size, &[]).into_owned(),
                    ));
                }
            }
            let menu_open =
                f.st.menu
                    .as_ref()
                    .is_some_and(|menu| menu.kind == RowKind::Folder && menu.id == folder.id);
            if f.st.hover.as_deref() == Some(key.as_str()) || menu_open {
                head = head.push(more_button(f, RowKind::Folder, &folder.id));
            }
            head.into()
        }
    };
    let marker = drag::marker(f.drag, DragKind::Folder, &folder.id);
    let head = row_shell(
        f,
        RowKind::Folder,
        &folder.id,
        head,
        false,
        marker,
        PAD,
        into,
    );

    let mut block = column![Space::new().height(geometry::FOLDER_GAP), head].spacing(geometry::GAP);
    if entry.expanded {
        let mut body = column![].spacing(geometry::GAP);
        for ws in &entry.workspaces {
            body = body.push(workspace_row(f, ws));
        }
        if entry.size == 0 {
            let empty = container(
                text(tr.t(Key::SidebarFolderEmpty).into_owned())
                    .size(font_size::CAPTION)
                    .color(t.colors.fg_muted),
            )
            .height(geometry::PLACEHOLDER_HEIGHT)
            .width(Length::Fill)
            .padding([0.0, PAD])
            .align_y(Alignment::Center)
            .style(skin::placeholder(t));
            let empty: Element<'a, Message> = if f.drag.is_some() {
                let target = folder.id.clone();
                mouse_area(empty)
                    .on_move(move |point| {
                        msg(Msg::RowMove {
                            row: format!("body:{target}"),
                            point,
                            hit: Hit::FolderBody(target.clone()),
                        })
                    })
                    .into()
            } else {
                empty.into()
            };
            body = body.push(container(empty).padding(Padding {
                bottom: geometry::PLACEHOLDER_GAP,
                ..Padding::ZERO
            }));
        }
        block = block.push(container(body).padding(Padding {
            bottom: geometry::BODY_PAD,
            ..Padding::ZERO
        }));
    }
    block.into()
}

// ---- The filter -------------------------------------------------------------------------------

/// The new-workspace button, the filter field with its « only what waits »
/// toggle, and the settings (`.nav-filter`): as tall as the status bar beside
/// it, so their top edges meet in one line.
fn filter_bar<'a>(f: &Frame<'a>) -> Element<'a, Message> {
    let t = f.ctx.theme;
    let tr = f.ctx.i18n;
    let st = f.st;
    let narrowing = !st.query.trim().is_empty() || st.attention_only;

    let input = text_input(&tr.t(Key::SidebarFilter), &st.query)
        .id(filter_id())
        .size(font_size::UI)
        .padding(0)
        .width(Length::Fill)
        .on_input(|query| msg(Msg::Query(query)))
        .style(style::text_input::bare(t));
    let input = keys::on_key(input, |key, _| match key {
        iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowDown) => {
            Some(msg(Msg::FilterDown))
        }
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) => {
            Some(msg(Msg::FilterEscape))
        }
        _ => None,
    });
    // The keymap goes first: Ctrl+Shift+Enter typed in the filter splits the
    // pane, and must not also type or submit here.
    let input = crate::keymap::guard(f.ctx, input);

    // The waiting state's own diamond: this keeps only what waits.
    let on = st.attention_only;
    let scope = button(
        container(
            glyph::state(t, reads::activity_glyph(AgentActivity::Waiting), 9.0).tint(if on {
                t.colors.state_waiting_fill
            } else {
                t.colors.fg_muted
            }),
        )
        .center(Length::Fill),
    )
    .padding(0)
    .width(20)
    .height(18)
    .on_press(msg(Msg::ToggleAttention))
    .style(move |_: &iced::Theme, status| {
        let lit = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: if on {
                Some(Background::Color(
                    t.colors.state_waiting_fill.scale_alpha(0.16),
                ))
            } else if lit {
                Some(Background::Color(t.colors.surface_hover))
            } else {
                None
            },
            border: iced::Border {
                radius: radius::XS.into(),
                ..iced::Border::default()
            },
            ..button::Style::default()
        }
    });
    let field = container(
        row![
            icon(Icon::Search, 14.0).color(if narrowing {
                t.colors.accent_hi
            } else {
                t.colors.fg_muted
            }),
            input,
            tip::bottom(t, scope, tr.t(Key::SidebarAttention).into_owned()),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center),
    )
    .height(24)
    .width(Length::Fill)
    .padding(Padding {
        top: 0.0,
        right: 3.0,
        bottom: 0.0,
        left: 8.0,
    })
    .align_y(Alignment::Center)
    .style(skin::field(t, narrowing));

    let bar = row![
        ui_button::icon_button(t, Icon::Plus)
            .icon_size(16.0)
            .tip(tr.t(Key::CmdWorkspaceNew).into_owned())
            .keys(reads::binding(&f.ctx, CommandId::WorkspaceNew))
            .on_press(Message::Intent(Intent::OpenOverlay(
                crate::shell::Overlay::WorkspaceDialog
            ))),
        field,
        ui_button::icon_button(t, Icon::Gear)
            .icon_size(16.0)
            .tip(tr.t(Key::SidebarSettings).into_owned())
            .keys(reads::binding(&f.ctx, CommandId::ViewSettings))
            .on_press(Message::Intent(Intent::OpenSettings(None))),
    ]
    .spacing(space::XS)
    .align_y(Alignment::Center)
    .height(size::STATUS_BAR_HEIGHT)
    .padding([0.0, 6.0]);

    column![
        container(Space::new())
            .width(Length::Fill)
            .height(1)
            .style(move |_: &iced::Theme| container::Style {
                background: Some(Background::Color(t.colors.sep)),
                ..container::Style::default()
            }),
        bar
    ]
    .into()
}
