//! The board's two surfaces (`BoardView.tsx`): the full view, four columns
//! side by side that stack when the room is narrow, and the dock, a panel of
//! sections that fold.

use std::collections::HashMap;

use hangar_i18n::{Key, args};
use hangar_theme::tokens::{font_size, size, space};
use hangar_theme::{Theme, fonts, style};
use hangar_ui::button;
use hangar_ui::icon::{Icon, icon};
use iced::alignment::{Horizontal, Vertical};
use iced::widget::{
    Space, column, container, mouse_area, responsive, row, rule, scrollable, stack, text,
    text_input,
};
use iced::{Alignment, Element, Length, Padding, Radians};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::GlyphState;

use super::agent_pane::find_agent_pane;
use super::card;
use super::dock;
use super::filter;
use super::format::now_ms;
use super::look;
use super::model::{Column, Task, index_by_id};
use super::select;
use super::{Msg, State};

/// The pane an assignee leads to, as a card names it (`AssigneePane`).
#[derive(Debug, Clone)]
pub struct AssigneePane {
    pub workspace: String,
    pub id: String,
    pub name: String,
    /// The agent running there, "Claude Code"; dropped when the card is narrow.
    pub agent: &'static str,
    pub state: GlyphState,
}

/// What a frame of the board is drawn from: the model, the cache and the
/// work worked out once per frame rather than once per card.
pub(super) struct View<'a> {
    pub st: &'a State,
    pub ctx: Ctx<'a>,
    pub all: &'a [Task],
    /// `false` until the first read answers.
    pub loaded: bool,
    pub shown: Vec<&'a Task>,
    pub by_id: HashMap<&'a str, &'a Task>,
    /// The pane each assignee on the board leads to, once per assignee: the
    /// same few agents hold most of the tasks.
    pub panes: HashMap<&'a str, Option<AssigneePane>>,
    /// The time the frame is drawn at, for the clocks of cards in progress.
    pub now: i64,
    pub dock: bool,
}

impl<'a> View<'a> {
    pub fn new(st: &'a State, ctx: Ctx<'a>, cwd: &str, dock: bool) -> View<'a> {
        let tasks = ctx.model.live.board.tasks(cwd);
        let all: &'a [Task] = tasks.unwrap_or(&[]);
        let shown = select::shown(all, &st.filters);
        let mut panes = HashMap::new();
        for assignee in all.iter().filter_map(|task| task.assignee.as_deref()) {
            panes
                .entry(assignee)
                .or_insert_with(|| pane_for(ctx, assignee));
        }
        View {
            st,
            ctx,
            loaded: tasks.is_some(),
            by_id: index_by_id(all),
            all,
            shown,
            panes,
            now: now_ms() as i64,
            dock,
        }
    }

    pub fn t(&self) -> &'a Theme {
        self.ctx.theme
    }

    pub fn pane(&self, assignee: &str) -> Option<&AssigneePane> {
        self.panes.get(assignee).and_then(Option::as_ref)
    }
}

/// `paneFor`: the pane an assignee's name leads to, if one clearly does.
pub(super) fn pane_for(ctx: Ctx<'_>, assignee: &str) -> Option<AssigneePane> {
    let state = ctx.model.state();
    let target = find_agent_pane(
        assignee,
        &state.workspaces,
        state.active_workspace_id.as_deref(),
    )?;
    let workspace = ctx.model.workspace(&target.workspace_id)?;
    let pane = workspace.panes.iter().find(|p| p.id == target.pane_id)?;
    Some(AssigneePane {
        workspace: workspace.id.clone(),
        id: pane.id.clone(),
        name: pane.name.clone(),
        agent: pane.agent.info().label,
        state: crate::panes_logic::state_glyph_state(ctx.model, &pane.id),
    })
}

/// The board of a workspace folder: full, or in the dock with the room its
/// grip may take (the width it has now and the widest it may be).
pub(super) fn board<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    cwd: &str,
    dock_room: Option<(f32, f32)>,
) -> Element<'a, Message> {
    let in_dock = dock_room.is_some();
    let v = View::new(st, ctx, cwd, in_dock);
    let t = v.t();
    let body: Element<'a, Message> = if in_dock {
        dock_body(v)
    } else {
        responsive(move |size| full_body(&v, size.width)).into()
    };
    let surface = container(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(look::surface(t, in_dock));
    match dock_room {
        Some((width, max)) => dock::panel(st, ctx, width, max, surface.into()),
        None => surface.into(),
    }
}

// ---- Full ------------------------------------------------------------------------------------

fn full_body<'a>(v: &View<'a>, width: f32) -> Element<'a, Message> {
    let t = v.t();
    let compact = width < look::COMPACT_BELOW;
    let stacked = width < look::STACK_BELOW;

    let content: Element<'a, Message> = if !v.loaded {
        skeleton(v)
    } else if v.all.is_empty() && !v.st.composing {
        empty(v)
    } else if stacked {
        scrollable(
            column(
                Column::ALL
                    .into_iter()
                    .map(|c| column_view(v, c, false, true)),
            )
            .width(Length::Fill),
        )
        .height(Length::Fill)
        .style(style::scrollable::default(t))
        .into()
    } else {
        row(Column::ALL
            .into_iter()
            .map(|c| column_view(v, c, false, false)))
        .height(Length::Fill)
        .into()
    };

    let page = column![
        bar_full(v, compact),
        rule::horizontal(1).style(style::rule::sep(t)),
        content,
    ];
    with_toast(v, page.into())
}

fn bar_full<'a>(v: &View<'a>, compact: bool) -> Element<'a, Message> {
    let t = v.t();
    let tr = v.ctx.i18n;
    let title = text(if v.loaded {
        count_text(v)
    } else {
        tr.t(Key::BoardTitle).into_owned()
    })
    .size(font_size::BODY)
    .font(fonts::ui_semibold());

    let mut bar = row![title].spacing(space::MD).align_y(Alignment::Center);
    // Nothing to filter before the first read, or on an empty board.
    if v.st.filters.active() || !v.all.is_empty() {
        bar = bar.push(filter::group(v));
    }
    bar = bar.push(Space::new().width(Length::Fill));

    let mut mcp_label = row![icon(Icon::Plug, 12.0)]
        .spacing(space::SM)
        .align_y(Alignment::Center);
    if !compact {
        mcp_label = mcp_label.push(text(tr.t(Key::BoardHint)).size(font_size::CAPTION_LG));
    }
    bar = bar.push(hangar_ui::tip::bottom(
        t,
        iced::widget::button(look::centred(mcp_label))
            .padding([0.0, space::SM])
            .height(24.0)
            .style(look::quiet_button(t))
            .on_press(Message::Board(Msg::OpenMcp)),
        tr.t(Key::BoardMcp),
    ));
    bar = bar.push(
        button::secondary(t, tr.t(Key::BoardAddTask))
            .icon(Icon::Plus)
            .small()
            .on_press(Message::Board(Msg::StartAdding(Column::Todo))),
    );

    container(bar)
        .padding(Padding {
            top: 0.0,
            right: space::XL,
            bottom: 0.0,
            left: space::XXL,
        })
        .center_y(look::BAR_HEIGHT)
        .style(style::container::pane_head(t, false))
        .into()
}

/// "12 tasks", or "3 of 12 tasks" while a filter hides some.
fn count_text(v: &View<'_>) -> String {
    let tr = v.ctx.i18n;
    let n = v.all.len();
    if v.st.filters.active() {
        tr.tn(Key::BoardShown, n, &args!("shown" => v.shown.len()))
            .into_owned()
    } else {
        tr.tn(Key::BoardTasks, n, &[]).into_owned()
    }
}

// ---- Dock ------------------------------------------------------------------------------------

fn dock_body<'a>(v: View<'a>) -> Element<'a, Message> {
    let t = v.t();
    let tr = v.ctx.i18n;
    let filtering = v.st.filters.active();

    let sub = v.loaded.then(|| {
        let words = if filtering {
            count_text(&v)
        } else {
            tr.tn(Key::BoardDockActive, select::active_count(&v.shown), &[])
                .into_owned()
        };
        text(words)
            .size(font_size::LABEL)
            .font(fonts::mono())
            .style(style::text::subtle(t))
    });

    let filter_tool: Element<'a, Message> = {
        let tool = button::toggle_button(t, Icon::Filter, v.st.filter_bar)
            .small()
            .tip(tr.t(Key::BoardFilters))
            .on_press(Message::Board(Msg::ToggleFilterBar));
        // Filters left on with their row folded away: the button says so.
        if filtering && !v.st.filter_bar {
            let dot = container(
                container(Space::new())
                    .width(5.0)
                    .height(5.0)
                    .style(look::flag(t)),
            )
            .width(size::CONTROL_HEIGHT_SM)
            .height(size::CONTROL_HEIGHT_SM)
            .padding(3.0)
            .align_x(Horizontal::Right)
            .align_y(Vertical::Top);
            stack![tool, dot].into()
        } else {
            tool.into()
        }
    };

    let mut tools = row![
        text(tr.t(Key::BoardTitle))
            .size(font_size::UI)
            .font(fonts::ui_semibold())
    ]
    .spacing(space::SM)
    .align_y(Alignment::Center);
    if let Some(sub) = sub {
        tools = tools.push(sub);
    }
    let tools = tools
        .push(Space::new().width(Length::Fill))
        .push(filter_tool)
        .push(
            button::icon_button(t, Icon::Expand)
                .small()
                .tip(tr.t(Key::BoardDockExpand))
                .on_press(Message::Board(Msg::Expand)),
        )
        .push(
            button::icon_button(t, Icon::Close)
                .small()
                .tip(tr.t(Key::BoardDockHide))
                .on_press(Message::Board(Msg::HideDock)),
        );
    let bar = container(tools)
        .padding(Padding {
            top: 0.0,
            right: space::XS,
            bottom: 0.0,
            left: space::XL,
        })
        .center_y(look::DOCK_BAR_HEIGHT)
        .style(style::container::pane_head(t, false));

    let mut page = column![bar, rule::horizontal(1).style(style::rule::hairline(t))];
    if v.st.filter_bar {
        page = page
            .push(
                container(filter::group(&v))
                    .padding([space::SM, space::LG])
                    .width(Length::Fill),
            )
            .push(rule::horizontal(1).style(style::rule::hairline(t)));
    }

    let content: Element<'a, Message> = if !v.loaded {
        skeleton(&v)
    } else if v.all.is_empty() && !v.st.composing {
        empty(&v)
    } else {
        scrollable(
            column(
                Column::DOCK_ORDER
                    .into_iter()
                    .map(|c| column_view(&v, c, true, true)),
            )
            .padding(Padding {
                top: space::XS,
                bottom: space::LG,
                ..Padding::ZERO
            })
            .width(Length::Fill),
        )
        .height(Length::Fill)
        .style(style::scrollable::default(t))
        .into()
    };
    page = page.push(content);
    with_toast(&v, page.into())
}

// ---- Columns ---------------------------------------------------------------------------------

/// A column. In the dock it is a section that folds; `flow` lets its cards
/// run the length of the page instead of scrolling inside the column.
fn column_view<'a>(v: &View<'a>, which: Column, in_dock: bool, flow: bool) -> Element<'a, Message> {
    let t = v.t();
    let tr = v.ctx.i18n;
    let items = select::column_items(&v.shown, which);
    let folded = in_dock && v.st.folded.contains(&which);
    let name = tr.t(which.label_key()).into_owned();
    let over =
        v.st.press
            .as_ref()
            .is_some_and(|press| press.moved && press.over == Some(which));
    let add_label = tr
        .t_with(Key::BoardAddTo, &args!("column" => name.as_str()))
        .into_owned();

    let mark = container(Space::new())
        .width(look::MARK)
        .height(look::MARK)
        .style(look::mark(t, which, look::MARK));
    let count = text(items.len().to_string())
        .size(font_size::LABEL)
        .font(fonts::mono())
        .style(style::text::subtle(t));
    let plus = button::icon_button(t, Icon::Plus)
        .small()
        .icon_size(12.0)
        .tip(add_label.clone())
        .on_press(Message::Board(Msg::StartAdding(which)));

    let head: Element<'a, Message> = if in_dock {
        let caret = icon(Icon::ChevronRight, 10.0)
            .color(t.colors.fg_subtle)
            .rotate(if folded {
                Radians(0.0)
            } else {
                Radians(std::f32::consts::FRAC_PI_2)
            });
        let toggle = iced::widget::button(look::centred(
            row![
                caret,
                mark,
                text(name.clone())
                    .size(font_size::CAPTION)
                    .font(fonts::ui_semibold())
                    .style(style::text::muted(t)),
                count
            ]
            .spacing(7.0)
            .align_y(Alignment::Center),
        ))
        .width(Length::Fill)
        .height(24.0)
        .padding([0.0, space::SM])
        .style(look::fold_button(t))
        .on_press(Message::Board(Msg::Fold(which)));
        container(
            row![toggle, plus]
                .spacing(space::XS)
                .align_y(Alignment::Center),
        )
        .padding(Padding {
            left: space::SM,
            right: space::MD,
            ..Padding::ZERO
        })
        .center_y(look::DOCK_COLUMN_HEAD)
        .into()
    } else {
        container(
            row![
                mark,
                text(name.clone())
                    .size(font_size::UI)
                    .font(fonts::ui_semibold()),
                count,
                Space::new().width(Length::Fill),
                plus,
            ]
            .spacing(space::MD)
            .align_y(Alignment::Center),
        )
        .padding(Padding {
            left: 14.0,
            right: space::MD,
            ..Padding::ZERO
        })
        .center_y(look::COLUMN_HEAD)
        .into()
    };

    let mut parts = iced::widget::column![head];
    if !folded {
        let mut cards = iced::widget::column(items.iter().map(|task| card::card(v, task)))
            .spacing(if in_dock { 5.0 } else { 6.0 })
            .padding(Padding {
                top: 2.0,
                right: space::LG,
                bottom: if in_dock { space::MD } else { space::XL },
                left: space::LG,
            });
        if let Some(field) = add_field(v, which, items.is_empty(), add_label) {
            cards = cards.push(field);
        }
        parts = parts.push(if flow {
            Element::from(cards)
        } else {
            scrollable(cards)
                .height(Length::Fill)
                .style(style::scrollable::default(t))
                .into()
        });
    }

    let surface = container(parts)
        .width(Length::Fill)
        .height(if flow { Length::Shrink } else { Length::Fill })
        .style(look::column(t, over, in_dock));

    // While a card is pressed, each column says when the pointer is over it.
    let mut area = mouse_area(surface);
    if v.st.press.is_some() {
        area = area
            .on_enter(Message::Board(Msg::Enter(which)))
            .on_exit(Message::Board(Msg::Leave(which)));
    }
    if flow {
        // Sections one under another, the board's rule between them.
        iced::widget::column![area, rule::horizontal(1).style(style::rule::sep(t))]
            .width(Length::Fill)
            .into()
    } else {
        container(row![area, rule::vertical(1).style(style::rule::sep(t))])
            .width(Length::FillPortion(1))
            .max_width(look::COLUMN_MAX_WIDTH)
            .height(Length::Fill)
            .into()
    }
}

/// The row to type a card into. A column with nothing in it keeps it open: it
/// is the column's empty state. Otherwise it is there once it has been asked
/// for (the column's "+", the bar's "Add…") or holds something typed.
fn add_field<'a>(
    v: &View<'a>,
    column: Column,
    empty: bool,
    label: String,
) -> Option<Element<'a, Message>> {
    let t = v.t();
    let draft = v.st.drafts.get(&column).map_or("", String::as_str);
    if !(empty || !draft.is_empty() || v.st.adding == Some(column)) {
        return None;
    }
    let placeholder = v.ctx.t(Key::BoardAdd).into_owned();
    let field = text_input(&placeholder, draft)
        .id(super::add_id(column))
        .size(font_size::UI)
        .padding([6.0, space::LG])
        .width(Length::Fill)
        .on_input(move |value| Message::Board(Msg::Draft(column, value)))
        .on_submit(Message::Board(Msg::Submit(column)))
        .style(look::add_field(t));
    // Escape empties what was typed, and goes no further.
    let has_draft = !draft.is_empty();
    let field = hangar_ui::keys::on_key(field, move |key, _| match key {
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) if has_draft => {
            Some(Message::Board(Msg::ClearDraft(column)))
        }
        _ => None,
    });
    let field = hangar_ui::tip::over(t, field, label);
    // Something typed can be taken to the full form: a description, a
    // priority, the files its worker may change.
    if !has_draft {
        return Some(field);
    }
    let details = button::icon_button(t, Icon::Pencil)
        .small()
        .tip(v.ctx.t(Key::BoardAddDetails))
        .on_press(Message::Board(Msg::AddDetails(column)));
    Some(
        row![field, details]
            .spacing(space::XS)
            .align_y(Alignment::Center)
            .into(),
    )
}

// ---- Empty, skeleton, toast --------------------------------------------------------------------

/// No task yet: what the board is for, and the two ways to fill it.
fn empty<'a>(v: &View<'a>) -> Element<'a, Message> {
    let t = v.t();
    let tr = v.ctx.i18n;
    let dock = v.dock;
    let glyph = container(icon(Icon::Board, if dock { 18.0 } else { 22.0 }))
        .center(look::EMPTY_ICON_BOX)
        .style(look::empty_icon(t));
    let title = text(tr.t(Key::BoardEmptyTitle))
        .size(if dock {
            font_size::BODY
        } else {
            font_size::TITLE
        })
        .font(fonts::ui_semibold());
    let body = text(tr.t(Key::BoardEmptyBody))
        .size(if dock {
            font_size::UI
        } else {
            font_size::UI_LG
        })
        .style(style::text::muted(t))
        .align_x(Horizontal::Center)
        .width(Length::Fill);
    let actions = row![
        button::primary(t, tr.t(Key::BoardEmptyAdd))
            .icon(Icon::Plus)
            .small()
            .on_press(Message::Board(Msg::StartAdding(Column::Todo))),
        button::secondary(t, tr.t(Key::BoardEmptyMcp))
            .icon(Icon::Plug)
            .small()
            .on_press(Message::Board(Msg::OpenMcp)),
    ]
    .spacing(space::MD);
    let content = column![
        glyph,
        title,
        container(body).max_width(look::EMPTY_TEXT_WIDTH),
        container(actions).padding(Padding {
            top: space::XL,
            ..Padding::ZERO
        })
    ]
    .spacing(space::MD)
    .align_x(Alignment::Center);
    let page = container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding([36.0, 24.0])
        .center_x(Length::Fill);
    if dock {
        page.align_y(Vertical::Top).into()
    } else {
        page.center_y(Length::Fill).into()
    }
}

/// The columns' outline while the first read is out.
fn skeleton<'a>(v: &View<'a>) -> Element<'a, Message> {
    let t = v.t();
    let shape: &[usize] = if v.dock { &[2, 1] } else { &[3, 2, 1, 2] };
    let columns = shape.iter().map(|cards| {
        let label = container(Space::new())
            .width(look::SKELETON_LABEL.0)
            .height(look::SKELETON_LABEL.1)
            .style(look::skeleton(t));
        let cards = (0..*cards).map(|n| {
            container(Space::new())
                .width(Length::Fill)
                .height(if n % 2 == 1 {
                    look::SKELETON_CARD_SHORT
                } else {
                    look::SKELETON_CARD
                })
                .style(look::skeleton(t))
                .into()
        });
        container(
            column![
                container(label).center_y(look::COLUMN_HEAD),
                column(cards).spacing(6.0)
            ]
            .padding([0.0, space::LG]),
        )
        .width(Length::FillPortion(1))
        .into()
    });
    container(row(columns))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Why a move was refused, for a few seconds, at the foot of the board.
fn with_toast<'a>(v: &View<'a>, page: Element<'a, Message>) -> Element<'a, Message> {
    let t = v.t();
    let Some((_, words)) = &v.st.toast else {
        return page;
    };
    let toast = container(
        container(text(words.clone()).size(font_size::UI))
            .padding([space::MD, space::XL])
            .max_width(520.0)
            .style(look::toast(t)),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(space::XXL)
    .center_x(Length::Fill)
    .align_y(Vertical::Bottom);
    stack![page, toast].into()
}
