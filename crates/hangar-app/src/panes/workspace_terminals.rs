//! Every pane of one workspace (`WorkspaceTerminals.tsx`, inventory §2.1/12):
//! the grid, a pane per slot, the zoom.
//!
//! The shell draws this for each opened workspace; a workspace off screen
//! draws nothing, and its panes go on running and parsing their output all
//! the same, since they live in the package's state and not in a widget.

use std::collections::HashMap;

use hangar_i18n::{Key, args};
use iced::widget::Space;
use iced::{Element, Size};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::panes::grid::{Grid, Slot};
use crate::panes::header::PADDING_X;
use crate::panes::zoom_bar::{self, Other};
use crate::panes::{State, focused_pane, keyboard_free, terminal_pane, zoomed_pane};
use crate::state::Workspace;
use crate::state::layout as geometry;
use crate::state::types::SplitDir;

/// A grid width to fold headers by before the grid has measured itself.
const UNMEASURED: Size = Size::new(1000.0, 700.0);

pub fn view<'a>(st: &'a State, ctx: Ctx<'a>, ws: &'a Workspace) -> Element<'a, Message> {
    let t = ctx.theme;
    let c = &t.colors;
    let tree = geometry::normalize_tree(&ws.panes, ws.tree.as_ref());
    let layout = geometry::compute_layout(&tree);
    let order: Vec<&str> = layout.order.iter().map(String::as_str).collect();
    let focused = focused_pane(ctx.model, ws);
    let zoomed = zoomed_pane(ctx.model, ws);
    let size = st.grid_sizes.get(&ws.id).copied();
    let measured = size.unwrap_or(UNMEASURED);

    let mut slots = Vec::with_capacity(ws.panes.len());
    for pane in &ws.panes {
        let (Some(&rect), Some(rt)) = (layout.rects.get(&pane.id), st.panes.get(&pane.id)) else {
            continue;
        };
        let is_focused = focused.as_deref() == Some(pane.id.as_str());
        let is_zoomed = zoomed.as_deref() == Some(pane.id.as_str());
        let width = if is_zoomed {
            measured.width
        } else {
            rect.width as f32 * measured.width
        };
        let element = terminal_pane::view(
            st,
            ctx,
            ws,
            pane,
            rt,
            terminal_pane::Placing {
                focused: is_focused,
                zoomed: is_zoomed,
                can_close: ws.panes.len() > 1,
                can_zoom: ws.panes.len() > 1,
                width: width.max(PADDING_X),
                index: order
                    .iter()
                    .position(|id| *id == pane.id)
                    .unwrap_or_default(),
            },
        );
        // The outline says what the pane wants, an answer or (finished, or a
        // shell whose command is done) your turn; else the focus ring. Never
        // the accent for the first two: it would pass for the focused pane.
        let logic = &ctx.model.live.panes_logic;
        let ring = if logic.needs_attention(&pane.id) {
            Some(if logic.is_waiting(&pane.id) {
                c.state_waiting_fill
            } else {
                c.state_yours_fill
            })
        } else if is_focused {
            Some(c.accent.scale_alpha(0.66))
        } else {
            None
        };
        slots.push(Slot {
            id: &pane.id,
            name: &pane.name,
            rect,
            element,
            ring,
        });
    }
    if slots.is_empty() {
        return Space::new().into();
    }

    let zoom_bar = zoomed.as_ref().and_then(|zoomed| {
        let name = ws.panes.iter().find(|p| &p.id == zoomed)?.name.as_str();
        let others = order
            .iter()
            .filter(|id| *id != zoomed)
            .filter_map(|id| {
                let pane = ws.panes.iter().find(|p| p.id == *id)?;
                Some(Other {
                    id: &pane.id,
                    name: &pane.name,
                    activity: ctx.model.live.panes_logic.activity_or_idle(&pane.id),
                })
            })
            .collect();
        Some(zoom_bar::view(ctx, &ws.id, name, others, measured.width))
    });

    let tr = ctx.i18n;
    let names: HashMap<&str, &str> = ws
        .panes
        .iter()
        .map(|pane| (pane.id.as_str(), pane.name.as_str()))
        .collect();
    let mut grid = Grid::new(
        &ws.id,
        t,
        tree,
        slots,
        zoomed.as_ref(),
        zoom_bar,
        ws.panes
            .iter()
            .find(|p| Some(&p.id) == focused.as_ref())
            .map(|p| &p.id),
        size,
        move |count| tr.tn(Key::DropPaste, count, &[]).into_owned(),
    );
    // The boundaries take keys only where the keyboard is the grid's: no
    // dialog open over it, no field holding it.
    if keyboard_free(ctx.model) {
        grid = grid.boundary_keys(move |handle| boundary_name(tr, &names, handle));
    }
    grid.into()
}

/// What names a boundary on screen, once the keyboard has it, after the
/// panes on its two sides ("Resize Ava ↔ Leo": the separator's `aria-label`
/// of the old grid).
fn boundary_name(
    tr: hangar_i18n::Translator,
    names: &HashMap<&str, &str>,
    handle: &geometry::Handle,
) -> String {
    let side = |ids: &[String]| {
        ids.iter()
            .map(|id| names.get(id.as_str()).copied().unwrap_or_default())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let key = match handle.dir {
        SplitDir::Row => Key::GridResizeColumns,
        SplitDir::Col => Key::GridResizeRows,
    };
    tr.t_with(
        key,
        &args!("before" => side(&handle.before), "after" => side(&handle.after)),
    )
    .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_i18n::{Locale, Translator};

    fn handle(dir: SplitDir, before: &[&str], after: &[&str]) -> geometry::Handle {
        geometry::Handle {
            key: "k".into(),
            path: Vec::new(),
            dir,
            rect: geometry::Rect::FULL,
            bounds: geometry::Rect::FULL,
            before: before.iter().map(|id| (*id).to_string()).collect(),
            after: after.iter().map(|id| (*id).to_string()).collect(),
        }
    }

    #[test]
    fn a_boundary_is_named_after_the_panes_on_its_two_sides_in_both_languages() {
        let names: HashMap<&str, &str> = [("a", "Ava"), ("b", "Leo"), ("c", "Mia")]
            .into_iter()
            .collect();
        let side_by_side = handle(SplitDir::Row, &["a"], &["b", "c"]);
        assert_eq!(
            boundary_name(Translator::new(Locale::En), &names, &side_by_side),
            "Resize Ava ↔ Leo, Mia"
        );
        let stacked = handle(SplitDir::Col, &["a", "b"], &["c"]);
        assert_eq!(
            boundary_name(Translator::new(Locale::Fr), &names, &stacked),
            "Redimensionner Ava, Leo ↕ Mia"
        );
    }
}
