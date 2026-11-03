//! The new-workspace dialog, drawn (`WorkspaceDialog.tsx`): one form, a
//! *Source* control, and two sets of fields.

use std::borrow::Cow;
use std::fmt;

use hangar_i18n::{Key, args};
use hangar_theme::{style, tokens};
use hangar_ui::dialog::{self, Dialog};
use hangar_ui::{Icon, button as ui_button, input, text as typography};
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{
    Space, button, column, container, progress_bar, row, text, text_input, text_input::TextInput,
};
use iced::{Alignment, Border, Element, Length, Padding, Pixels};

use crate::accounts;
use crate::ctx::Ctx;
use crate::git_flow::clone::{ParseResult, overall_percent, progress_label};
use crate::git_flow::paths::clone_destination;
use crate::message::Message;
use crate::state::types::{AGENTS, AgentId};

use super::folder::{self, LAYOUTS, Root};
use super::git_form::{self, FromGitStatus};
use super::hooks;
use super::sweep;
use super::{Msg, Source, State};

/// The column of the labels (`.form-grid__label`, at most 180).
const LABEL_WIDTH: f32 = 150.0;
/// The tallest the form grows before it scrolls.
const BODY_HEIGHT: f32 = 460.0;
/// The height of the layout preview, of the proportions of the window's grid.
const PREVIEW_HEIGHT: f32 = 150.0;

fn msg(msg: Msg) -> Message {
    Message::WorkspaceDialog(msg)
}

/// A dictionary string that takes no argument is borrowed from the
/// dictionary, which is what a widget that keeps its label needs to outlive
/// the view.
fn word(text: Cow<'static, str>) -> &'static str {
    match text {
        Cow::Borrowed(text) => text,
        Cow::Owned(_) => "",
    }
}

/// Over the whole window, a veil and the dialog in the middle.
pub fn modal<'a>(ctx: Ctx<'a>, panel: Dialog<'a, Message>) -> Element<'a, Message> {
    let page = Space::new().width(Length::Fill).height(Length::Fill);
    dialog::over(ctx.theme, page, Some(panel)).into()
}

// ── pieces ────────────────────────────────────────────────────────────

/// A label at the left, right-aligned, and its field (`.form-grid`).
fn labelled<'a>(
    ctx: Ctx<'a>,
    label: Option<String>,
    field: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let label: Element<'a, Message> = match label {
        Some(label) => typography::label(t, label)
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .into(),
        None => Space::new().into(),
    };
    row![
        container(label).width(Length::Fixed(LABEL_WIDTH)),
        container(field).width(Length::Fill),
    ]
    .spacing(14)
    .into()
}

/// A one-line field, as the dialog's fields are (`.text-input`,
/// `.picker__path`).
fn field<'a>(
    ctx: Ctx<'a>,
    placeholder: &str,
    value: &'a str,
    invalid: bool,
) -> TextInput<'a, Message> {
    let t = ctx.theme;
    text_input(placeholder, value)
        .size(tokens::font_size::UI)
        .line_height(LineHeight::Absolute(Pixels(16.0)))
        .padding(Padding::from([5.0, tokens::space::MD]))
        .width(Length::Fill)
        .style(move |theme: &iced::Theme, status| {
            if invalid {
                style::text_input::field_invalid(t)(theme, status)
            } else {
                style::text_input::field(t)(theme, status)
            }
        })
}

/// A path field and the buttons beside it (`.picker`). The field is guarded
/// like every other of the dialog: the keymap takes its shortcuts first.
fn picker<'a>(
    ctx: Ctx<'a>,
    path: TextInput<'a, Message>,
    buttons: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut line = row![crate::keymap::guard(
        ctx,
        path.font(hangar_theme::fonts::mono())
    )]
    .spacing(tokens::space::SM)
    .align_y(Alignment::Center);
    for one in buttons {
        line = line.push(one);
    }
    line.into()
}

/// A boxed line of the form (`.notice`): a suggestion, or a failure.
fn notice<'a>(ctx: Ctx<'a>, content: Element<'a, Message>, error: bool) -> Element<'a, Message> {
    let t = ctx.theme;
    container(content)
        .padding(Padding::from([3.0, tokens::space::LG]))
        .width(Length::Fill)
        .style(move |_: &iced::Theme| {
            let c = &t.colors;
            iced::widget::container::Style {
                background: Some(if error { c.danger_soft } else { c.bg }.into()),
                text_color: Some(if error { c.danger } else { c.fg_muted }),
                border: Border {
                    color: if error { c.danger } else { c.line },
                    width: tokens::BORDER_WIDTH,
                    radius: tokens::radius::MD.into(),
                },
                ..iced::widget::container::Style::default()
            }
        })
        .into()
}

/// What a choice of the selects is called, and what it picks.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Choice {
    id: Option<String>,
    label: String,
}

impl fmt::Display for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// What sending the form says: for a folder, with the kits picked in the
/// section (P14 holds them; `update` cannot read them, so they ride along).
fn submit_message(st: &State, ctx: Ctx<'_>) -> Message {
    let kits = match st.source {
        Source::Folder => crate::kits::draft_kits(&ctx.pkg.kits).to_vec(),
        Source::Git => Vec::new(),
    };
    msg(Msg::Submit(kits))
}

// ── the dialog ────────────────────────────────────────────────────────

pub fn dialog<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    let status = st.git_status(ctx.model);
    let running = st.git.running;

    let mut form = column![].spacing(14).width(Length::Fill);
    form = form.push(labelled(
        ctx,
        Some(ctx.t(Key::CreateSource).into_owned()),
        input::segmented(
            t,
            &[
                (Source::Folder, word(ctx.t(Key::CreateSourceFolder))),
                (Source::Git, word(ctx.t(Key::CreateSourceGit))),
            ],
            &st.source,
            |source| msg(Msg::Source(source)),
        ),
    ));

    match st.source {
        Source::Git => form = git_fields(form, st, ctx, status),
        Source::Folder => form = folder_fields(form, st, ctx),
    }

    // The name is both modes'.
    let placeholder = match st.source {
        Source::Git => match st.git.parse() {
            Some(ParseResult::Ok { repo }) => repo.name.clone(),
            _ => ctx.t(Key::CreateNamePlaceholder).into_owned(),
        },
        Source::Folder if st.folder.cwd.is_empty() => {
            ctx.t(Key::CreateNamePlaceholder).into_owned()
        }
        Source::Folder => folder::basename(&st.folder.cwd),
    };
    let mut name = field(ctx, &placeholder, &st.name, false);
    if !git_form::fields_disabled(status) {
        name = name
            .on_input(|name| msg(Msg::Name(name)))
            .on_submit(submit_message(st, ctx));
    }
    form = form.push(labelled(
        ctx,
        Some(ctx.t(Key::CreateName).into_owned()),
        crate::keymap::guard(ctx, name),
    ));

    if st.source == Source::Git && (running || st.git.error.is_some()) {
        form = form.push(labelled(ctx, None, progress_or_error(st, ctx)));
    }
    if st.source == Source::Folder {
        form = folder_agents(form, st, ctx);
    }

    // The footer.
    let stop_or_cancel = if running {
        ui_button::ghost(t, ctx.t(Key::CreateGitStop)).on_press(msg(Msg::Stop))
    } else {
        ui_button::ghost(t, ctx.t(Key::CreateCancel)).on_press(msg(Msg::Cancel))
    };
    let submit = match st.source {
        Source::Git => {
            let (label, disabled) = git_form::from_git_button(status);
            ui_button::primary(t, ctx.t(label))
                .on_press_maybe((!disabled).then(|| submit_message(st, ctx)))
        }
        Source::Folder => ui_button::primary(t, ctx.t(Key::CreateSubmit))
            .on_press_maybe((!st.folder.cwd.is_empty()).then(|| submit_message(st, ctx))),
    };

    let panel = dialog::dialog(t, ctx.t(Key::CreateTitle), form)
        .width(640.0)
        .body_max_height(BODY_HEIGHT)
        .body_padding(Padding::from([18.0, 22.0]))
        .close_tip(ctx.t(Key::ModalClose))
        .action(stop_or_cancel)
        .action(submit)
        .on_close(msg(Msg::Dismiss));
    modal(ctx, panel)
}

/// Repository, destination.
fn git_fields<'a>(
    mut form: iced::widget::Column<'a, Message>,
    st: &'a State,
    ctx: Ctx<'a>,
    status: FromGitStatus,
) -> iced::widget::Column<'a, Message> {
    let t = ctx.theme;
    let locked = git_form::fields_disabled(status);

    let placeholder = ctx.t(Key::CreateGitRepoPlaceholder);
    let mut repo = field(
        ctx,
        &placeholder,
        &st.git.repo,
        status == FromGitStatus::Refused,
    )
    .id(super::repo_id());
    if !locked {
        repo = repo
            .on_input(|repo| msg(Msg::Repo(repo)))
            .on_submit(submit_message(st, ctx));
    }
    let mut repo_column = column![crate::keymap::guard(ctx, repo)].spacing(tokens::space::SM);
    match (status, st.git.parse()) {
        (FromGitStatus::Refused, Some(ParseResult::Refused { reason })) => {
            repo_column = repo_column.push(typography::hint(
                t,
                ctx.i18n
                    .t_with(Key::CreateGitRefused, &args!("reason" => reason.as_str()))
                    .into_owned(),
            ));
        }
        (FromGitStatus::Checking, _) => {
            repo_column = repo_column.push(typography::hint(t, ctx.t(Key::CreateGitChecking)));
        }
        _ => {}
    }
    form = form.push(labelled(
        ctx,
        Some(ctx.t(Key::CreateGitRepo).into_owned()),
        repo_column,
    ));

    let parent = st.parent_for(ctx.model);
    let empty = ctx.t(Key::CreateFolderEmpty);
    let mut path = field(
        ctx,
        parent.as_deref().unwrap_or(&empty),
        &st.git.parent,
        false,
    );
    if !locked {
        // Return anywhere in the form sends it, as the `<form>` of the web
        // app did.
        path = path
            .on_input(|parent| msg(Msg::Parent(parent)))
            .on_submit(submit_message(st, ctx));
    }
    let browse = ui_button::secondary(t, ctx.t(Key::CreateBrowse))
        .on_press_maybe((!locked).then(|| msg(Msg::BrowseParent)));
    let mut parent_column = column![
        picker(ctx, path, vec![browse.into()]),
        typography::hint(t, ctx.t(Key::CreateGitParentHint)),
    ]
    .spacing(tokens::space::SM);
    if status == FromGitStatus::NoFolder {
        parent_column = parent_column.push(typography::hint(t, ctx.t(Key::CreateGitNoFolder)));
    } else if let Some(ParseResult::Ok { repo }) = st.git.parse()
        && let Some(destination) = clone_destination(parent.as_deref(), Some(&repo.name))
    {
        parent_column = parent_column.push(typography::hint(
            t,
            ctx.i18n
                .t_with(
                    Key::CreateGitDestination,
                    &args!("path" => destination.as_str()),
                )
                .into_owned(),
        ));
    }
    form.push(labelled(
        ctx,
        Some(ctx.t(Key::CreateGitParent).into_owned()),
        parent_column,
    ))
}

/// The progress of a clone, or why it failed.
fn progress_or_error<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    if st.git.running {
        let percent = st
            .git
            .progress
            .as_ref()
            .and_then(|progress| overall_percent(&progress.phase, progress.percent));
        let label = match &st.git.progress {
            Some(progress) => progress_label(&progress.phase, progress.percent),
            None => ctx.t(Key::CreateGitStarting).into_owned(),
        };
        // A percentage the bar can use, else a segment that slides: a bar
        // stuck at zero would read as a clone that does not move.
        let bar: Element<'a, Message> = match percent {
            Some(percent) => progress_bar(0.0..=100.0, f32::from(percent))
                .girth(6)
                .style(style::progress_bar::accent(t))
                .into(),
            None => sweep::bar(t, 6.0, st.git.sweep_elapsed()),
        };
        return column![bar, typography::hint(t, label)]
            .spacing(tokens::space::SM)
            .into();
    }
    match &st.git.error {
        Some(error) => notice(
            ctx,
            text(error.as_str()).size(tokens::font_size::UI).into(),
            true,
        ),
        None => Space::new().into(),
    }
}

/// Folder, workspace file, its roots.
fn folder_fields<'a>(
    form: iced::widget::Column<'a, Message>,
    st: &'a State,
    ctx: Ctx<'a>,
) -> iced::widget::Column<'a, Message> {
    let t = ctx.theme;
    let empty = ctx.t(Key::CreateFolderEmpty);
    let cwd = field(ctx, &empty, &st.folder.cwd, false)
        .on_input(|cwd| msg(Msg::Cwd(cwd)))
        .on_submit(submit_message(st, ctx));
    let mut folder_column = column![picker(
        ctx,
        cwd,
        vec![
            ui_button::secondary(t, ctx.t(Key::CreateBrowse))
                .on_press(msg(Msg::Browse))
                .into(),
            ui_button::secondary(t, ctx.t(Key::CreateOpenFile))
                .tip(ctx.t(Key::CreateFileDialog))
                .on_press(msg(Msg::BrowseFile))
                .into(),
        ],
    )]
    .spacing(tokens::space::SM);

    if let Some(suggestion) = &st.folder.suggestion {
        let found = ctx
            .i18n
            .t_with(
                Key::CreateFileFound,
                &args!("name" => folder::basename(suggestion)),
            )
            .into_owned();
        folder_column = folder_column.push(notice(
            ctx,
            row![
                text(found).size(tokens::font_size::UI).width(Length::Fill),
                ui_button::secondary(t, ctx.t(Key::CreateFileUse))
                    .small()
                    .on_press(msg(Msg::UseSuggestion)),
                ui_button::icon_button(t, Icon::Close)
                    .small()
                    .tip(ctx.t(Key::ModalClose))
                    .on_press(msg(Msg::DismissSuggestion)),
            ]
            .spacing(tokens::space::MD)
            .align_y(Alignment::Center)
            .into(),
            false,
        ));
    }
    if let Some(failed) = &st.folder.file_error {
        let said = ctx
            .i18n
            .t_with(
                Key::CreateFileFailed,
                &args!("name" => folder::basename(failed)),
            )
            .into_owned();
        folder_column = folder_column.push(notice(
            ctx,
            text(said).size(tokens::font_size::UI).into(),
            true,
        ));
    }
    if !st.folder.roots.is_empty() {
        folder_column = folder_column
            .push(roots(st, ctx))
            .push(typography::hint(t, ctx.t(Key::CreateRootsHint)));
    }
    form.push(labelled(
        ctx,
        Some(ctx.t(Key::CreateFolder).into_owned()),
        folder_column,
    ))
}

/// The roots of a workspace file, one per line; the main one is selected.
fn roots<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    let lines = st.folder.roots.iter().map(|root: &'a Root| {
        let is_main = root.path == st.folder.cwd;
        let tag = if !root.exists {
            Key::CreateRootMissing
        } else if is_main {
            Key::CreateRootMain
        } else {
            Key::CreateRootExtra
        };
        let content = row![
            text(root.name.as_str())
                .size(tokens::font_size::UI_LG)
                .font(hangar_theme::fonts::ui_medium()),
            typography::mono(t, root.path.as_str())
                .size(tokens::font_size::CAPTION)
                .wrapping(Wrapping::None)
                .width(Length::Fill),
            text(ctx.t(tag)).size(tokens::font_size::CAPTION),
        ]
        .spacing(tokens::space::LG)
        .align_y(Alignment::Center);
        let line = button(content)
            .width(Length::Fill)
            .padding(Padding::from([4.0, tokens::space::LG]))
            .style(style::button::row(t, is_main))
            .on_press_maybe(root.exists.then(|| msg(Msg::PickRoot(root.path.clone()))));
        Element::from(line)
    });
    container(column(lines))
        .width(Length::Fill)
        .style(style::container::well(t))
        .into()
}

/// Shell, panes, agents, arrangement, account, kits, hooks.
fn folder_agents<'a>(
    mut form: iced::widget::Column<'a, Message>,
    st: &'a State,
    ctx: Ctx<'a>,
) -> iced::widget::Column<'a, Message> {
    let t = ctx.theme;
    let locale = ctx.i18n;

    // The shell.
    let shells = &ctx.model.ui.shells;
    let none = locale.t(Key::CreateShellNone);
    let default = Choice {
        id: None,
        label: locale
            .t_with(
                Key::CreateShellDefault,
                &args!("name" => shells.first().map_or(none.as_ref(), |shell| shell.label.as_str())),
            )
            .into_owned(),
    };
    let mut options = vec![default.clone()];
    options.extend(shells.iter().map(|shell| Choice {
        id: Some(shell.id.clone()),
        label: shell.label.clone(),
    }));
    let selected = st
        .folder
        .shell_id
        .as_deref()
        .and_then(|id| {
            options
                .iter()
                .find(|option| option.id.as_deref() == Some(id))
                .cloned()
        })
        .or(Some(default));
    let detected = locale
        .tn(Key::CreateShellDetected, shells.len(), &[])
        .into_owned();
    form = form.push(labelled(
        ctx,
        Some(ctx.t(Key::PaneShell).into_owned()),
        row![
            container(input::select(t, options, selected, |choice: Choice| {
                msg(Msg::Shell(choice.id))
            }))
            .width(Length::Fixed(260.0)),
            typography::hint(t, detected),
        ]
        .spacing(tokens::space::XL)
        .align_y(Alignment::Center),
    ));

    // The number of panes.
    let layouts: Vec<(u8, &'static str)> = LAYOUTS
        .iter()
        .map(|layout| {
            (
                *layout,
                match layout {
                    1 => "1",
                    2 => "2",
                    4 => "4",
                    _ => "8",
                },
            )
        })
        .collect();
    form = form.push(labelled(
        ctx,
        Some(ctx.t(Key::CreateTerminals).into_owned()),
        row![input::segmented(t, &layouts, &st.folder.layout, |layout| {
            msg(Msg::Layout(layout))
        })],
    ));

    // The agents.
    let free = folder::count_of(&st.folder.assignments, AgentId::Shell);
    let counters: Vec<Element<'a, Message>> = AGENTS
        .iter()
        .filter(|agent| agent.id != AgentId::Shell)
        .map(|agent| counter(st, ctx, agent.id, agent.label, free))
        .collect();
    let mut counter_rows = column![].width(Length::Fill);
    let mut pending: Vec<Element<'a, Message>> = counters;
    while !pending.is_empty() {
        let rest = pending.split_off(pending.len().min(2));
        let mut line = row![].width(Length::Fill);
        for one in pending {
            line = line.push(container(one).width(Length::FillPortion(1)));
        }
        counter_rows = counter_rows.push(line);
        pending = rest;
    }
    let free_text = if free > 0 {
        locale.tn(Key::CreateFreeSlots, free, &[]).into_owned()
    } else {
        ctx.t(Key::CreateAllAssigned).into_owned()
    };
    form = form.push(labelled(
        ctx,
        Some(ctx.t(Key::CreateAgents).into_owned()),
        column![
            container(counter_rows)
                .width(Length::Fill)
                .style(style::container::well(t)),
            typography::hint(t, free_text),
        ]
        .spacing(tokens::space::SM),
    ));

    // The arrangement.
    form = form.push(labelled(
        ctx,
        Some(ctx.t(Key::CreateLayout).into_owned()),
        column![
            preview(st, ctx),
            typography::hint(t, ctx.t(Key::CreateLayoutHint)),
        ]
        .spacing(tokens::space::SM),
    ));

    // The Claude account, when there are accounts to choose from.
    if accounts::has_any(ctx) {
        form = form.push(labelled(
            ctx,
            Some(ctx.t(Key::AccountsPickerLabel).into_owned()),
            accounts::workspace_picker(ctx, st.folder.claude_account.as_deref()),
        ));
    }

    // The kits (P14's section) and the worktree hooks.
    form = form.push(labelled(
        ctx,
        Some(ctx.t(Key::KitsManageSectionTitle).into_owned()),
        crate::kits::workspace_section(&ctx.pkg.kits, ctx, None),
    ));
    form.push(labelled(
        ctx,
        Some(ctx.t(Key::WorktreesHooksTitle).into_owned()),
        hooks::view(
            ctx,
            &st.folder.hooks,
            Some(submit_message(st, ctx)),
            |field, text| msg(Msg::Hook(field, text)),
        ),
    ))
}

/// One agent's line of the counters: its name, « not installed » when it is
/// missing, and a stepper.
fn counter<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    agent: AgentId,
    label: &'static str,
    free: usize,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let missing = agent == AgentId::Claude && !ctx.model.ui.claude_installed;
    let count = folder::count_of(&st.folder.assignments, agent);
    let mut name = column![text(label).size(tokens::font_size::BODY)];
    if missing {
        name = name.push(
            text(ctx.t(Key::CreateNotInstalled))
                .size(tokens::font_size::CAPTION)
                .style(style::text::warning(t)),
        );
    }
    let less = ctx
        .i18n
        .t_with(Key::CreateLess, &args!("agent" => label))
        .into_owned();
    let more = ctx
        .i18n
        .t_with(Key::CreateMore, &args!("agent" => label))
        .into_owned();
    let stepper = container(
        row![
            ui_button::icon_button(t, Icon::Minus)
                .small()
                .tip(less)
                .on_press_maybe((count > 0).then(|| msg(Msg::Less(agent)))),
            container(
                typography::mono(t, count.to_string())
                    .size(tokens::font_size::UI)
                    .align_x(iced::alignment::Horizontal::Center)
            )
            .width(Length::Fixed(20.0)),
            ui_button::icon_button(t, Icon::Plus)
                .small()
                .tip(more)
                .on_press_maybe((free > 0).then(|| msg(Msg::More(agent)))),
        ]
        .align_y(Alignment::Center),
    )
    .style(style::container::segmented(t));
    container(
        row![name.width(Length::Fill), stepper]
            .spacing(tokens::space::LG)
            .align_y(Alignment::Center),
    )
    .padding(Padding::from([4.0, 8.0]))
    .into()
}

/// The grid the workspace will open with, drawn the way the window draws it:
/// panes on a separator, each with its header — first name, then agent. A
/// click cycles that pane's agent.
fn preview<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    let columns = folder::preview_columns(st.folder.layout);
    // Pane names are unique across workspaces: these are the next free ones,
    // the same the store will hand out.
    let names = crate::state::pane_names::pick_pane_names(
        st.folder.assignments.len(),
        crate::state::pane_names::all_pane_names(&ctx.model.state().workspaces),
    );
    let cells: Vec<Element<'a, Message>> = st
        .folder
        .assignments
        .iter()
        .enumerate()
        .map(|(index, agent)| {
            let is_shell = *agent == AgentId::Shell;
            let name = names
                .get(index)
                .cloned()
                .unwrap_or_else(|| (index + 1).to_string());
            let head = container(
                row![
                    text(name)
                        .size(tokens::font_size::CAPTION_LG)
                        .font(if is_shell {
                            hangar_theme::fonts::ui_medium()
                        } else {
                            hangar_theme::fonts::ui_semibold()
                        })
                        .style(move |theme: &iced::Theme| {
                            if is_shell {
                                style::text::muted(t)(theme)
                            } else {
                                style::text::primary(t)(theme)
                            }
                        }),
                    text(agent.info().label)
                        .size(tokens::font_size::CAPTION)
                        .wrapping(Wrapping::None)
                        .style(style::text::secondary(t)),
                ]
                .spacing(tokens::space::SM)
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([2.0, 7.0]))
            .width(Length::Fill)
            .style(style::container::pane_head(t, false));
            let cell = button(column![head, Space::new().height(Length::Fill)])
                .padding(0)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_: &iced::Theme, status| {
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: Some(t.colors.bg.into()),
                        text_color: t.colors.fg,
                        border: Border {
                            color: if hovered {
                                t.colors.accent
                            } else {
                                iced::Color::TRANSPARENT
                            },
                            width: if hovered { tokens::BORDER_WIDTH } else { 0.0 },
                            radius: 0.0.into(),
                        },
                        ..button::Style::default()
                    }
                })
                .on_press(msg(Msg::Cycle(index)));
            ui_tip(t, cell.into(), agent.info().label)
        })
        .collect();

    let mut grid = column![]
        .spacing(1)
        .width(Length::Fill)
        .height(Length::Fill);
    let mut pending = cells;
    while !pending.is_empty() {
        let rest = pending.split_off(pending.len().min(columns));
        let mut line = row![].spacing(1).height(Length::Fill);
        for cell in pending {
            line = line.push(
                container(cell)
                    .width(Length::FillPortion(1))
                    .height(Length::Fill),
            );
        }
        grid = grid.push(line);
        pending = rest;
    }
    container(grid)
        .width(Length::Fill)
        .height(Length::Fixed(PREVIEW_HEIGHT))
        .padding(1)
        .style(move |_: &iced::Theme| iced::widget::container::Style {
            background: Some(t.colors.sep.into()),
            border: Border {
                color: t.colors.line_strong,
                width: tokens::BORDER_WIDTH,
                radius: tokens::radius::LG.into(),
            },
            ..iced::widget::container::Style::default()
        })
        .into()
}

fn ui_tip<'a>(
    t: &'a hangar_theme::Theme,
    content: Element<'a, Message>,
    words: &'static str,
) -> Element<'a, Message> {
    hangar_ui::tip::over(t, content, words)
}
