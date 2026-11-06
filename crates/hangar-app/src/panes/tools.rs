//! The panes of a workspace that are not terminals (what the previews show: a
//! file, a 3D model, a kit's tab), drawn on their own for the tools area of
//! the chat view (`chat_view`).
//!
//! Each is the grid's own view pane (`view_pane.rs`), header and body, so
//! what it shows and the state it keeps are the preview's as in the grid.
//! They are stacked in the grid's reading order rather than laid out by the
//! workspace's tree: the tree places them among terminals that are not on
//! screen here, and its splits stay the grid's, untouched, for when the
//! workspace goes back to it.

use iced::widget::{column, mouse_area};
use iced::{Element, Length};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::panes::terminal_pane::Placing;
use crate::panes::{Msg, State, focused_pane, view_pane};
use crate::state::Workspace;
use crate::state::layout as geometry;

/// Nothing when the workspace has no view pane: the chat view then leaves
/// the room to the conversation. `width` is the room the area has, for the
/// headers' breakpoints.
pub fn view<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ws: &'a Workspace,
    width: f32,
) -> Option<Element<'a, Message>> {
    if ws.view_panes.is_empty() {
        return None;
    }
    let order = geometry::grid_order(ws);
    let focused = focused_pane(ctx.model, ws);
    let mut views: Vec<&str> = ws.view_panes.iter().map(|view| view.id.as_str()).collect();
    // The grid's reading order; a view the tree does not hold yet goes last.
    views.sort_by_key(|id| order.iter().position(|at| at == id).unwrap_or(usize::MAX));
    let mut list = column![]
        .spacing(1)
        .width(Length::Fill)
        .height(Length::Fill);
    for id in views {
        let placing = Placing {
            focused: focused.as_deref() == Some(id),
            zoomed: false,
            // A view pane may always close; the zoom is the grid's.
            can_close: true,
            can_zoom: false,
            width,
            index: order.iter().position(|at| at == id).unwrap_or_default(),
        };
        let hovered = st.hovered.as_deref() == Some(id);
        // A press in the pane gives it the focus, as it does in the grid;
        // one a control inside took (a button, the 3D view's drag) does not.
        let pane = mouse_area(view_pane::view(ctx, ws, id, hovered, placing))
            .on_press(Message::Panes(Msg::Focus(ws.id.clone(), id.to_owned())));
        list = list.push(pane);
    }
    Some(list.into())
}
