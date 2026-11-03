//! Every pane of one workspace (`WorkspaceTerminals.tsx`, inventory §2.1/12):
//! the grid, a pane per slot, the zoom.
//!
//! The shell draws this for each opened workspace; a workspace off screen
//! draws nothing, and its panes go on running and parsing their output all
//! the same, since they live in the package's state and not in a widget.

use hangar_i18n::Key;
use iced::widget::Space;
use iced::{Element, Size};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::panes::grid::{Grid, Slot};
use crate::panes::header::PADDING_X;
use crate::panes::zoom_bar::{self, Other};
use crate::panes::{State, focused_pane, geometry, terminal_pane, zoomed_pane};
use crate::state::Workspace;

/// A grid width to fold headers by before the grid has measured itself.
const UNMEASURED: Size = Size::new(1000.0, 700.0);

pub fn view<'a>(st: &'a State, ctx: Ctx<'a>, ws: &'a Workspace) -> Element<'a, Message> {
    let t = ctx.theme;
    let c = &t.colors;
    let tree = geometry::normalize_tree(&ws.panes, ws.tree.as_ref());
    let layout = geometry::compute_layout(&tree);
    let order: Vec<&str> = layout.order().map(String::as_str).collect();
    let focused = focused_pane(ctx.model, ws);
    let zoomed = zoomed_pane(ctx.model, ws);
    let size = st.grid_sizes.get(&ws.id).copied();
    let measured = size.unwrap_or(UNMEASURED);

    let mut slots = Vec::with_capacity(ws.panes.len());
    for pane in &ws.panes {
        let (Some(rect), Some(rt)) = (layout.rect(&pane.id), st.panes.get(&pane.id)) else {
            continue;
        };
        let is_focused = focused.as_deref() == Some(pane.id.as_str());
        let is_zoomed = zoomed.as_deref() == Some(pane.id.as_str());
        let width = if is_zoomed {
            measured.width
        } else {
            rect.width * measured.width
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
    Grid::new(
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
    )
    .into()
}
