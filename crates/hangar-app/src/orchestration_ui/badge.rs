//! What a pane is in its workspace's orchestration, in its header, right after
//! its name (`OrchestrationBadge.tsx`): « Orchestrateur », or « Worker » and
//! the title of the task it is on, or « Chef de projet » on the HQ's manager.
//! Nothing at all on every other pane.
//!
//! A worker's task is read from the workspace's board, the one its worktree
//! shares, so a title edited on the board shows here too. The title is read
//! from the boards cache (`board::Live`) at every draw: the cache holds the
//! board of every workspace's folder and reads it again when it changes, so
//! a title edited from the worktree, or by an agent, follows at once.

use iced::widget::{container, row, text};
use iced::{Alignment, Element, Length, Padding};

use hangar_i18n::{Key, args};
use hangar_theme::{Theme, fonts, style, tokens};
use hangar_ui::{Icon, icon, tip};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::Model;
use crate::state::types::{Pane, PaneRole, Workspace};
use crate::state::{PaneId, WsId};

/// The longest a task's title gets before it is cut, and the shorter limit of
/// a narrow header (`.orch-badge__task` and its container queries).
const TASK_WIDTH: f32 = 220.0;
const TASK_WIDTH_NARROW: f32 = 120.0;

/// The header widths at which the badge gives way, in pixels: the stylesheet's
/// `@container pane-bar` limits in ems of the bar's 12 px text.
const NARROW: f32 = 504.0;
const NO_TASK: f32 = 408.0;
const NO_ROLE_WORD: f32 = 480.0;
const NONE: f32 = 186.0;

/// The badge of a pane in a header `bar_width` wide: a narrower header drops
/// the task's title, then the orchestrator's word, then the badge, before the
/// pane's name has to give way to it.
pub fn pane_badge_in<'a>(
    ctx: Ctx<'a>,
    ws: &'a WsId,
    pane: &'a PaneId,
    bar_width: f32,
) -> Element<'a, Message> {
    let found = ctx
        .model
        .workspace(ws)
        .and_then(|workspace| Some((workspace, workspace.panes.iter().find(|p| &p.id == pane)?)));
    let Some((workspace, pane)) = found else {
        return nothing();
    };
    if bar_width < NONE {
        return nothing();
    }
    match pane.role {
        Some(PaneRole::Orchestrator) => lead(
            ctx,
            Key::OrchBadgeOrchestrator,
            Key::OrchBadgeOrchestratorHint,
            bar_width,
        ),
        Some(PaneRole::Manager) => lead(
            ctx,
            Key::OrchBadgeManager,
            Key::OrchBadgeManagerHint,
            bar_width,
        ),
        Some(PaneRole::Worker) => worker(ctx, workspace, pane, bar_width),
        None => nothing(),
    }
}

/// Nothing, and no room for it.
fn nothing<'a>() -> Element<'a, Message> {
    iced::widget::space().width(0).height(0).into()
}

/// The capsule every badge sits in.
fn capsule<'a>(
    t: &'a Theme,
    content: Element<'a, Message>,
    padding_x: f32,
) -> Element<'a, Message> {
    container(content)
        .padding(Padding::from([0.0, padding_x]))
        .height(18.0)
        .align_y(Alignment::Center)
        .style(style::container::chip(t))
        .into()
}

/// The orchestrator's and the manager's: the accent, with a mark and a word.
fn lead<'a>(ctx: Ctx<'a>, role: Key, hint: Key, bar_width: f32) -> Element<'a, Message> {
    let t = ctx.theme;
    let ink = t.colors.accent_hi;
    let mark = icon(Icon::Orchestrator, 11.0).color(ink);
    let content: Element<'a, Message> = if bar_width < NO_ROLE_WORD {
        row![mark].into()
    } else {
        row![
            mark,
            text(ctx.t(role))
                .size(tokens::font_size::CAPTION)
                .font(fonts::ui_medium())
                .color(ink)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into()
    };
    let padding = if bar_width < NO_ROLE_WORD { 4.0 } else { 6.0 };
    tip::bottom(t, capsule(t, content, padding), ctx.t(hint))
}

/// The title of a task of the workspace's board, as the boards cache holds it
/// now (`board::Live`).
fn task_title<'a>(m: &'a Model, workspace: &Workspace, task: &str) -> Option<&'a str> {
    m.live
        .board
        .tasks(&workspace.cwd)?
        .iter()
        .find(|candidate| candidate.id == task)
        .map(|candidate| candidate.title.as_str())
}

/// A worker's: the quiet grey, and the title of the task it is on.
fn worker<'a>(
    ctx: Ctx<'a>,
    workspace: &'a Workspace,
    pane: &'a Pane,
    bar_width: f32,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let title = pane
        .task_id
        .as_deref()
        .and_then(|task| task_title(ctx.model, workspace, task));
    let hint = match title {
        Some(title) => ctx
            .i18n
            .t_with(Key::OrchBadgeWorkerHint, &args!("title" => title)),
        None => ctx.t(Key::OrchBadgeWorkerNoTask),
    };

    let mut line = row![
        text(ctx.t(Key::OrchBadgeWorker))
            .size(tokens::font_size::CAPTION)
            .font(fonts::ui_medium())
            .color(t.colors.fg_secondary)
            .wrapping(text::Wrapping::None)
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    if let (Some(title), true) = (title, bar_width >= NO_TASK) {
        let limit = if bar_width < NARROW {
            TASK_WIDTH_NARROW
        } else {
            TASK_WIDTH
        };
        line = line
            .push(
                text("·")
                    .size(tokens::font_size::CAPTION)
                    .color(t.colors.fg_secondary),
            )
            .push(
                container(
                    text(title)
                        .size(tokens::font_size::CAPTION)
                        .font(fonts::ui_medium())
                        .color(t.colors.fg)
                        .wrapping(text::Wrapping::None),
                )
                .max_width(limit)
                .width(Length::Shrink)
                .clip(true),
            );
    }
    tip::bottom(t, capsule(t, line.into(), 6.0), hint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::model::test_task;

    fn workspace(cwd: &str) -> Workspace {
        Workspace {
            id: "wt".into(),
            cwd: cwd.into(),
            ..Workspace::default()
        }
    }

    fn titled(id: &str, title: &str) -> crate::board::model::Task {
        let mut task = test_task(id);
        task.title = title.into();
        task
    }

    #[test]
    fn a_workers_badge_follows_a_title_edited_from_its_worktree() {
        // The worker's workspace is a worktree of the project, on a folder of
        // its own; the board it shares is read by the boards cache.
        let ws = workspace("/work/project-wt");
        let mut m = Model::default();
        m.live
            .board
            .hold("/work/project-wt", vec![titled("t1", "Draft the page")]);
        assert_eq!(task_title(&m, &ws, "t1"), Some("Draft the page"));

        // The title is edited (the dialog, a worktree, an agent): the cache
        // reads the board again, and the next draw says so, with no state of
        // this package to refresh.
        m.live.board.hold(
            "/work/project-wt",
            vec![titled("t1", "Draft the page, twice")],
        );
        assert_eq!(task_title(&m, &ws, "t1"), Some("Draft the page, twice"));
    }

    #[test]
    fn a_task_the_board_does_not_have_has_no_title() {
        let ws = workspace("/work/project-wt");
        let mut m = Model::default();
        // Not read yet.
        assert_eq!(task_title(&m, &ws, "t1"), None);
        m.live
            .board
            .hold("/work/project-wt", vec![titled("t2", "Another")]);
        assert_eq!(task_title(&m, &ws, "t1"), None);
        // Another folder's board is not this workspace's.
        assert_eq!(task_title(&m, &workspace("/elsewhere"), "t2"), None);
    }
}
