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
use crate::panes::{
    State, focus_on_panes, focused_pane, keyboard_free, terminal_pane, view_pane, zoomed_pane,
};
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
    )
    .keyboard_elsewhere(!focus_on_panes(ctx.model));
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

    /// A workspace of one terminal, Ava, and a kit's tab beside it as a view
    /// pane, its entry in the process's preview store (which the views read
    /// through the model) under a workspace id no other test uses.
    fn terminal_and_kit_tab() -> (crate::state::Model, crate::app::Packages, String) {
        use crate::preview::store::KitTabToOpen;
        use crate::state::layout::leaf;
        use crate::state::types::{AgentId, AppState, Pane, SplitNode};
        let ws = "grid-kit-pane".to_string();
        let mut m = crate::navigator::testing::model_of(AppState {
            workspaces: vec![crate::state::Workspace {
                id: ws.clone(),
                name: "Shop".into(),
                panes: vec![Pane {
                    id: "ava".into(),
                    name: "Ava".into(),
                    agent: AgentId::Shell,
                    ..Pane::default()
                }],
                ..crate::state::Workspace::default()
            }],
            active_workspace_id: Some(ws.clone()),
            ..AppState::default()
        });
        let shared = m.live.preview.shared().clone();
        let id = shared.edit(|store| {
            store.close_workspace_models(&ws);
            store.open_kit_tab(KitTabToOpen::new(ws.clone(), "ci-kit", "runs", "CI runs"))
        });
        m.live.preview.sync();
        crate::preview::panes::sync(&mut m);
        let workspace = m.workspace(&ws).unwrap();
        assert!(workspace.has_view(&id));
        assert_eq!(
            workspace.tree,
            Some(SplitNode::Split {
                dir: SplitDir::Row,
                ratio: 0.5,
                a: Box::new(leaf("ava")),
                b: Box::new(leaf(id.clone())),
            }),
            "beside the terminal, along its longer side"
        );
        let mut pkg = crate::app::Packages::default();
        let rt = crate::panes::controller::mounted(workspace, &workspace.panes[0]);
        pkg.panes.panes.insert("ava".into(), rt);
        (m, pkg, id)
    }

    #[test]
    fn a_grid_draws_a_terminal_and_a_kit_tab_and_a_press_in_the_tab_focuses_its_pane() {
        use crate::navigator::testing::{ctx, simulate};
        let (m, pkg, id) = terminal_and_kit_tab();
        let ws = m.workspace("grid-kit-pane").unwrap();
        let ctx = ctx(&m, &pkg);
        let mut ui = simulate(view(&pkg.panes, ctx, ws));
        let _ = ui
            .snapshot(&hangar_theme::to_iced(ctx.theme))
            .expect("it draws");
        // Both headers: the terminal's name, the tab's title.
        assert!(ui.find("Ava").is_ok());
        assert!(ui.find("CI runs").is_ok());
        // A press in the tab's pane gives it the focus, as in a terminal.
        ui.click("CI runs").expect("the tab's header is there");
        let focused: Vec<String> = ui
            .into_messages()
            .filter_map(|message| match message {
                Message::Panes(crate::panes::Msg::Focus(_, pane)) => Some(pane),
                _ => None,
            })
            .collect();
        assert_eq!(focused, std::slice::from_ref(&id));
        let shared = m.live.preview.shared().clone();
        shared.edit(|store| store.close_workspace_models("grid-kit-pane"));
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
