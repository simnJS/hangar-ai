//! The board's three filters (`BoardFilter.tsx`): a compact button reading
//! "Label  All ⇅" that opens a list to pick from.
//!
//! The list is hangar-ui's menu: the arrows walk it, Enter picks, Escape and
//! a click outside close it, and the pick is ticked, as the listbox of the web
//! app was.

use hangar_i18n::Key;
use hangar_theme::tokens::{font_size, space};
use hangar_theme::{fonts, style};
use hangar_ui::icon::{Icon, icon};
use hangar_ui::menu::{self, Entry};
use hangar_ui::tip;
use iced::widget::{button, row, text};
use iced::{Alignment, Element};

use crate::message::Message;

use super::select::{self, ANY, NOBODY, STATE_BLOCKED, STATE_FREE};
use super::view::View;
use super::{FilterKind, Msg};

/// A choice of a filter: `FilterOption` of the TS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opt {
    pub value: String,
    pub label: String,
    /// What the button says when this is picked, if shorter: "All".
    pub short: Option<String>,
    /// Said after the label, quieter: the pane an agent's name leads to.
    pub hint: Option<String>,
}

fn opt(value: &str, label: impl Into<String>) -> Opt {
    Opt {
        value: value.to_owned(),
        label: label.into(),
        short: None,
        hint: None,
    }
}

/// The options of a filter, for the tasks on the board.
pub(super) fn options(v: &View<'_>, kind: FilterKind) -> Vec<Opt> {
    let tr = v.ctx.i18n;
    let all = |label: Key| Opt {
        short: Some(tr.t(Key::BoardFilterAll).into_owned()),
        ..opt(ANY, tr.t(label))
    };
    match kind {
        FilterKind::Label => std::iter::once(all(Key::BoardAllLabels))
            .chain(
                select::labels(v.all, &v.st.filters.label)
                    .into_iter()
                    .map(|label| opt(&label, label.clone())),
            )
            .collect(),
        FilterKind::Assignee => {
            let named = select::assignees(v.all, &v.st.filters.assignee)
                .into_iter()
                .map(|name| {
                    // The pane's name, when it is not the name itself.
                    let hint = v
                        .pane(&name)
                        .filter(|pane| pane.name != name)
                        .map(|pane| pane.name.clone());
                    Opt {
                        hint,
                        ..opt(&name, name.clone())
                    }
                });
            std::iter::once(all(Key::BoardAllAssignees))
                .chain(std::iter::once(opt(NOBODY, tr.t(Key::BoardUnassigned))))
                .chain(named)
                .collect()
        }
        FilterKind::State => vec![
            all(Key::BoardStateAny),
            opt(STATE_BLOCKED, tr.t(Key::BoardStateBlocked)),
            opt(STATE_FREE, tr.t(Key::BoardStateFree)),
        ],
    }
}

/// The three filters, and the button that clears them while any is on.
pub(super) fn group<'a>(v: &View<'a>) -> Element<'a, Message> {
    let t = v.t();
    let tr = v.ctx.i18n;
    let mut filters = row![
        control(v, FilterKind::Label),
        control(v, FilterKind::Assignee),
        control(v, FilterKind::State),
    ]
    .spacing(space::SM)
    .align_y(Alignment::Center);
    if v.st.filters.active() {
        let clear = button(super::look::centred(
            row![
                icon(Icon::Close, 10.0),
                text(tr.t(Key::BoardClearFiltersShort)).size(font_size::CAPTION_LG),
            ]
            .spacing(5.0)
            .align_y(Alignment::Center),
        ))
        .height(24.0)
        .padding([0.0, space::MD])
        .style(super::look::clear_button(t))
        .on_press(Message::Board(Msg::ClearFilters));
        filters = filters.push(tip::bottom(t, clear, tr.t(Key::BoardClearFilters)));
    }
    filters.into()
}

/// One filter: its button, and its list while it is open.
fn control<'a>(v: &View<'a>, kind: FilterKind) -> Element<'a, Message> {
    let t = v.t();
    let tr = v.ctx.i18n;
    let (name, title, value) = match kind {
        FilterKind::Label => (
            Key::BoardFilterNameLabel,
            Key::BoardFilterLabel,
            &v.st.filters.label,
        ),
        FilterKind::Assignee => (
            Key::BoardFilterNameAgent,
            Key::BoardFilterAssignee,
            &v.st.filters.assignee,
        ),
        FilterKind::State => (
            Key::BoardFilterNameState,
            Key::BoardFilterState,
            &v.st.filters.state,
        ),
    };
    let options = options(v, kind);
    let active = value != ANY;
    let current = options
        .iter()
        .find(|option| &option.value == value)
        .or(options.first());
    let shown = current
        .map(|option| option.short.clone().unwrap_or_else(|| option.label.clone()))
        .unwrap_or_default();

    let mut name_text = text(tr.t(name)).size(font_size::UI);
    if !active {
        name_text = name_text.style(style::text::secondary(t));
    }
    let face = button(super::look::centred(
        row![
            name_text,
            text(shown).size(font_size::UI).font(fonts::ui_medium()),
            icon(Icon::Updown, 10.0).color(if active {
                t.colors.accent_hi
            } else {
                t.colors.fg_subtle
            }),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center),
    ))
    .height(24.0)
    .padding([0.0, 6.0])
    .style(super::look::filter_button(t, active))
    .on_press(Message::Board(Msg::OpenFilter(Some(kind))));

    let open = v.st.open_filter == Some(kind);
    let entries = open.then(|| {
        options
            .iter()
            .map(|option| {
                let label = match &option.hint {
                    Some(hint) => format!("{}  ·  {hint}", option.label),
                    None => option.label.clone(),
                };
                Entry::from(
                    menu::item(label)
                        .checked(&option.value == value)
                        .on_select(Message::Board(Msg::Filter(kind, option.value.clone()))),
                )
            })
            .collect::<Vec<_>>()
    });
    menu::dropdown(t, tip::bottom(t, face, tr.t(title)), entries)
        .on_dismiss(Message::Board(Msg::OpenFilter(None)))
        .into()
}
