//! What the header of a view pane says of the entry it shows: the mark of
//! its kind (a spinner while it loads, an alert on an error, a cube for a
//! model, a window or the kit page's badge for a kit's tab, a mark for a
//! file), what it says beside its title (who opened it, or the kit page's own
//! word for its state), and its tooltip (its path, or its kit and params).
//!
//! The panes package draws the header itself, as it draws a terminal's, so
//! that every pane of the grid is moved, zoomed and closed the same way; it
//! asks this module for what only the preview knows (`panes/view_pane.rs`).

use std::f32::consts::TAU;

use iced::widget::{Space, container};
use iced::{Element, Radians};

use hangar_i18n::{Key, args};
use hangar_theme::tokens;
use hangar_ui::{Icon, icon};

use super::glyph::{Glyph, glyph};
use super::store::{Entry, KitBadge, KitBadgeState, Status};
use crate::ctx::Ctx;

/// The name of the pane that opened an entry, as the person reads it:
/// `opened_by` is a pane's id (or its name).
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
    // A narrow header may have cut it.
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

/// The entry's tooltip: a kit tab's kit and params, a file's path, and who
/// opened it.
pub fn hint(ctx: Ctx<'_>, ws: &str, entry: &Entry) -> String {
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

/// What the header says after the title: a kit page's word for its state,
/// else who opened it when an agent did.
pub fn aside(ctx: Ctx<'_>, ws: &str, entry: &Entry) -> Option<String> {
    match entry.kit() {
        Some(kit) => kit.badge.as_ref().and_then(|badge| badge.text.clone()),
        None => entry.opened_by.as_ref().map(|by| {
            ctx.i18n
                .t_with(
                    Key::ModelOpenedBy,
                    &args!("name" => opened_by_name(ctx, ws, by)),
                )
                .into_owned()
        }),
    }
}

/// A ring the size of the icon it stands in for, turning.
fn spinner<'a, M: 'a>(t: &'a hangar_theme::Theme, spin: usize) -> Element<'a, M> {
    let angle = spin as f32 * TAU / 8.0;
    icon(Icon::Restart, 12.0)
        .color(t.colors.fg_muted)
        .rotate(Radians(angle))
        .into()
}

/// The state a kit's page gave its tab: a small glyph where its icon goes.
/// Colour never says it alone: each state has its own shape.
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

/// The icon of an entry: a spinner while it loads, an alert on an error, a
/// cube for a model, a window for a kit (or its badge), a mark for a file.
/// `strong` when its pane has the focus.
pub fn mark<'a, M: 'a>(ctx: Ctx<'a>, entry: &'a Entry, strong: bool) -> Element<'a, M> {
    let t = ctx.theme;
    let spin = ctx.pkg.preview.spin;
    let quiet = if strong {
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
