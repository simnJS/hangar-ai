//! The tab strip of the panel's header: one tab per open file or kit tab, the
//! one in front joined to the panel under it (`PreviewTabs`, `KitBadgeGlyph`,
//! `.model-tab*`).
//!
//! Keyboard: after a tab is pressed, or with the panel focused (F6), Left and
//! Right cycle, Home and End jump, Delete closes the one in front (`TabKey`;
//! the arrows of the roving tabindex of the TS).

use std::f32::consts::TAU;

use iced::advanced::widget::operation::{Operation, Outcome, Scrollable};
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::{Space, button, container, row, scrollable, text};
use iced::{Alignment, Element, Length, Padding, Radians, Rectangle, Task, Vector};

use hangar_i18n::{Key, args};
use hangar_theme::{fonts, style, tokens};
use hangar_ui::{Icon, icon};

use super::glyph::{Glyph, glyph};
use super::store::{Entry, KitBadge, KitBadgeState, Scope, Status};
use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::WsId;

/// What a key does to the strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabKey {
    Next,
    Previous,
    First,
    Last,
    CloseActive,
}

/// The tab a key lands on: from the one in front, cycling (`onKeyDown`).
/// `None` for a key that closes, or when nothing is open.
pub fn target_of(key: TabKey, ids: &[String], active: Option<&str>) -> Option<usize> {
    if ids.is_empty() {
        return None;
    }
    let at = active
        .and_then(|active| ids.iter().position(|id| id == active))
        .unwrap_or(0);
    let len = ids.len();
    match key {
        TabKey::Next => Some((at + 1) % len),
        TabKey::Previous => Some((at + len - 1) % len),
        TabKey::First => Some(0),
        TabKey::Last => Some(len - 1),
        TabKey::CloseActive => None,
    }
}

pub fn strip_id() -> iced::widget::Id {
    iced::widget::Id::new("preview-tabs")
}

/// The widget id of a tab, by the id of what it shows.
pub fn tab_id(entry: &str) -> iced::widget::Id {
    iced::widget::Id::from(format!("preview-tab:{entry}"))
}

/// Where the strip must be scrolled to for a tab to be in view: `None` when
/// it already is. All in pixels along the strip: the tab's left edge and
/// width from the start of the strip's content, the strip's own width and
/// how far it is scrolled now. A tab wider than the strip is brought to its
/// left edge. The least movement that shows the tab: tabs have widths of
/// their own (a title may be long, a tab may say who opened it), so the
/// place of the tab cannot be told from its number.
pub fn reveal(tab_left: f32, tab_width: f32, view: f32, offset: f32) -> Option<f32> {
    if tab_left < offset || tab_width > view {
        Some(tab_left.max(0.0))
    } else if tab_left + tab_width > offset + view {
        Some(tab_left + tab_width - view)
    } else {
        None
    }
}

/// Finds a tab and the strip it is in, and says how far the strip is to be
/// scrolled for the tab to show (`reveal`).
struct Reveal {
    strip: iced::widget::Id,
    tab: iced::widget::Id,
    /// The strip's width, how far it is scrolled, and where its content starts.
    strip_view: Option<(f32, f32, f32)>,
    found: Option<Option<f32>>,
}

impl Operation<Option<f32>> for Reveal {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Option<f32>>)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&iced::widget::Id>, bounds: Rectangle) {
        if id != Some(&self.tab) {
            return;
        }
        if let Some((view, offset, start)) = self.strip_view {
            self.found = Some(reveal(bounds.x - start, bounds.width, view, offset));
        }
    }

    fn scrollable(
        &mut self,
        id: Option<&iced::widget::Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        translation: Vector,
        _state: &mut dyn Scrollable,
    ) {
        if id == Some(&self.strip) {
            self.strip_view = Some((bounds.width, translation.x, content_bounds.x));
        }
    }

    fn finish(&self) -> Outcome<Option<f32>> {
        match self.found {
            Some(found) => Outcome::Some(found),
            None => Outcome::None,
        }
    }
}

/// Scrolls the strip, if it needs to be, so that the tab of `entry` is in
/// view.
pub fn reveal_tab<M: Send + 'static>(entry: &str) -> Task<M> {
    let find = Reveal {
        strip: strip_id(),
        tab: tab_id(entry),
        strip_view: None,
        found: None,
    };
    iced::advanced::widget::operate(find).then(|found| match found {
        Some(x) => iced::widget::operation::scroll_to(
            strip_id(),
            AbsoluteOffset {
                x: Some(x),
                y: None,
            },
        ),
        None => Task::none(),
    })
}

/// The name of the pane that opened a tab, as the tab says it: `opened_by` is
/// a pane's id (or its name); the person reads the name.
pub fn opened_by_name(ctx: Ctx<'_>, ws: &str, opened_by: &str) -> String {
    ctx.model
        .workspace(ws)
        .and_then(|workspace| {
            workspace
                .panes
                .iter()
                .find(|pane| pane.id == opened_by)
                .map(|pane| pane.name.clone())
        })
        .unwrap_or_else(|| opened_by.to_string())
}

/// What a kit's tab says on hover: which kit, and what tells it from its
/// siblings (`kitTabHint`).
fn kit_hint(ctx: Ctx<'_>, ws: &str, entry: &Entry) -> String {
    let Some(kit) = entry.kit() else {
        return entry.path.clone();
    };
    let params: Vec<String> = kit
        .params
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    let mut lines = vec![
        ctx.i18n
            .t_with(
                Key::KitsTabTabHint,
                &args!("title" => entry.title.as_str(), "kit" => kit.kit_id.as_str()),
            )
            .into_owned(),
    ];
    // Beside several tabs, the strip may have cut it.
    if let Some(text) = kit.badge.as_ref().and_then(|badge| badge.text.as_ref()) {
        lines.push(text.clone());
    }
    if !params.is_empty() {
        lines.push(
            ctx.i18n
                .t_with(Key::KitsTabTabParams, &args!("params" => params.join(", ")))
                .into_owned(),
        );
    }
    if let Some(by) = &entry.opened_by {
        lines.push(
            ctx.i18n
                .t_with(
                    Key::ModelOpenedBy,
                    &args!("name" => opened_by_name(ctx, ws, by)),
                )
                .into_owned(),
        );
    }
    lines.join("\n")
}

fn tooltip(ctx: Ctx<'_>, ws: &str, entry: &Entry) -> String {
    if entry.is_kit_tab() {
        return kit_hint(ctx, ws, entry);
    }
    match &entry.opened_by {
        Some(by) => format!(
            "{}\n{}",
            entry.path,
            ctx.i18n.t_with(
                Key::ModelOpenedBy,
                &args!("name" => opened_by_name(ctx, ws, by))
            )
        ),
        None => entry.path.clone(),
    }
}

/// A ring the size of the icon it stands in for, turning (`.model-tab__spinner`).
fn spinner<'a, M: 'a>(t: &'a hangar_theme::Theme, spin: usize) -> Element<'a, M> {
    let angle = spin as f32 * TAU / 8.0;
    icon(Icon::Restart, 12.0)
        .color(t.colors.fg_muted)
        .rotate(Radians(angle))
        .into()
}

/// The state a kit's page gave its tab: a small glyph where the tab's icon
/// goes. Colour never says it alone: each state has its own shape.
fn badge_glyph<'a, M: 'a>(ctx: Ctx<'a>, badge: &KitBadge, spin: usize) -> Element<'a, M> {
    let t = ctx.theme;
    match badge.state {
        KitBadgeState::Running => spinner(t, spin),
        KitBadgeState::Failure => icon(Icon::Alert, 12.0).color(t.colors.danger).into(),
        KitBadgeState::Success => icon(Icon::Check, 12.0).color(t.colors.success).into(),
        KitBadgeState::Neutral => container(Space::new())
            .width(6.0)
            .height(6.0)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(t.colors.fg_subtle)),
                border: iced::Border {
                    radius: tokens::radius::FULL.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into(),
    }
}

/// The icon of a tab: a spinner while it loads, an alert on an error, a cube
/// for a model, a window for a kit (or its badge), a mark for a file.
fn mark<'a, M: 'a>(ctx: Ctx<'a>, entry: &'a Entry, active: bool, spin: usize) -> Element<'a, M> {
    let t = ctx.theme;
    let quiet = if active {
        t.colors.fg_muted
    } else {
        t.colors.fg_subtle
    };
    if entry.status == Status::Loading {
        return spinner(t, spin);
    }
    if entry.status == Status::Error {
        return icon(Icon::Alert, 12.0).color(t.colors.danger).into();
    }
    if entry.is_model() {
        return icon(Icon::Cube, 12.0).color(quiet).into();
    }
    if let Some(kit) = entry.kit() {
        return match &kit.badge {
            Some(badge) => badge_glyph(ctx, badge, spin),
            None => icon(Icon::Window, 12.0).color(quiet).into(),
        };
    }
    match entry.file_kind() {
        Some(kind) => glyph(Glyph::of_kind(kind), 12.0, quiet),
        None => icon(Icon::Window, 12.0).color(quiet).into(),
    }
}

/// The strip of tabs. A tab pressed is brought forward; its cross closes it.
pub fn view<'a>(ctx: Ctx<'a>, ws: &'a WsId, scope: &'a Scope, spin: usize) -> Element<'a, Message> {
    let t = ctx.theme;
    let alone = scope.entries.len() == 1;
    let mut tabs = row![].align_y(Alignment::Center).height(Length::Fill);
    for entry in &scope.entries {
        let active = scope.active_id.as_deref() == Some(entry.id.as_str());
        let mut label = row![
            mark(ctx, entry, active, spin),
            text(entry.title.as_str())
                .size(tokens::font_size::UI)
                .font(if active {
                    fonts::ui_semibold()
                } else {
                    fonts::ui_medium()
                })
                .wrapping(text::Wrapping::None),
        ]
        .spacing(tokens::space::SM)
        .align_y(Alignment::Center);
        // Beside several tabs it would be cut to "ou…" in each: only the one
        // in front says who opened it, the others in their tooltip. A kit's
        // tab says its state there instead, in any tab: who opened it stays
        // in its tooltip, as a tab per run needs every letter of its title.
        let aside = match entry.kit() {
            Some(kit) => kit.badge.as_ref().and_then(|badge| badge.text.clone()),
            None => entry
                .opened_by
                .as_ref()
                .filter(|_| active || alone)
                .map(|by| {
                    ctx.i18n
                        .t_with(
                            Key::ModelOpenedBy,
                            &args!("name" => opened_by_name(ctx, ws, by)),
                        )
                        .into_owned()
                }),
        };
        if let Some(aside) = aside {
            label = label.push(
                text(aside)
                    .size(tokens::font_size::LABEL)
                    .font(fonts::mono())
                    .style(style::text::subtle(t))
                    .wrapping(text::Wrapping::None),
            );
        }
        let main = button(container(label).center_y(Length::Fill).clip(true))
            .padding(Padding {
                left: tokens::space::LG,
                right: tokens::space::XS,
                ..Padding::ZERO
            })
            .height(Length::Fill)
            .style(style::button::tab_strip(t, active))
            .on_press(Message::Preview(super::Msg::Activate(
                ws.clone(),
                entry.id.clone(),
            )));
        let main = container(main).max_width(260.0);
        let close = button(container(icon(Icon::Close, 10.0)).center(Length::Fill))
            .padding(0)
            .width(18.0)
            .height(18.0)
            .style(style::button::icon(t))
            .on_press(Message::Preview(super::Msg::Close(
                ws.clone(),
                entry.id.clone(),
            )));
        let close = hangar_ui::tip::bottom(
            t,
            close,
            ctx.i18n
                .t_with(Key::ModelClose, &args!("title" => entry.title.as_str()))
                .into_owned(),
        );
        let tab = container(
            row![
                hangar_ui::tip::bottom(t, main, tooltip(ctx, ws, entry)),
                container(close).padding(Padding {
                    right: 5.0,
                    ..Padding::ZERO
                })
            ]
            .align_y(Alignment::Center),
        )
        .id(tab_id(&entry.id))
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: active.then_some(iced::Background::Color(t.colors.chrome)),
            ..Default::default()
        });
        tabs = tabs.push(tab);
        tabs = tabs.push(iced::widget::rule::vertical(1).style(style::rule::hairline(t)));
    }
    scrollable(tabs)
        .id(strip_id())
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new().width(0).scroller_width(0),
        ))
        .height(Length::Fill)
        .style(style::scrollable::default(t))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn the_arrows_cycle_through_the_tabs_and_wrap() {
        let tabs = ids(&["a", "b", "c"]);
        assert_eq!(target_of(TabKey::Next, &tabs, Some("a")), Some(1));
        assert_eq!(target_of(TabKey::Next, &tabs, Some("c")), Some(0));
        assert_eq!(target_of(TabKey::Previous, &tabs, Some("a")), Some(2));
        assert_eq!(target_of(TabKey::Previous, &tabs, Some("c")), Some(1));
    }

    #[test]
    fn home_and_end_jump_to_the_ends() {
        let tabs = ids(&["a", "b", "c"]);
        assert_eq!(target_of(TabKey::First, &tabs, Some("c")), Some(0));
        assert_eq!(target_of(TabKey::Last, &tabs, Some("a")), Some(2));
    }

    #[test]
    fn delete_goes_nowhere_and_an_empty_strip_has_no_target() {
        let tabs = ids(&["a"]);
        assert_eq!(target_of(TabKey::CloseActive, &tabs, Some("a")), None);
        assert_eq!(target_of(TabKey::Next, &[], None), None);
        // Nothing in front: the first is where the keys start from.
        assert_eq!(target_of(TabKey::Next, &ids(&["a", "b"]), None), Some(1));
    }

    #[test]
    fn a_tab_in_view_does_not_move_the_strip() {
        assert_eq!(reveal(100.0, 80.0, 400.0, 0.0), None);
        assert_eq!(reveal(100.0, 80.0, 400.0, 100.0), None, "flush at the left");
        assert_eq!(reveal(320.0, 80.0, 400.0, 0.0), None, "flush at the right");
    }

    #[test]
    fn a_tab_past_the_right_edge_is_brought_to_it_whole_and_no_further() {
        // Half of it showing: scroll just far enough to show all of it.
        assert_eq!(reveal(350.0, 100.0, 400.0, 0.0), Some(50.0));
        // Wholly out of sight.
        assert_eq!(reveal(900.0, 120.0, 400.0, 100.0), Some(620.0));
    }

    #[test]
    fn a_tab_before_the_left_edge_is_brought_to_it() {
        assert_eq!(reveal(40.0, 100.0, 400.0, 300.0), Some(40.0));
        assert_eq!(reveal(0.0, 100.0, 400.0, 300.0), Some(0.0));
    }

    #[test]
    fn a_tab_wider_than_the_strip_shows_its_beginning() {
        assert_eq!(reveal(500.0, 460.0, 400.0, 0.0), Some(500.0));
        assert_eq!(reveal(500.0, 460.0, 400.0, 900.0), Some(500.0));
    }

    #[test]
    fn tabs_have_ids_of_their_own() {
        assert_eq!(tab_id("a"), tab_id("a"));
        assert_ne!(tab_id("a"), tab_id("b"));
        assert_ne!(tab_id("a"), strip_id());
    }
}
