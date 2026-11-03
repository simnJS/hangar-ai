//! What the agents of a workspace are doing, in as little room as it takes
//! (`NavigatorStatus.tsx`): how many wait for an answer, are done and wait for
//! you, or are at work, each with the glyph that state wears everywhere else.
//! Nothing when they are idle.
//!
//! Pointing at one says who; clicking it brings that agent's pane forward, and
//! the next click the next one, as the toolbar's counters do. The keyboard
//! reaches the same agents through the row's menu.

use iced::widget::{button, row, text, tooltip};
use iced::{Alignment, Border, Element, Length, Padding};

use hangar_i18n::{Key, Translator, args};
use hangar_theme::tokens::{font_size, radius, space};
use hangar_theme::{fonts, style};
use hangar_ui::glyph;

use super::reads::{self, BUSY};
use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::state::types::{Pane, Workspace};
use crate::state::{AgentActivity, PaneId};

/// "Ava", "Ava and Ben", "Ava, Ben and Cleo" (`listNames`).
pub fn list_names(tr: Translator, names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [only] => (*only).to_string(),
        [init @ .., last] => tr
            .t_with(
                Key::SidebarNamesLast,
                &args!("list" => init.join(", "), "last" => *last),
            )
            .into_owned(),
    }
}

/// "Eve waits for your answer", "Eve and Max are working" (`whoIs`).
pub fn who_is(tr: Translator, state: AgentActivity, panes: &[&Pane]) -> String {
    let key = match state {
        AgentActivity::Waiting => Key::SidebarWhoWaiting,
        AgentActivity::Yours => Key::SidebarWhoYours,
        _ => Key::SidebarWhoWorking,
    };
    let names: Vec<&str> = panes.iter().map(|pane| pane.name.as_str()).collect();
    tr.tn(key, panes.len(), &args!("names" => list_names(tr, &names)))
        .into_owned()
}

/// The pane a click on a counter goes to: the one after the pane in front, in
/// the order they sit on screen, wrapping (`go`). The first click lands on the
/// first.
pub fn go_target<'a, T>(
    panes: &'a [T],
    id: impl Fn(&T) -> &PaneId,
    current: Option<&PaneId>,
) -> Option<&'a T> {
    if panes.is_empty() {
        return None;
    }
    let at = current.and_then(|current| panes.iter().position(|pane| id(pane) == current));
    panes.get(at.map_or(0, |at| at + 1) % panes.len())
}

/// The agents of these workspaces in each state worth counting, in the order
/// they sit on screen, with the workspace each is in.
pub fn roster<'a>(
    ctx: Ctx<'a>,
    workspaces: &[&'a Workspace],
) -> Vec<(AgentActivity, Vec<(&'a Workspace, &'a Pane)>)> {
    BUSY.iter()
        .map(|state| (*state, reads::agents_in(ctx.model, workspaces, *state)))
        .filter(|(_, panes)| !panes.is_empty())
        .collect()
}

/// The summary at the end of a row, or of a folded folder's heading: `None`
/// while nothing is going on.
pub fn view<'a>(ctx: Ctx<'a>, workspaces: &[&'a Workspace]) -> Option<Element<'a, Message>> {
    let t = ctx.theme;
    let tr = ctx.i18n;
    let shown = roster(ctx, workspaces);
    if shown.is_empty() {
        return None;
    }
    // The pane in front of the active workspace, so a second click moves on.
    let current = ctx
        .model
        .active_workspace()
        .and_then(|ws| ctx.model.ui.focus_by_workspace.get(&ws.id));

    let items = shown.iter().map(|(state, panes)| {
        let just_panes: Vec<&Pane> = panes.iter().map(|(_, pane)| *pane).collect();
        let words = who_is(tr, *state, &just_panes);
        let target = go_target(panes, |(_, pane)| &pane.id, current);
        let ink = match state {
            AgentActivity::Waiting => t.colors.state_waiting,
            AgentActivity::Yours => t.colors.state_yours,
            _ => t.colors.fg_muted,
        };
        let face = row![
            glyph::state(t, reads::activity_glyph(*state), 7.0),
            text(panes.len().to_string())
                .size(font_size::LABEL)
                .font(fonts::mono())
                .color(ink),
        ]
        .spacing(5)
        .align_y(Alignment::Center);
        let pressed = button(face)
            .padding(Padding::from([0.0, 5.0]))
            .height(18)
            .on_press_maybe(target.map(|(ws, pane)| {
                // Not the row's own click, which would only open the
                // workspace: this brings the agent forward and gives it the
                // keyboard.
                Message::Intent(Intent::FocusPane {
                    ws: ws.id.clone(),
                    pane: pane.id.clone(),
                    terminal: true,
                })
            }))
            .style(move |_: &iced::Theme, status| {
                let lit = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    // An outline rather than a fill: the count keeps its
                    // contrast on any row.
                    border: Border {
                        color: if lit {
                            t.colors.line_strong
                        } else {
                            iced::Color::TRANSPARENT
                        },
                        width: 1.0,
                        radius: radius::XS.into(),
                    },
                    text_color: ink,
                    ..button::Style::default()
                }
            });
        Element::from(tooltip(
            pressed,
            iced::widget::container(text(words).size(font_size::CAPTION_LG))
                .padding([space::XS - 1.0, space::MD - 1.0])
                .style(style::container::tooltip(t)),
            tooltip::Position::Bottom,
        ))
    });
    Some(
        row(items)
            .spacing(1)
            .width(Length::Shrink)
            .align_y(Alignment::Center)
            .into(),
    )
}

/// The agents of these workspaces that are doing something, for a menu: the
/// way to go to them from the keyboard.
pub fn busy_agents<'a>(
    ctx: Ctx<'a>,
    workspaces: &[&'a Workspace],
) -> Vec<(AgentActivity, &'a Workspace, &'a Pane)> {
    let mut found = Vec::new();
    for state in BUSY {
        for (ws, pane) in reads::agents_in(ctx.model, workspaces, state) {
            found.push((state, ws, pane));
        }
    }
    found
}

#[cfg(test)]
mod tests;
