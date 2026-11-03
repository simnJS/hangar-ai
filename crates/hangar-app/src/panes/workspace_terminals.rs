//! Every pane of one workspace (`WorkspaceTerminals.tsx`, inventory §2.1/12):
//! the grid, a pane per slot, the zoom. A slot holds a terminal, or a view
//! of what a preview shows (`view_pane.rs`): both are leaves of the same
//! tree, moved, resized, zoomed and focused alike.
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
use crate::panes::{State, focused_pane, keyboard_free, terminal_pane, view_pane, zoomed_pane};
use crate::state::layout as geometry;
use crate::state::types::SplitDir;
use crate::state::{AgentActivity, Workspace};

/// A grid width to fold headers by before the grid has measured itself.
const UNMEASURED: Size = Size::new(1000.0, 700.0);

pub fn view<'a>(st: &'a State, ctx: Ctx<'a>, ws: &'a Workspace) -> Element<'a, Message> {
    let t = ctx.theme;
    let c = &t.colors;
    let tree = geometry::grid_tree(ws);
    let layout = geometry::compute_layout(&tree);
    let order: Vec<&str> = layout.order.iter().map(String::as_str).collect();
    let focused = focused_pane(ctx.model, ws);
    let zoomed = zoomed_pane(ctx.model, ws);
    let size = st.grid_sizes.get(&ws.id).copied();
    let measured = size.unwrap_or(UNMEASURED);

    let many = ws.pane_count() > 1;
    let place = |id: &str| {
        let is_focused = focused.as_deref() == Some(id);
        let is_zoomed = zoomed.as_deref() == Some(id);
        let rect = layout
            .rects
            .get(id)
            .copied()
            .unwrap_or(geometry::Rect::FULL);
        let width = if is_zoomed {
            measured.width
        } else {
            rect.width as f32 * measured.width
        };
        terminal_pane::Placing {
            focused: is_focused,
            zoomed: is_zoomed,
            can_close: ws.panes.len() > 1,
            can_zoom: many,
            width: width.max(PADDING_X),
            index: order.iter().position(|at| *at == id).unwrap_or_default(),
        }
    };
    let mut slots = Vec::with_capacity(ws.pane_count());
    for pane in &ws.panes {
        let (Some(&rect), Some(rt)) = (layout.rects.get(&pane.id), st.panes.get(&pane.id)) else {
            continue;
        };
        let placing = place(&pane.id);
        let is_focused = placing.focused;
        let element = terminal_pane::view(st, ctx, ws, pane, rt, placing);
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
            terminal: true,
        });
    }
    // The views after the terminals, in the order they were opened: the
    // slots keep a stable order whatever the arrangement.
    for view in &ws.view_panes {
        let Some(&rect) = layout.rects.get(&view.id) else {
            continue;
        };
        let placing = place(&view.id);
        let ring = placing.focused.then(|| c.accent.scale_alpha(0.66));
        let hovered = st.hovered.as_deref() == Some(view.id.as_str());
        slots.push(Slot {
            id: &view.id,
            name: view_pane::name_of(ctx, &ws.id, &view.id),
            rect,
            element: view_pane::view(ctx, ws, &view.id, hovered, placing),
            ring,
            terminal: false,
        });
    }
    if slots.is_empty() {
        return Space::new().into();
    }

    let names: HashMap<&str, &str> = ws
        .panes
        .iter()
        .map(|pane| (pane.id.as_str(), pane.name.as_str()))
        .chain(
            ws.view_panes
                .iter()
                .map(|view| (view.id.as_str(), view_pane::name_of(ctx, &ws.id, &view.id))),
        )
        .collect();

    let zoom_bar = zoomed.as_ref().and_then(|zoomed| {
        let name = *names.get(zoomed.as_str())?;
        let others = order
            .iter()
            .filter(|id| *id != zoomed)
            .filter_map(|id| {
                let (&id, &name) = names.get_key_value(*id)?;
                // A view has no agent: it rests.
                let activity = if ws.has_view(id) {
                    AgentActivity::Idle
                } else {
                    ctx.model.live.panes_logic.activity_or_idle(id)
                };
                Some(Other { id, name, activity })
            })
            .collect();
        Some(zoom_bar::view(ctx, &ws.id, name, others, measured.width))
    });

    let tr = ctx.i18n;
    let mut grid = Grid::new(
        &ws.id,
        t,
        tree,
        slots,
        zoomed.as_ref(),
        zoom_bar,
        ws.panes
            .iter()
            .map(|p| &p.id)
            .chain(ws.view_panes.iter().map(|v| &v.id))
            .find(|id| Some(*id) == focused.as_ref()),
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
