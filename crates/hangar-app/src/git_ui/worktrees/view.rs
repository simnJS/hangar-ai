//! What the worktree panel draws: the list of worktrees with a row of
//! actions each, the cleanup, the form that makes a new one, and the footer.

use std::collections::HashMap;

use hangar_core::git::{Branch, MergedWorktree, MigrateResult, Worktree};
use hangar_i18n::{Key, Translator, args};
use hangar_theme::tokens::{BORDER_WIDTH, font_size, radius, space};
use hangar_theme::{Theme, fonts, style};
use hangar_ui::badge::{Tone, badge};
use hangar_ui::button::{self};
use hangar_ui::input::{checkbox, segmented_labels, text_field};
use hangar_ui::menu::{self, Entry, item};
use hangar_ui::menu_select::menu_select;
use hangar_ui::{dialog, tip};
use iced::widget::{column, container, row, rule, space as filler, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding};

use super::create::{Mode, PlanError};
use super::rows::{Track, normalize, parse_track, relative_age, shorten_path};
use super::{Busy, Panel, Picker, WtMsg, wrap, wrap_diff};
use crate::ctx::Ctx;
use crate::git_ui::diff_view;
use crate::message::Message;

/// The shape of a notice: plain, an error (the danger colour) or half a
/// failure (the warning colour) (`.notice`, `.notice--error`,
/// `.wt__notice--warn`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Note {
    Plain,
    Error,
    Warn,
}

fn note_box<'a>(t: &'a Theme, note: Note, body: Element<'a, Message>) -> Element<'a, Message> {
    let c = &t.colors;
    let (border, wash) = match note {
        Note::Plain => (c.line, c.bg),
        Note::Error => (c.danger.scale_alpha(0.4), c.danger.scale_alpha(0.08)),
        Note::Warn => (c.warning.scale_alpha(0.4), c.bg),
    };
    container(body)
        .padding(Padding::from([6.0, 6.0]).left(10.0))
        .width(Length::Fill)
        .align_y(Alignment::Center)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(Background::Color(wash)),
            border: Border {
                color: border,
                width: 1.0,
                radius: radius::MD.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// The words of a notice, in the tone's ink.
fn note_text<'a>(t: &'a Theme, note: Note, words: impl Into<String>) -> Element<'a, Message> {
    let c = &t.colors;
    let ink = match note {
        Note::Plain => c.fg_muted,
        Note::Error => c.danger,
        Note::Warn => c.warning,
    };
    text(words.into())
        .size(font_size::UI)
        .style(move |_: &iced::Theme| text::Style { color: Some(ink) })
        .width(Length::Fill)
        .into()
}

/// A notice with only words.
fn notice<'a>(t: &'a Theme, note: Note, words: impl Into<String>) -> Element<'a, Message> {
    note_box(t, note, note_text(t, note, words))
}

/// A line of the quiet colour that says there is nothing (`.modal__empty`).
fn empty<'a>(t: &'a Theme, words: impl Into<String>) -> Element<'a, Message> {
    container(
        text(words.into())
            .size(font_size::UI)
            .style(style::text::subtle(t)),
    )
    .padding(Padding::from([8.0, 0.0]))
    .into()
}

/// A small button, the width of its label.
fn small<'a>(t: &'a Theme, label: impl Into<String>, msg: Option<WtMsg>) -> Element<'a, Message> {
    button::secondary(t, label)
        .small()
        .on_press_maybe(msg.map(wrap))
        .into()
}

fn small_danger<'a>(
    t: &'a Theme,
    label: impl Into<String>,
    msg: Option<WtMsg>,
) -> Element<'a, Message> {
    button::danger(t, label)
        .small()
        .on_press_maybe(msg.map(wrap))
        .into()
}

/// The panel, or, while a row's changes are being read, the diff in its place.
pub fn view<'a>(panel: &'a Panel, ctx: Ctx<'a>) -> Element<'a, Message> {
    if let Some(diff) = &panel.diff_for {
        return diff_view::view(diff, ctx, wrap_diff, wrap(WtMsg::CloseDiff));
    }
    dialog::over(
        ctx.theme,
        filler::vertical().height(Length::Fill),
        Some(modal(panel, ctx)),
    )
    .into()
}

/// The panel's dialog, without the veil it is shown over.
pub(crate) fn modal<'a>(panel: &'a Panel, ctx: Ctx<'a>) -> dialog::Dialog<'a, Message> {
    let (t, tr) = (ctx.theme, ctx.i18n);
    let busy = panel.busy.is_some();

    let footer_left = row![
        button::ghost(
            t,
            tr.t(if panel.busy == Some(Busy::Fetch) {
                Key::WorktreesFetching
            } else {
                Key::WorktreesFetch
            })
            .into_owned(),
        )
        .tip(tr.t(Key::WorktreesFetchHint).into_owned())
        .on_press_maybe((!busy && !panel.loading).then(|| wrap(WtMsg::Fetch))),
        button::ghost(t, tr.t(Key::WorktreesPrune).into_owned())
            .tip(tr.t(Key::WorktreesPruneHint).into_owned())
            .on_press_maybe((!busy && !panel.loading).then(|| wrap(WtMsg::Prune))),
        button::ghost(t, tr.t(Key::WorktreesCleanup).into_owned())
            .tip(tr.t(Key::WorktreesCleanupHint).into_owned())
            .on_press_maybe(
                (!busy && !panel.loading && panel.cleanup.is_none())
                    .then(|| wrap(WtMsg::OpenCleanup))
            ),
        button::ghost(t, tr.t(Key::WorktreesRefresh).into_owned())
            .on_press_maybe((!busy && !panel.loading).then(|| wrap(WtMsg::Refresh))),
    ]
    .spacing(space::XS);

    dialog::dialog(t, tr.t(Key::WorktreesTitle).into_owned(), body(panel, ctx))
        .subtitle(panel.cwd.clone())
        .width(680.0)
        .body_max_height(600.0)
        .lead(footer_left)
        .action(
            button::ghost(t, tr.t(Key::WorktreesClose).into_owned()).on_press(wrap(WtMsg::Close)),
        )
        .close_tip(tr.t(Key::WorktreesClose).into_owned())
        .on_close(wrap(WtMsg::Close))
}

/// Where each worktree is already open — a workspace on it, or a pane
/// spawned into it — keyed the way paths compare. Removing a checkout that
/// live terminals are standing in deletes the ground from under them (and on
/// Windows their cwd handles fail the deletion halfway), so the Remove button
/// asks for that workspace to go first.
fn open_worktrees(ctx: Ctx<'_>) -> HashMap<String, String> {
    let mut open = HashMap::new();
    for ws in &ctx.model.state().workspaces {
        for pane in &ws.panes {
            if let Some(cwd) = &pane.cwd {
                open.insert(normalize(cwd), ws.name.clone());
            }
        }
        // Last, so the workspace's own name wins over a pane's when both point
        // at the same folder.
        open.insert(normalize(&ws.cwd), ws.name.clone());
    }
    open
}

fn body<'a>(panel: &'a Panel, ctx: Ctx<'a>) -> Element<'a, Message> {
    let (t, tr) = (ctx.theme, ctx.i18n);
    let mut parts: Vec<Element<'a, Message>> = Vec::new();

    if panel.loading {
        parts.push(empty(t, tr.t(Key::WorktreesLoading).into_owned()));
    }
    match &panel.info {
        Some(info) if !panel.loading && !info.installed => {
            parts.push(notice(
                t,
                Note::Error,
                tr.t(Key::WorktreesNoGit).into_owned(),
            ));
        }
        Some(info) if !panel.loading && info.installed && !info.is_repo => {
            parts.push(notice(
                t,
                Note::Plain,
                tr.t(Key::WorktreesNotRepo).into_owned(),
            ));
        }
        Some(info) if !panel.loading && info.installed && info.is_repo => {
            parts.push(list_section(panel, ctx));
            parts.push(form(panel, ctx));
        }
        _ => {}
    }
    if let Some(flash) = &panel.notice {
        let tone = if flash.warn { Note::Warn } else { Note::Plain };
        parts.push(notice(t, tone, flash.text.clone()));
    }
    if let Some(error) = &panel.error {
        parts.push(notice(t, Note::Error, error.clone()));
    }
    column(parts).spacing(space::XL).width(Length::Fill).into()
}

// ---- The list ---------------------------------------------------------------------------------

fn list_section<'a>(panel: &'a Panel, ctx: Ctx<'a>) -> Element<'a, Message> {
    let (t, tr) = (ctx.theme, ctx.i18n);
    let mut parts: Vec<Element<'a, Message>> = Vec::new();

    if let Some(info) = &panel.info
        && info.is_linked_worktree
        && let Some(root) = &info.main_root
    {
        parts.push(
            text(
                tr.t_with(Key::WorktreesLinked, &args!("root" => root.as_str()))
                    .into_owned(),
            )
            .size(font_size::CAPTION_LG)
            .style(style::text::secondary(t))
            .into(),
        );
    }
    if panel.worktrees.is_empty() {
        parts.push(empty(t, tr.t(Key::WorktreesNone).into_owned()));
    } else {
        let open = open_worktrees(ctx);
        let rows = panel.worktrees.iter().enumerate().map(|(at, worktree)| {
            let line = worktree_row(panel, ctx, worktree, &open);
            if at == 0 {
                line
            } else {
                column![
                    rule::horizontal(BORDER_WIDTH).style(style::rule::hairline(t)),
                    line
                ]
                .into()
            }
        });
        let c = &t.colors;
        let (page, edge) = (c.bg, c.line);
        parts.push(
            container(column(rows))
                .width(Length::Fill)
                .style(move |_: &iced::Theme| container::Style {
                    background: Some(Background::Color(page)),
                    border: Border {
                        color: edge,
                        width: 1.0,
                        radius: radius::LG.into(),
                    },
                    ..container::Style::default()
                })
                .into(),
        );
    }

    // The branch outlives the worktree that held it; removing it too is one
    // click, but never the default.
    if let Some(orphan) = &panel.orphan {
        let words = orphan.message.clone().unwrap_or_else(|| {
            tr.t_with(
                Key::WorktreesOrphanAsk,
                &args!("name" => orphan.name.as_str()),
            )
            .into_owned()
        });
        let tone = if orphan.needs_force {
            Note::Error
        } else {
            Note::Plain
        };
        let busy = panel.busy.is_some();
        parts.push(note_box(
            t,
            tone,
            row![
                note_text(t, tone, words),
                small_danger(
                    t,
                    tr.t(if orphan.needs_force {
                        Key::WorktreesOrphanForce
                    } else {
                        Key::WorktreesOrphanDelete
                    })
                    .into_owned(),
                    (!busy).then_some(WtMsg::OrphanDelete),
                ),
                small(
                    t,
                    tr.t(Key::WorktreesOrphanKeep).into_owned(),
                    (!busy).then_some(WtMsg::OrphanKeep)
                ),
            ]
            .spacing(space::MD)
            .align_y(Alignment::Center)
            .into(),
        ));
    }

    if panel.cleanup.is_some() {
        parts.push(cleanup_section(panel, ctx));
    }
    column(parts).spacing(space::MD).width(Length::Fill).into()
}

/// A word on a capsule, with its explanation on hover when it has one.
fn badge_with<'a>(
    t: &'a Theme,
    label: impl Into<String>,
    tone: Tone,
    hint: Option<String>,
) -> Element<'a, Message> {
    let capsule = badge(t, label, tone);
    match hint {
        Some(hint) => tip::bottom(t, capsule, hint),
        None => capsule,
    }
}

fn worktree_row<'a>(
    panel: &'a Panel,
    ctx: Ctx<'a>,
    worktree: &'a Worktree,
    open: &HashMap<String, String>,
) -> Element<'a, Message> {
    let (t, tr) = (ctx.theme, ctx.i18n);
    let c = &t.colors;
    let busy = panel.busy.is_some();
    let meta = worktree
        .branch
        .as_ref()
        .and_then(|branch| panel.locals().find(|local| &local.name == branch));
    let track = parse_track(meta.and_then(|meta| meta.track.as_deref()));
    // The panel's own workspace holds the current row, which already has no
    // Remove; this catches every other one.
    let held_by = if worktree.is_current {
        None
    } else {
        open.get(&normalize(&worktree.path))
    };

    // The branch and its badges.
    let name = worktree
        .branch
        .clone()
        .or_else(|| worktree.head.clone())
        .unwrap_or_else(|| "—".into());
    let mut label = row![
        text(name)
            .size(font_size::UI_LG)
            .font(fonts::ui_semibold())
            .style(style::text::primary(t))
    ]
    .spacing(space::SM)
    .align_y(Alignment::Center);
    if worktree.detached {
        label = label.push(badge(
            t,
            tr.t(Key::WorktreesDetached).into_owned(),
            Tone::Neutral,
        ));
    }
    if worktree.bare {
        label = label.push(badge(
            t,
            tr.t(Key::WorktreesBare).into_owned(),
            Tone::Neutral,
        ));
    }
    if worktree.is_main {
        label = label.push(badge(
            t,
            tr.t(Key::WorktreesMain).into_owned(),
            Tone::Accent,
        ));
    }
    if worktree.is_current {
        label = label.push(badge(
            t,
            tr.t(Key::WorktreesCurrent).into_owned(),
            Tone::Accent,
        ));
    }
    match track {
        Some(Track { gone: true, .. }) => {
            label = label.push(badge_with(
                t,
                tr.t(Key::WorktreesTrackGone).into_owned(),
                Tone::Danger,
                meta.and_then(|meta| meta.track.clone()),
            ));
        }
        Some(track) => {
            label = label.push(badge_with(
                t,
                track.arrows(),
                Tone::Neutral,
                meta.and_then(|meta| meta.track.clone()),
            ));
        }
        None => {}
    }
    if let Some(reason) = &worktree.locked {
        let hint = if reason.is_empty() {
            tr.t(Key::WorktreesLockedNoReason).into_owned()
        } else {
            reason.clone()
        };
        label = label.push(badge_with(
            t,
            tr.t(Key::WorktreesLocked).into_owned(),
            Tone::Danger,
            Some(hint),
        ));
    }
    if let Some(reason) = &worktree.prunable {
        label = label.push(badge_with(
            t,
            tr.t(Key::WorktreesPrunable).into_owned(),
            Tone::Danger,
            (!reason.is_empty()).then(|| reason.clone()),
        ));
    }

    let mut lines = column![label].spacing(3);
    lines = lines.push(tip::bottom(
        t,
        text(shorten_path(&worktree.path, 54))
            .size(font_size::CAPTION)
            .font(fonts::mono())
            .style(style::text::secondary(t)),
        worktree.path.clone(),
    ));
    if let Some(meta) = meta {
        let mut stats = row![].spacing(space::MD).align_y(Alignment::Center);
        if let Some(head) = &meta.head {
            stats = stats.push(
                text(head.clone())
                    .size(font_size::LABEL)
                    .font(fonts::mono())
                    .style(style::text::subtle(t)),
            );
        }
        if let Some(author) = &meta.author_name {
            stats = stats.push(
                text(author.clone())
                    .size(font_size::LABEL)
                    .style(style::text::subtle(t)),
            );
        }
        if let Some(date) = meta.committer_date {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_secs() as i64)
                .unwrap_or(date);
            stats = stats.push(
                text(relative_age(now, date).text(tr))
                    .size(font_size::LABEL)
                    .style(style::text::subtle(t)),
            );
        }
        lines = lines.push(match &meta.subject {
            Some(subject) => tip::bottom(t, stats, subject.clone()),
            None => stats.into(),
        });
    }

    // The actions: flat, side by side. A bare entry has no files to diff, and
    // a prunable one no folder left to read them from.
    let path = &worktree.path;
    let mut actions = row![].spacing(2).align_y(Alignment::Center);
    let ghost = |label: String, hint: String, msg: Option<WtMsg>| -> Element<'a, Message> {
        button::ghost(t, label)
            .small()
            .tip(hint)
            .on_press_maybe(msg.map(wrap))
            .into()
    };
    actions = actions.push(ghost(
        tr.t(Key::DiffWorktree).into_owned(),
        tr.t(Key::DiffWorktreeHint).into_owned(),
        (!busy && !worktree.bare && worktree.prunable.is_none())
            .then(|| WtMsg::ShowChanges(path.clone())),
    ));
    actions = actions.push(ghost(
        tr.t(Key::WorktreesOpenWorkspace).into_owned(),
        tr.t(Key::WorktreesOpenWorkspaceHint).into_owned(),
        (!busy).then(|| WtMsg::OpenWorkspace(path.clone(), worktree.branch.clone())),
    ));
    actions = actions.push(ghost(
        tr.t(Key::WorktreesOpenPane).into_owned(),
        tr.t(if worktree.is_current {
            Key::WorktreesOpenPaneCurrent
        } else {
            Key::WorktreesOpenPaneHint
        })
        .into_owned(),
        (!worktree.is_current && !busy).then(|| WtMsg::OpenPane(path.clone())),
    ));
    // The main checkout is where the changes go, and a bare or prunable entry
    // has nothing to take.
    if !worktree.is_main && !worktree.bare && worktree.prunable.is_none() {
        actions = actions.push(ghost(
            tr.t(Key::WorktreesMigrate).into_owned(),
            tr.t(Key::WorktreesMigrateHint).into_owned(),
            (!busy).then(|| WtMsg::MigrateAsk(path.clone())),
        ));
    }
    // git refuses to lock the main worktree, so the button only exists where
    // it can work.
    if !worktree.is_main {
        let locked = worktree.locked.is_some();
        actions = actions.push(ghost(
            tr.t(if locked {
                Key::WorktreesUnlock
            } else {
                Key::WorktreesLock
            })
            .into_owned(),
            tr.t(if locked {
                Key::WorktreesUnlockHint
            } else {
                Key::WorktreesLockHint
            })
            .into_owned(),
            (!busy).then(|| WtMsg::ToggleLock(path.clone())),
        ));
    }
    if !worktree.is_main && !worktree.is_current {
        let armed = panel
            .armed
            .as_ref()
            .is_some_and(|armed| &armed.path == path);
        let hint = match held_by {
            Some(name) => tr
                .t_with(Key::WorktreesInUse, &args!("name" => name.as_str()))
                .into_owned(),
            None => tr.t(Key::WorktreesRemoveHint).into_owned(),
        };
        actions = actions.push(tip::bottom(
            t,
            button::danger(
                t,
                tr.t(if armed {
                    Key::WorktreesRemoveConfirm
                } else {
                    Key::WorktreesRemove
                })
                .into_owned(),
            )
            .small()
            .on_press_maybe(
                (!busy && held_by.is_none()).then(|| wrap(WtMsg::RemovePressed(path.clone()))),
            ),
            hint,
        ));
    }
    lines = lines.push(actions);

    if panel.migrating.as_ref() == Some(path) {
        lines = lines.push(note_box(
            t,
            Note::Plain,
            row![
                note_text(t, Note::Plain, tr.t(Key::WorktreesMigrateWarn).into_owned()),
                small(
                    t,
                    tr.t(Key::WorktreesMigrateConfirm).into_owned(),
                    (!busy).then(|| WtMsg::MigrateRun(path.clone())),
                ),
                small(
                    t,
                    tr.t(Key::WorktreesMigrateCancel).into_owned(),
                    Some(WtMsg::MigrateCancel)
                ),
            ]
            .spacing(space::MD)
            .align_y(Alignment::Center)
            .into(),
        ));
    }
    if let Some((of, result)) = &panel.migrated
        && of == path
    {
        let tone = if result.outcome == "moved" {
            Note::Plain
        } else {
            Note::Error
        };
        lines = lines.push(note_box(
            t,
            tone,
            row![
                note_text(t, tone, migrate_words(tr, result)),
                small(t, "×", Some(WtMsg::DismissMigrated)),
            ]
            .spacing(space::MD)
            .align_y(Alignment::Center)
            .into(),
        ));
    }
    if let Some(blocked) = &panel.blocked
        && &blocked.path == path
    {
        let words = blocked
            .message
            .clone()
            .filter(|message| !message.is_empty())
            .unwrap_or_else(|| tr.t(Key::WorktreesRemoveHint).into_owned());
        let again = if blocked.hook {
            WtMsg::Remove {
                path: path.clone(),
                force: false,
                skip_hook: true,
            }
        } else {
            WtMsg::Remove {
                path: path.clone(),
                force: true,
                skip_hook: false,
            }
        };
        let label = tr.t(if blocked.hook {
            Key::WorktreesHookRemoveAnyway
        } else {
            Key::WorktreesForce
        });
        let action: Element<'a, Message> = match held_by {
            Some(name) => tip::bottom(
                t,
                small_danger(t, label.into_owned(), None),
                tr.t_with(Key::WorktreesInUse, &args!("name" => name.as_str()))
                    .into_owned(),
            ),
            None => small_danger(t, label.into_owned(), (!busy).then_some(again)),
        };
        lines = lines.push(note_box(
            t,
            Note::Error,
            row![note_text(t, Note::Error, words), action]
                .spacing(space::MD)
                .align_y(Alignment::Center)
                .into(),
        ));
    }
    container(lines)
        .padding(Padding::from([10.0, 12.0]).bottom(8.0))
        .width(Length::Fill)
        .into()
}

/// What a migration said that was not "moved, nothing left over"
/// (`worktrees.migrate*`).
fn migrate_words(tr: Translator, result: &MigrateResult) -> String {
    match result.outcome {
        "nothing" => tr.t(Key::WorktreesMigrateNothing).into_owned(),
        "moved" => tr.t(Key::WorktreesMigrateDropFailed).into_owned(),
        "conflict" => {
            let mut words = format!(
                "{} {}",
                tr.t(Key::WorktreesMigrateConflict),
                result.files.join(", ")
            );
            if result.more > 0 {
                words.push(' ');
                words.push_str(&tr.tn(Key::WorktreesMigrateMore, result.more, &[]));
            }
            words
        }
        "applyFailed" => {
            let stash = result.stash.clone().unwrap_or_default();
            let mut words = tr
                .t_with(
                    Key::WorktreesMigrateFailed,
                    &args!("stash" => stash.as_str()),
                )
                .into_owned();
            if let Some(message) = &result.message
                && !message.is_empty()
            {
                words.push_str(&format!(" ({message})"));
            }
            words
        }
        other => other.to_string(),
    }
}

// ---- The cleanup ------------------------------------------------------------------------------

fn cleanup_section<'a>(panel: &'a Panel, ctx: Ctx<'a>) -> Element<'a, Message> {
    let (t, tr) = (ctx.theme, ctx.i18n);
    let Some(cleanup) = &panel.cleanup else {
        return filler::vertical().into();
    };
    let busy = panel.busy.is_some();
    let open = open_worktrees(ctx);
    let mut parts: Vec<Element<'a, Message>> = vec![
        text(tr.t(Key::WorktreesCleanupTitle).into_owned())
            .size(font_size::UI)
            .font(fonts::ui_semibold())
            .style(style::text::primary(t))
            .into(),
    ];

    match &cleanup.items {
        None => parts.push(empty(t, tr.t(Key::WorktreesCleanupLoading).into_owned())),
        Some(items) if items.is_empty() && cleanup.removed == 0 => {
            parts.push(empty(t, tr.t(Key::WorktreesCleanupNone).into_owned()));
        }
        Some(items) if !items.is_empty() => {
            parts.push(
                text(tr.t(Key::WorktreesCleanupIntro).into_owned())
                    .size(font_size::CAPTION_LG)
                    .style(style::text::secondary(t))
                    .into(),
            );
            for item in items {
                parts.push(cleanup_item(panel, ctx, item, &open));
            }
            let picked = cleanup.picked();
            let dirty = cleanup.picked_dirty();
            let mut confirm = row![].spacing(space::MD).align_y(Alignment::Center);
            if cleanup.confirming && dirty > 0 {
                confirm = confirm.push(note_text(
                    t,
                    Note::Plain,
                    tr.t_with(Key::WorktreesCleanupConfirmDirty, &args!("n" => dirty))
                        .into_owned(),
                ));
            }
            let label = if panel.busy == Some(Busy::Cleanup) {
                tr.t(Key::WorktreesCleanupRemoving).into_owned()
            } else {
                tr.tn(
                    if cleanup.confirming {
                        Key::WorktreesCleanupConfirm
                    } else {
                        Key::WorktreesCleanupRemove
                    },
                    picked.len(),
                    &[],
                )
                .into_owned()
            };
            confirm = confirm.push(small_danger(
                t,
                label,
                (!busy && !picked.is_empty()).then_some(WtMsg::CleanupRemovePressed),
            ));
            parts.push(note_box(t, Note::Plain, confirm.into()));
        }
        Some(_) => {}
    }

    if cleanup.removed > 0 {
        parts.push(
            text(
                tr.tn(Key::WorktreesCleanupRemoved, cleanup.removed, &[])
                    .into_owned(),
            )
            .size(font_size::CAPTION_LG)
            .style(style::text::secondary(t))
            .into(),
        );
    }

    if !cleanup.forced.is_empty() {
        let mut held = column![note_text(
            t,
            Note::Error,
            tr.t(Key::WorktreesCleanupNeedsForce).into_owned()
        )]
        .spacing(space::SM);
        for (item, message) in &cleanup.forced {
            let reason = message.clone().unwrap_or_default();
            let mut line = row![note_text(
                t,
                Note::Error,
                format!("{}: {reason}", item.branch)
            )]
            .spacing(space::MD)
            .align_y(Alignment::Center);
            // A lock is not something force removes.
            if !reason.to_lowercase().contains("locked") {
                line = line.push(small_danger(
                    t,
                    tr.t(Key::WorktreesForce).into_owned(),
                    (!busy).then(|| WtMsg::CleanupForce(item.path.clone())),
                ));
            }
            held = held.push(line);
        }
        parts.push(note_box(t, Note::Error, held.into()));
    }

    if !cleanup.failed.is_empty() {
        let mut failed = column![note_text(
            t,
            Note::Error,
            tr.t(Key::WorktreesCleanupFailed).into_owned()
        )]
        .spacing(2);
        for (item, message) in &cleanup.failed {
            failed = failed.push(note_text(
                t,
                Note::Error,
                format!("{}: {message}", item.branch),
            ));
        }
        parts.push(note_box(t, Note::Error, failed.into()));
    }

    if !cleanup.branches.is_empty() {
        let tone = if cleanup.branches_force {
            Note::Error
        } else {
            Note::Plain
        };
        let words = format!(
            "{} {}",
            tr.t(if cleanup.branches_force {
                Key::WorktreesCleanupBranchesForce
            } else {
                Key::WorktreesCleanupBranches
            }),
            cleanup.branches.join(", ")
        );
        parts.push(note_box(
            t,
            tone,
            row![
                note_text(t, tone, words),
                small_danger(
                    t,
                    tr.t(if cleanup.branches_force {
                        Key::WorktreesOrphanForce
                    } else {
                        Key::WorktreesCleanupBranchesDelete
                    })
                    .into_owned(),
                    (!busy).then_some(WtMsg::CleanupDeleteBranches),
                ),
                small(
                    t,
                    tr.t(Key::WorktreesCleanupBranchesKeep).into_owned(),
                    (!busy).then_some(WtMsg::CleanupKeepBranches),
                ),
            ]
            .spacing(space::MD)
            .align_y(Alignment::Center)
            .into(),
        ));
    }

    if cleanup.branches_deleted > 0 {
        parts.push(
            text(
                tr.tn(
                    Key::WorktreesCleanupBranchesDone,
                    cleanup.branches_deleted,
                    &[],
                )
                .into_owned(),
            )
            .size(font_size::CAPTION_LG)
            .style(style::text::secondary(t))
            .into(),
        );
    }

    parts.push(small(
        t,
        tr.t(Key::WorktreesCleanupBack).into_owned(),
        (!busy).then_some(WtMsg::CleanupBack),
    ));
    column(parts).spacing(space::MD).width(Length::Fill).into()
}

/// One worktree of the cleanup: a tick box, its branch and path, and the
/// reasons to leave it alone.
fn cleanup_item<'a>(
    panel: &'a Panel,
    ctx: Ctx<'a>,
    item: &'a MergedWorktree,
    open: &HashMap<String, String>,
) -> Element<'a, Message> {
    let (t, tr) = (ctx.theme, ctx.i18n);
    let busy = panel.busy.is_some();
    let selected = panel
        .cleanup
        .as_ref()
        .is_some_and(|cleanup| cleanup.selected.contains(&item.path));
    let locked = item.locked.is_some();
    let path = item.path.clone();
    // Without a handler the box is drawn disabled.
    let mut tick = checkbox(t, "", selected);
    if !busy && !locked {
        tick = tick.on_toggle(move |_| wrap(WtMsg::CleanupToggle(path.clone())));
    }
    let mut line = row![
        tick,
        text(item.branch.clone())
            .size(font_size::UI_LG)
            .font(fonts::ui_semibold())
            .style(style::text::primary(t)),
        text(shorten_path(&item.path, 54))
            .size(font_size::CAPTION)
            .font(fonts::mono())
            .style(style::text::secondary(t)),
    ]
    .spacing(space::MD)
    .align_y(Alignment::Center);
    if locked {
        line = line.push(badge(
            t,
            tr.t(Key::WorktreesLocked).into_owned(),
            Tone::Danger,
        ));
    }
    if item.no_own_commit {
        line = line.push(badge_with(
            t,
            tr.t(Key::WorktreesCleanupNoOwnCommit).into_owned(),
            Tone::Neutral,
            Some(tr.t(Key::WorktreesCleanupNoOwnCommitHint).into_owned()),
        ));
    }
    if item.held {
        let who = open
            .get(&normalize(&item.path))
            .cloned()
            .unwrap_or_default();
        line = line.push(badge_with(
            t,
            tr.t_with(Key::WorktreesCleanupHeld, &args!("name" => who.as_str()))
                .into_owned(),
            Tone::Danger,
            Some(tr.t(Key::WorktreesCleanupHeldHint).into_owned()),
        ));
    }
    match item.dirty {
        None => {
            line = line.push(badge(
                t,
                tr.t(Key::WorktreesCleanupDirtyUnknown).into_owned(),
                Tone::Danger,
            ));
        }
        Some(n) if n > 0 => {
            line = line.push(badge(
                t,
                tr.tn(Key::WorktreesCleanupDirty, n, &[]).into_owned(),
                Tone::Danger,
            ));
        }
        Some(_) => {}
    }
    let hint = if locked {
        tr.t(Key::WorktreesCleanupLockedHint).into_owned()
    } else {
        item.path.clone()
    };
    tip::bottom(t, line, hint)
}

// ---- The form ---------------------------------------------------------------------------------

fn form<'a>(panel: &'a Panel, ctx: Ctx<'a>) -> Element<'a, Message> {
    let (t, tr) = (ctx.theme, ctx.i18n);
    let c = &t.colors;
    let resolved = panel.resolved();
    let sanitized = panel.sanitized();
    let label = |key: Key| -> Element<'a, Message> {
        container(
            text(tr.t(key).into_owned())
                .size(font_size::UI)
                .style(style::text::secondary(t)),
        )
        .width(96.0)
        .padding(Padding::ZERO.top(5.0))
        .align_x(iced::alignment::Horizontal::Right)
        .into()
    };
    let hint = |words: String, color: Option<Color>| -> Element<'a, Message> {
        text(words)
            .size(font_size::CAPTION_LG)
            .style(move |_: &iced::Theme| text::Style {
                color: Some(color.unwrap_or(c.fg_subtle)),
            })
            .into()
    };

    let mut rows: Vec<Element<'a, Message>> = Vec::new();

    // New branch, or one that exists.
    rows.push(
        row![
            label(Key::WorktreesCreate),
            segmented_labels(
                t,
                vec![
                    (Mode::New, tr.t(Key::WorktreesModeNew).into_owned()),
                    (
                        Mode::Existing,
                        tr.t(Key::WorktreesModeExisting).into_owned()
                    ),
                ],
                &panel.form.mode,
                |mode| wrap(WtMsg::SetMode(mode)),
            ),
        ]
        .spacing(space::LG)
        .into(),
    );

    // The branch.
    let mut field = column![].spacing(space::XS).width(Length::Fill);
    if panel.form.mode == Mode::New {
        field = field.push(
            text_field(t, &panel.form.new_branch)
                .placeholder(plain(tr, Key::WorktreesBranchPlaceholder))
                .on_input(|name| wrap(WtMsg::NewBranch(name))),
        );
        let trimmed = panel.form.new_branch.trim();
        if !sanitized.is_empty() && sanitized != trimmed {
            field = field.push(hint(
                tr.t_with(
                    Key::WorktreesSanitized,
                    &args!("name" => sanitized.as_str()),
                )
                .into_owned(),
                None,
            ));
        }
    } else {
        let shown = panel
            .form
            .existing
            .as_deref()
            .and_then(|key| {
                panel
                    .branches
                    .iter()
                    .find(|branch| super::key_of(branch) == key)
            })
            .map(|branch| branch.name.clone());
        let placeholder = shown.is_none();
        field = field.push(
            menu_select(
                t,
                shown.unwrap_or_else(|| tr.t(Key::WorktreesPickBranch).into_owned()),
                picker_entries(panel, ctx, true, panel.form.existing.as_deref(), None),
            )
            .placeholder(placeholder)
            .open(panel.picker == Some(Picker::Existing))
            .on_toggle(wrap(WtMsg::TogglePicker(Picker::Existing)))
            .on_dismiss(wrap(WtMsg::ClosePicker)),
        );
        let nothing_to_pick = panel.locals().all(|branch| branch.worktree_path.is_some())
            && !panel.branches.iter().any(|branch| branch.kind == "remote");
        if nothing_to_pick {
            field = field.push(hint(tr.t(Key::WorktreesNoFreeBranch).into_owned(), None));
        }
    }
    if let Some(error) = &resolved.error {
        field = field.push(hint(plan_error_words(error, tr), Some(c.danger)));
    }
    if panel.form.mode == Mode::Existing
        && let Some(plan) = &resolved.plan
        && plan.create_branch
    {
        field = field.push(hint(
            tr.t_with(
                Key::WorktreesWillTrack,
                &args!(
                    "name" => plan.branch.as_str(),
                    "remote" => plan.base_ref.as_deref().unwrap_or("")
                ),
            )
            .into_owned(),
            None,
        ));
    }
    rows.push(
        row![label(Key::WorktreesBranch), field]
            .spacing(space::LG)
            .into(),
    );

    // What a new branch is based on.
    if panel.form.mode == Mode::New {
        let current = panel
            .info
            .as_ref()
            .and_then(|info| info.branch.clone().or_else(|| info.head.clone()))
            .unwrap_or_else(|| "HEAD".into());
        let current_words = tr
            .t_with(Key::WorktreesBase, &args!("branch" => current.as_str()))
            .into_owned();
        let shown = panel
            .form
            .new_base
            .as_deref()
            .and_then(|key| {
                panel
                    .branches
                    .iter()
                    .find(|branch| super::key_of(branch) == key)
            })
            .map(|branch| branch.name.clone())
            .unwrap_or_else(|| current_words.clone());
        rows.push(
            row![
                label(Key::WorktreesBaseLabel),
                container(
                    menu_select(
                        t,
                        shown,
                        picker_entries(
                            panel,
                            ctx,
                            false,
                            panel.form.new_base.as_deref(),
                            Some(current_words)
                        ),
                    )
                    .open(panel.picker == Some(Picker::Base))
                    .on_toggle(wrap(WtMsg::TogglePicker(Picker::Base)))
                    .on_dismiss(wrap(WtMsg::ClosePicker))
                )
                .width(Length::Fill),
            ]
            .spacing(space::LG)
            .into(),
        );
    }

    // Where it goes.
    rows.push(
        row![
            label(Key::WorktreesPath),
            column![
                text_field(t, panel.folder_text())
                    .mono()
                    .on_input(|path| wrap(WtMsg::Folder(path))),
                hint(tr.t(Key::WorktreesPathHint).into_owned(), None),
            ]
            .spacing(space::XS)
            .width(Length::Fill),
        ]
        .spacing(space::LG)
        .into(),
    );

    // What comes with it, and the button.
    rows.push(
        row![
            container(filler::horizontal()).width(96.0),
            column![
                tip::bottom(
                    t,
                    checkbox(
                        t,
                        tr.t(Key::WorktreesCopy).into_owned(),
                        panel.form.copy_untracked
                    )
                    .on_toggle(|on| wrap(WtMsg::CopyUntracked(on))),
                    tr.t(Key::WorktreesCopyHint).into_owned(),
                ),
                checkbox(
                    t,
                    tr.t(Key::WorktreesOpenAfter).into_owned(),
                    panel.form.open_after
                )
                .on_toggle(|on| wrap(WtMsg::OpenAfter(on))),
                button::primary(
                    t,
                    tr.t(if panel.busy == Some(Busy::Create) {
                        Key::WorktreesCreating
                    } else {
                        Key::WorktreesCreateSubmit
                    })
                    .into_owned(),
                )
                .on_press_maybe(panel.can_create().then(|| wrap(WtMsg::Create))),
            ]
            .spacing(space::MD)
            .width(Length::Fill),
        ]
        .spacing(space::LG)
        .into(),
    );
    column(rows).spacing(space::LG).width(Length::Fill).into()
}

/// A plain (placeholder-free) string of the dictionary, as a `&'static str`,
/// for the widgets that borrow their text for as long as the view lives.
fn plain(tr: Translator, key: Key) -> &'static str {
    match tr.t(key) {
        std::borrow::Cow::Borrowed(words) => words,
        std::borrow::Cow::Owned(_) => "",
    }
}

fn plan_error_words(error: &PlanError, tr: Translator) -> String {
    error.text(tr)
}

/// The lines of a branch picker: the locals, then the remotes, each under its
/// own heading. Branches already checked out in a worktree are greyed rather
/// than hidden: knowing where one is checked out is half the answer.
///
/// `disable_taken` is the picker of an existing branch, where a held one
/// cannot be chosen; the base picker lists them all. `current` adds a first
/// line for "the current branch".
fn picker_entries<'a>(
    panel: &'a Panel,
    ctx: Ctx<'a>,
    disable_taken: bool,
    selected: Option<&str>,
    current: Option<String>,
) -> Vec<Entry<Message>> {
    let tr = ctx.i18n;
    let mut entries: Vec<Entry<Message>> = Vec::new();
    if let Some(current) = current {
        entries.push(
            item(current)
                .checked(selected.is_none())
                .on_select(wrap(WtMsg::PickBase(None)))
                .into(),
        );
    }
    let pick = |key: String| -> Message {
        if disable_taken {
            wrap(WtMsg::PickExisting(key))
        } else {
            wrap(WtMsg::PickBase(Some(key)))
        }
    };
    let taken_word = tr.t(Key::WorktreesTaken).into_owned();

    let locals: Vec<&Branch> = panel.locals().collect();
    if !locals.is_empty() {
        entries.push(menu::heading(tr.t(Key::WorktreesGroupLocal).into_owned()));
        for branch in locals {
            let key = super::key_of(branch);
            let held = disable_taken
                .then(|| branch.worktree_path.clone())
                .flatten();
            let line = match &held {
                Some(_) => item(format!("{} — {taken_word}", branch.name)),
                None => item(branch.name.clone()),
            };
            let line = line.checked(selected == Some(key.as_str()));
            entries.push(match held {
                Some(path) => line.disabled_because(path).into(),
                None => line.on_select(pick(key)).into(),
            });
        }
    }

    // A remote whose tracking branch sits in a worktree cannot be checked out
    // again: git would refuse the second one. Better said before the click.
    let remote_held: HashMap<&str, &str> = panel
        .locals()
        .filter_map(|local| Some((local.upstream.as_deref()?, local.worktree_path.as_deref()?)))
        .collect();
    let remotes: Vec<&Branch> = panel
        .branches
        .iter()
        .filter(|branch| branch.kind == "remote")
        .collect();
    if !remotes.is_empty() {
        entries.push(menu::heading(tr.t(Key::WorktreesGroupRemote).into_owned()));
        for branch in remotes {
            let key = super::key_of(branch);
            let held = disable_taken
                .then(|| remote_held.get(branch.name.as_str()).copied())
                .flatten();
            let line = match held {
                Some(_) => item(format!("{} — {taken_word}", branch.name)),
                None => item(branch.name.clone()),
            };
            let line = line.checked(selected == Some(key.as_str()));
            entries.push(match held {
                Some(path) => line.disabled_because(path.to_string()).into(),
                None => line.on_select(pick(key)).into(),
            });
        }
    }
    entries
}
