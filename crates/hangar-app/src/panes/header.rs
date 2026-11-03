//! A pane's header (`.pane__bar` of TerminalPane.tsx): one line of status,
//! and the drag handle for the whole pane.
//!
//! What it says about the agent (its activity, whether the mod reports it,
//! the queue, the context gauge, the attention badge, an overlap, the git
//! branch) is read from P4's `panes_logic::Live`; what it says about the
//! shell and the launch is this package's.
//!
//! A narrowing pane drops its details in a fixed order, so nothing ever wraps
//! or overlaps, down to eight panes beside both side panels in the smallest
//! window. The web app did it with container queries in `em` of the header's
//! 12 px font; [`Fit::of`] is the same table over the header's content width
//! (the pane's width less its 16 px of padding).

use hangar_i18n::{Key, args};
use hangar_theme::{AgentState, style, tokens};
use hangar_ui::{Icon, button, glyph, icon, menu, tip};
use iced::widget::{Space, container, mouse_area, row, text};
use iced::{Alignment, Element, Length, Padding};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::pane_menu::Folded;
use crate::panes::controller::{PaneRt, folder_of};
use crate::panes::gauge;
use crate::panes::status::{pane_glyph_state, shell_short_name, state_key, ui_glyph};
use crate::panes::{Msg, Picker};
use crate::panes_logic::{PaneBranch, short_session_id};
use crate::state::live::PaneStatus;
use crate::state::types::{AGENTS, AgentId, Pane, ShellInfo, Workspace};
use crate::state::{AgentActivity, GlyphState};

/// The header's font size, which its breakpoints are counted in.
const EM: f32 = 12.0;
/// The header's padding, left and right: what its content box loses.
pub const PADDING_X: f32 = 16.0;
/// The branch label's widest (`max-width: 18ch` at the header's 10.8 px
/// mono).
const BRANCH_MAX: f32 = 18.0 * 6.5;

/// What a header of a given width shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fit {
    pub shell: bool,
    /// The waiting word in its short form ("Answer?").
    pub short_waiting: bool,
    /// Every state word in its short form.
    pub short_states: bool,
    pub session_id: bool,
    pub gauge_pct: bool,
    pub diff: bool,
    /// The state word, on the focused pane, where the actions take its place.
    pub state_focused: bool,
    /// The state word while the header is hovered.
    pub state_hovered: bool,
    pub commands: bool,
    pub session: bool,
    pub meta: bool,
    pub zoom: bool,
    /// The agent's first word ("Claude") rather than its full name.
    pub short_agent: bool,
    /// The small icons (the picker's chevron, the queue's tray).
    pub icons: bool,
    /// Actions 20 px wide instead of 22.
    pub narrow_actions: bool,
    pub agent: bool,
    pub queue: bool,
    pub state: bool,
}

impl Fit {
    /// The table of the `@container pane-bar` rules, for a content box
    /// `width` logical pixels wide.
    pub fn of(width: f32) -> Fit {
        let below = |em: f32| width <= em * EM;
        Fit {
            shell: !below(47.0),
            short_waiting: below(47.0),
            short_states: below(42.0),
            session_id: !below(42.0),
            gauge_pct: !below(37.0),
            diff: !below(34.0),
            state_focused: !below(29.0),
            state_hovered: !below(28.5),
            commands: !below(27.5),
            session: !below(27.5) && !below(21.0),
            meta: !below(25.0) && !below(15.5),
            zoom: !below(25.0),
            short_agent: below(21.0),
            icons: !below(15.5),
            narrow_actions: below(15.5),
            agent: !below(12.0),
            queue: !below(12.0),
            state: !below(12.0),
        }
    }

    /// The header buttons this width folded away, which the "…" menu takes
    /// in (`toggleMore`'s `folded`).
    pub fn folded(self) -> Folded {
        Folded {
            commands: !self.commands,
            diff: !self.diff,
            zoom: !self.zoom,
        }
    }
}

/// What the header is drawn with.
pub struct Head<'a> {
    pub ws: &'a Workspace,
    pub pane: &'a Pane,
    pub rt: &'a PaneRt,
    pub focused: bool,
    pub hovered: bool,
    pub zoomed: bool,
    pub can_close: bool,
    /// Zoom means something: there is more than one pane.
    pub can_zoom: bool,
    pub width: f32,
    pub shell: Option<&'a ShellInfo>,
    pub picker: Option<Picker>,
}

fn wrap(msg: Msg) -> Message {
    Message::Panes(msg)
}

/// The state word's colour, the state's own (`.pane__state--*`).
fn state_tone(glyph: GlyphState) -> AgentState {
    match glyph {
        GlyphState::Working => AgentState::Working,
        GlyphState::Waiting => AgentState::Waiting,
        GlyphState::Yours => AgentState::Yours,
        _ => AgentState::Idle,
    }
}

pub fn view<'a>(ctx: Ctx<'a>, head: Head<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    let c = &t.colors;
    let rt = head.rt;
    let pane = head.pane;
    let logic = &ctx.model.live.panes_logic;
    let content_width = head.width - PADDING_X;
    let fit = Fit::of(content_width);

    // The shell's life until it runs, then the agent's state, or nothing
    // more than a resting ring for a plain shell, whose builds and tests are
    // not an agent at work.
    let status = logic.pane_status(&pane.id).unwrap_or(rt.status);
    let published = logic.pane_activity(&pane.id);
    let bridged = published.is_some_and(|p| p.bridged);
    let activity = published.map(|p| p.activity);
    let reports = activity.is_some();
    let glyph_state = pane_glyph_state(status, activity);
    let state_label = ctx.t(state_key(status, activity)).into_owned();
    let source = (status == PaneStatus::Running && reports).then(|| {
        ctx.t(if bridged {
            Key::PaneStateReported
        } else {
            Key::PaneStateGuessed
        })
    });
    let state_title = match &source {
        Some(source) => format!("{state_label} · {source}"),
        None => state_label.clone(),
    };
    let short_state = match glyph_state {
        GlyphState::Waiting => ctx.t(Key::PaneStateWaitingShort).into_owned(),
        GlyphState::Yours => ctx.t(Key::PaneStateYoursShort).into_owned(),
        _ => state_label.clone(),
    };
    let insight = logic.insight(&pane.id);

    let mut bar = row![].spacing(tokens::space::SM).align_y(Alignment::Center);

    bar = bar.push(tip::bottom(
        t,
        container(glyph::state(t, ui_glyph(glyph_state), 8.0)).center_x(10),
        state_title.clone(),
    ));

    // The jump shortcut's hint is the keymap's (`pane.jump {keys}` for the
    // first nine panes); the name alone until P7 exposes the keys.
    let name_tip = ctx
        .i18n
        .t_with(Key::PaneLabel, &args!("name" => pane.name.as_str()));
    let focused = head.focused;
    bar = bar.push(tip::bottom(
        t,
        text(pane.name.as_str())
            .size(tokens::font_size::UI)
            .font(hangar_theme::fonts::ui_semibold())
            .wrapping(text::Wrapping::None)
            .style(move |theme| {
                if focused {
                    style::text::primary(t)(theme)
                } else {
                    style::text::muted(t)(theme)
                }
            }),
        name_tip.into_owned(),
    ));

    if let Some(branch) = logic.pane_branch(&folder_of(head.ws, pane)) {
        bar = bar.push(branch_label(ctx, branch));
    }

    bar = bar.push(crate::orchestration_ui::pane_badge_in(
        ctx,
        &head.ws.id,
        &pane.id,
        content_width,
    ));
    bar = bar.push(crate::accounts::pane_badge(ctx, &head.ws.id, &pane.id));

    if fit.agent {
        bar = bar.push(agent_picker(ctx, &head, fit));
    }

    if fit.meta {
        let mut meta = row![].spacing(tokens::space::SM).align_y(Alignment::Center);
        if fit.shell {
            meta = meta.push(shell_picker(ctx, &head));
        }
        if fit.session && pane.agent.info().resumable {
            meta = meta.push(session_button(ctx, &head, fit));
        }
        if let Some(reading) = &insight.context {
            meta = meta.push(gauge::view(ctx, reading, pane.agent, fit.gauge_pct));
        }
        bar = bar.push(meta);
    }

    bar = bar.push(Space::new().width(Length::Fill));

    let actions_shown = head.focused || head.hovered;
    let show_state = (status != PaneStatus::Running || reports)
        && fit.state
        && !(head.focused && !fit.state_focused)
        && !(head.hovered && !head.focused && !fit.state_hovered);
    if show_state {
        let shortened =
            fit.short_states || (fit.short_waiting && glyph_state == GlyphState::Waiting);
        bar = bar.push(tip::bottom(
            t,
            text(if shortened { short_state } else { state_label })
                .size(tokens::font_size::CAPTION_LG)
                .font(hangar_theme::fonts::ui_medium())
                .wrapping(text::Wrapping::None)
                .style(style::text::state(t, state_tone(glyph_state))),
            state_title,
        ));
    }

    // Prompts held for the agent; a click drops them.
    if fit.queue && !insight.queue.is_empty() {
        let n = insight.queue.len();
        let words = ctx.i18n.tn(Key::PaneQueue, n, &[]).into_owned();
        let mut content = row![].spacing(3).align_y(Alignment::Center);
        if fit.icons {
            content = content.push(icon(Icon::Tray, 11.0));
        }
        content = content.push(
            text(n.to_string())
                .size(tokens::font_size::LABEL)
                .font(hangar_theme::fonts::mono()),
        );
        bar = bar.push(tip::bottom(
            t,
            iced::widget::button(content)
                .padding(Padding::from([0.0, 6.0]))
                .height(18)
                .style(style::button::toggle(t, true))
                .on_press(wrap(Msg::CancelQueue(pane.id.clone()))),
            words,
        ));
    }

    // Another pane just edited a file this one had edited.
    if let Some(other) = logic.conflict(&pane.id) {
        let words = ctx
            .i18n
            .t_with(Key::PaneConflict, &args!("name" => other))
            .into_owned();
        bar = bar.push(tip::bottom(
            t,
            icon(Icon::Alert, 13.0).color(c.warning),
            words,
        ));
    }

    let missing = pane.agent != AgentId::Shell
        && !ctx
            .model
            .ui
            .available_agents
            .iter()
            .any(|agent| agent == pane.agent.as_str());
    if missing {
        let words = ctx
            .i18n
            .t_with(Key::PaneNotOnPath, &args!("agent" => pane.agent.as_str()))
            .into_owned();
        bar = bar.push(tip::bottom(
            t,
            icon(Icon::Alert, 13.0).color(c.warning),
            words,
        ));
    }
    // An agent pane says the same through its state; a shell keeps the bell
    // for the build that finished while the person was away.
    if logic.needs_attention(&pane.id) && !reports {
        bar = bar.push(tip::bottom(
            t,
            icon(Icon::Bell, 13.0).color(c.state_yours_fill),
            ctx.t(Key::PaneAttention).into_owned(),
        ));
    }
    // A shell that died says how to bring it back, in so many words.
    if status == PaneStatus::Exited {
        bar = bar.push(tip::bottom(
            t,
            iced::widget::button(
                row![
                    icon(Icon::Restart, 11.0),
                    text(ctx.t(Key::PaneRestartNow))
                        .size(tokens::font_size::CAPTION_LG)
                        .font(hangar_theme::fonts::ui_medium()),
                ]
                .spacing(tokens::space::XS)
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([0.0, 8.0]))
            .height(20)
            .style(style::button::secondary(t))
            .on_press(wrap(Msg::Restart(head.ws.id.clone(), pane.id.clone()))),
            ctx.t(Key::PaneRestart).into_owned(),
        ));
    }

    if actions_shown {
        bar = bar.push(actions(ctx, &head, fit));
    }

    let header = container(bar)
        .padding(Padding {
            top: 0.0,
            right: 6.0,
            bottom: 0.0,
            left: 10.0,
        })
        .height(tokens::size::PANE_HEAD_HEIGHT)
        .width(Length::Fill)
        .align_y(Alignment::Center)
        .clip(true)
        .style(style::container::pane_head(t, head.focused));

    let id = pane.id.clone();
    mouse_area(header)
        .on_enter(wrap(Msg::Hover(Some(id))))
        .on_exit(wrap(Msg::Hover(None)))
        .into()
}

/// The git branch of the pane's folder, beside its name; a detached head
/// shows its short sha, marked (`PaneBranchLabel`).
fn branch_label<'a>(ctx: Ctx<'a>, branch: &'a PaneBranch) -> Element<'a, Message> {
    let t = ctx.theme;
    let (label, words, detached) = match branch {
        PaneBranch::Branch {
            name,
            linked_worktree,
        } => (
            name.clone(),
            ctx.i18n.t_with(
                if *linked_worktree {
                    Key::PaneBranchBranchWorktree
                } else {
                    Key::PaneBranchBranch
                },
                &args!("branch" => name.as_str()),
            ),
            false,
        ),
        PaneBranch::Detached { head } => (
            head.clone(),
            ctx.i18n
                .t_with(Key::PaneBranchDetached, &args!("head" => head.as_str())),
            true,
        ),
    };
    let mut content = row![
        text(label)
            .size(tokens::font_size::LABEL)
            .font(hangar_theme::fonts::mono())
            .wrapping(text::Wrapping::None)
            .style(style::text::muted(t)),
    ]
    .spacing(3);
    if detached {
        content = content.push(
            text(ctx.t(Key::PaneBranchDetachedMark))
                .size(tokens::font_size::LABEL)
                .font(iced::Font {
                    style: iced::font::Style::Italic,
                    ..hangar_theme::fonts::ui()
                })
                .style(style::text::muted(t)),
        );
    }
    tip::bottom(
        t,
        container(content).max_width(BRANCH_MAX).clip(true),
        words.into_owned(),
    )
}

/// The ⚡, diff, zoom, "…" and close buttons: always on the focused pane, on
/// the others while the header is hovered.
fn actions<'a>(ctx: Ctx<'a>, head: &Head<'a>, fit: Fit) -> Element<'a, Message> {
    let t = ctx.theme;
    let pane = head.pane;
    let ws = &head.ws.id;
    let side = if fit.narrow_actions { 20.0 } else { 22.0 };
    let mut group = row![].spacing(1).align_y(Alignment::Center);
    if fit.commands {
        group = group.push(crate::saved_commands::menu_view(
            &ctx.pkg.saved_commands,
            ctx,
            &pane.id,
        ));
    }
    if fit.diff {
        group = group.push(
            button::icon_button(t, Icon::Diff)
                .side(side)
                .tip(ctx.t(Key::DiffOpen).into_owned())
                .on_press(wrap(Msg::Diff(ws.clone(), pane.id.clone()))),
        );
    }
    if fit.zoom && head.can_zoom {
        let words = ctx
            .t(if head.zoomed {
                Key::ZoomRestore
            } else {
                Key::PaneZoomIn
            })
            .into_owned();
        group = group.push(
            button::toggle_button(
                t,
                if head.zoomed {
                    Icon::Collapse
                } else {
                    Icon::Expand
                },
                head.zoomed,
            )
            .side(side)
            .tip(words)
            .on_press(wrap(Msg::ToggleZoom(ws.clone(), pane.id.clone()))),
        );
    }
    group = group.push(crate::pane_menu::view_folded(
        &ctx.pkg.pane_menu,
        ctx,
        &pane.id,
        fit.folded(),
    ));
    let close_words = ctx
        .t(if head.can_close {
            Key::PaneClose
        } else {
            Key::PaneCloseLast
        })
        .into_owned();
    group = group.push(
        button::icon_button(t, Icon::Close)
            .side(side)
            .danger()
            .tip(close_words)
            .on_press_maybe(
                head.can_close
                    .then(|| wrap(Msg::Close(ws.clone(), pane.id.clone()))),
            ),
    );
    container(group)
        .padding(Padding {
            left: 4.0,
            ..Padding::ZERO
        })
        .into()
}

/// A picker that reads as text: its label and a chevron, a menu under it.
fn picker_button<'a>(
    ctx: Ctx<'a>,
    label: String,
    chevron: bool,
    mono: bool,
    on_press: Msg,
) -> iced::widget::Button<'a, Message> {
    let t = ctx.theme;
    let mut content = row![
        text(label)
            .size(if mono {
                tokens::font_size::LABEL
            } else {
                tokens::font_size::UI
            })
            .font(if mono {
                hangar_theme::fonts::mono()
            } else {
                hangar_theme::fonts::ui()
            })
            .wrapping(text::Wrapping::None)
    ]
    .spacing(3)
    .align_y(Alignment::Center);
    if chevron {
        content = content.push(icon(Icon::Updown, 10.0));
    }
    iced::widget::button(content)
        .padding(Padding::from([0.0, 4.0]))
        .height(20)
        .style(style::button::ghost(t))
        .on_press(wrap(on_press))
}

fn agent_picker<'a>(ctx: Ctx<'a>, head: &Head<'a>, fit: Fit) -> Element<'a, Message> {
    let t = ctx.theme;
    let pane = head.pane;
    let label = pane.agent.info().label;
    let shown = if fit.short_agent {
        label.split(' ').next().unwrap_or(label)
    } else {
        label
    };
    let anchor = picker_button(
        ctx,
        shown.to_owned(),
        fit.icons,
        false,
        Msg::OpenPicker(pane.id.clone(), Picker::Agent),
    );
    let open = head.picker == Some(Picker::Agent);
    let entries = open.then(|| {
        AGENTS
            .iter()
            .map(|agent| {
                let missing = agent.id != AgentId::Shell
                    && !ctx
                        .model
                        .ui
                        .available_agents
                        .iter()
                        .any(|id| id == agent.id.as_str());
                let mut label = agent.label.to_owned();
                if missing {
                    label.push_str(&ctx.t(Key::PaneMissing));
                }
                menu::item(label)
                    .checked(agent.id == pane.agent)
                    .on_select(wrap(Msg::SetAgent(
                        head.ws.id.clone(),
                        pane.id.clone(),
                        agent.id,
                    )))
                    .into()
            })
            .collect()
    });
    tip::bottom(
        t,
        menu::dropdown(t, anchor, entries).on_dismiss(wrap(Msg::ClosePicker)),
        ctx.t(Key::PaneAgent).into_owned(),
    )
}

fn shell_picker<'a>(ctx: Ctx<'a>, head: &Head<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    let pane = head.pane;
    let name = shell_short_name(head.shell).unwrap_or_else(|| ctx.t(Key::PaneShell).into_owned());
    let anchor = picker_button(
        ctx,
        name,
        false,
        true,
        Msg::OpenPicker(pane.id.clone(), Picker::Shell),
    );
    let open = head.picker == Some(Picker::Shell);
    let entries = open.then(|| {
        let inherited_name = head
            .shell
            .map_or_else(|| ctx.t(Key::PaneShell).into_owned(), |s| s.label.clone());
        let mut entries = vec![
            menu::item(
                ctx.i18n
                    .t_with(
                        Key::PaneInherited,
                        &args!("name" => inherited_name.as_str()),
                    )
                    .into_owned(),
            )
            .checked(pane.shell_id.is_none())
            .on_select(wrap(Msg::SetShell(
                head.ws.id.clone(),
                pane.id.clone(),
                None,
            )))
            .into(),
        ];
        entries.extend(ctx.model.ui.shells.iter().map(|shell| {
            menu::item(shell.label.clone())
                .checked(pane.shell_id.as_deref() == Some(shell.id.as_str()))
                .on_select(wrap(Msg::SetShell(
                    head.ws.id.clone(),
                    pane.id.clone(),
                    Some(shell.id.clone()),
                )))
                .into()
        }));
        entries
    });
    let words = match head.shell {
        Some(shell) => ctx
            .i18n
            .t_with(
                Key::PaneShellTitle,
                &args!("program" => shell.program.as_str()),
            )
            .into_owned(),
        None => ctx.t(Key::PaneShell).into_owned(),
    };
    tip::bottom(
        t,
        menu::dropdown(t, anchor, entries).on_dismiss(wrap(Msg::ClosePicker)),
        words,
    )
}

/// The session a resumable agent comes back on, or "new"; a click opens the
/// sessions picker.
fn session_button<'a>(ctx: Ctx<'a>, head: &Head<'a>, fit: Fit) -> Element<'a, Message> {
    let t = ctx.theme;
    let pane = head.pane;
    let (words, label) = match pane.session_id.as_deref() {
        Some(id) => (
            ctx.i18n
                .t_with(Key::PaneResuming, &args!("id" => id))
                .into_owned(),
            format!("{}…", short_session_id(id)),
        ),
        None => (
            ctx.t(Key::PaneNoSession).into_owned(),
            ctx.t(Key::PaneNewSession).into_owned(),
        ),
    };
    let mut content = row![icon(Icon::Restart, 10.0)]
        .spacing(tokens::space::XS)
        .align_y(Alignment::Center);
    if fit.session_id {
        content = content.push(
            text(label)
                .size(tokens::font_size::LABEL)
                .font(hangar_theme::fonts::mono()),
        );
    }
    tip::bottom(
        t,
        iced::widget::button(content)
            .padding(Padding {
                top: 0.0,
                right: 6.0,
                bottom: 0.0,
                left: 5.0,
            })
            .height(18)
            .style(style::button::toggle(t, pane.session_id.is_some()))
            .on_press(wrap(Msg::Sessions(head.ws.id.clone(), pane.id.clone()))),
        words,
    )
}

/// The activity's word for a pane out of sight (the zoom bar's chips).
pub fn activity_word(ctx: &Ctx<'_>, activity: AgentActivity) -> String {
    ctx.t(crate::panes::status::activity_key(activity))
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_header_shows_everything() {
        let fit = Fit::of(800.0);
        assert!(
            fit.shell && fit.session_id && fit.gauge_pct && fit.diff && fit.commands && fit.zoom
        );
        assert!(!fit.short_states && !fit.short_agent);
    }

    #[test]
    fn details_go_in_the_order_of_the_stylesheet() {
        // 46em: the shell picker first, and the waiting word shortens.
        let fit = Fit::of(46.0 * EM);
        assert!(!fit.shell && fit.short_waiting && fit.session_id);
        // 40em: the session's id, and every word shortens.
        let fit = Fit::of(40.0 * EM);
        assert!(!fit.session_id && fit.short_states && fit.gauge_pct);
        // 33em: the gauge's figure and the diff button.
        let fit = Fit::of(33.0 * EM);
        assert!(!fit.gauge_pct && !fit.diff && fit.commands);
        // 26em: the commands and the session button.
        let fit = Fit::of(26.0 * EM);
        assert!(!fit.commands && !fit.session && fit.meta && fit.zoom);
        // 24em: the metadata and the zoom.
        let fit = Fit::of(24.0 * EM);
        assert!(!fit.meta && !fit.zoom && !fit.short_agent);
        // 20em: the agent's first word.
        assert!(Fit::of(20.0 * EM).short_agent);
        // 11em: the glyph and the name.
        let fit = Fit::of(11.0 * EM);
        assert!(!fit.agent && !fit.queue && !fit.state);
    }

    #[test]
    fn what_the_header_folds_the_menu_takes() {
        assert_eq!(Fit::of(800.0).folded(), Folded::default());
        assert_eq!(
            Fit::of(30.0 * EM).folded(),
            Folded {
                commands: false,
                diff: true,
                zoom: false
            }
        );
        assert_eq!(
            Fit::of(20.0 * EM).folded(),
            Folded {
                commands: true,
                diff: true,
                zoom: true
            }
        );
    }
}
