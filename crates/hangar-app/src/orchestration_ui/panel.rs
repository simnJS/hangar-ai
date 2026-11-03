//! « Orchestration »: how a workspace's orchestrator works (`OrchestrationPanel.tsx`):
//! how many workers at once, how many tries a task gets, whether a delivery
//! lands by itself, what checks it, where it lands, and on which models.
//!
//! A form with a draft rather than switches saved as they flip, like the MCP
//! servers' panel: half of it is typed, and a check command saved one letter
//! at a time would be run as whatever it said when a delivery came in. What
//! the panel shows is what the workspace runs with, defaults filled in, each
//! default named beside its field.

use std::fmt;

use iced::widget::{column, container, row, space};
use iced::{Alignment, Element, Length, Padding};

use hangar_i18n::{Key, args};
use hangar_theme::{fonts, style, tokens};
use hangar_ui::input::{NumberField, NumberMsg};
use hangar_ui::{Icon, button, dialog, form, icon, input, text as typography};

use super::form::{
    DEFAULT_ORCHESTRATOR_MODEL, DEFAULT_REVIEWER_MODEL, DEFAULT_WORKER_MODEL, Draft, Field,
    MAX_ATTEMPTS, MAX_WORKERS, MODEL_SUGGESTIONS, Problem, draft_of, draft_problems,
    normalize_draft, same_settings, stored_orchestration,
};
use super::{Msg, State};
use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::shell::Notice;
use crate::state::roles::{
    ORCHESTRATION_PERMISSION_MODES, orchestration_defaults, orchestration_of, orchestrator_of,
    workers_of,
};
use crate::state::types::WsId;
use crate::state::{Model, Patch, StoreAction, Workspace};

/// One of the three model fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelField {
    Orchestrator,
    Worker,
    Reviewer,
}

/// The panel's messages.
#[derive(Debug, Clone)]
pub enum PanelMsg {
    Workers(NumberMsg),
    Attempts(NumberMsg),
    /// A mode of `ORCHESTRATION_PERMISSION_MODES`, or empty for the person's own.
    Permission(String),
    AutoMerge(bool),
    Check(String),
    Branch(String),
    Model(ModelField, String),
    /// Every field back to the value Hangar ships with.
    Reset,
    Save,
    /// "New orchestrator": closes the panel, then makes one.
    CreateOrchestrator,
}

/// The panel while it is open: the draft, and the number fields it keeps
/// half-typed text in. Made again each time the panel opens, so what was
/// abandoned is gone, as the keyed React component's was.
#[derive(Debug)]
pub struct Panel {
    pub ws: WsId,
    draft: Draft,
    workers: NumberField,
    attempts: NumberField,
}

impl Panel {
    pub fn open(m: &Model, ws: &str) -> Option<Panel> {
        let ws = m.workspace(ws)?;
        let draft = draft_of(ws);
        Some(Panel {
            ws: ws.id.clone(),
            workers: count_field(draft.max_workers, MAX_WORKERS.min, MAX_WORKERS.max),
            attempts: count_field(draft.max_attempts, MAX_ATTEMPTS.min, MAX_ATTEMPTS.max),
            draft,
        })
    }
}

fn count_field(value: f64, min: u32, max: u32) -> NumberField {
    NumberField::new(value, f64::from(min), f64::from(max), 1.0)
}

fn model_field(draft: &mut Draft, which: ModelField) -> &mut String {
    match which {
        ModelField::Orchestrator => &mut draft.orchestrator_model,
        ModelField::Worker => &mut draft.worker_model,
        ModelField::Reviewer => &mut draft.reviewer_model,
    }
}

/// What the panel's messages do. `Save` writes the workspace and says so.
pub fn update(st: &mut State, m: &mut Model, msg: PanelMsg) -> iced::Task<Message> {
    let Some(panel) = st.panel.as_mut() else {
        return iced::Task::none();
    };
    match msg {
        PanelMsg::Workers(message) => {
            panel.workers.update(message);
            panel.draft.max_workers = panel.workers.value();
        }
        PanelMsg::Attempts(message) => {
            panel.attempts.update(message);
            panel.draft.max_attempts = panel.attempts.value();
        }
        PanelMsg::Permission(mode) => panel.draft.permission_mode = mode,
        PanelMsg::AutoMerge(on) => panel.draft.auto_merge = on,
        PanelMsg::Check(text) => panel.draft.check_command = text,
        PanelMsg::Branch(text) => panel.draft.branch = text,
        PanelMsg::Model(which, text) => *model_field(&mut panel.draft, which) = text,
        PanelMsg::Reset => {
            panel.draft = draft_of(&Workspace::default());
            panel.workers.set(panel.draft.max_workers);
            panel.attempts.set(panel.draft.max_attempts);
        }
        PanelMsg::Save => return save(panel, m),
        PanelMsg::CreateOrchestrator => {
            let ws = panel.ws.clone();
            return iced::Task::batch([
                iced::Task::done(Message::Intent(Intent::CloseOverlay)),
                iced::Task::done(Message::Intent(Intent::CreateOrchestrator(ws))),
            ]);
        }
    }
    iced::Task::none()
}

/// Saves the draft as the workspace's settings, unless a field is refused or
/// nothing changed (the button is disabled then, and Enter in a field comes
/// here too).
fn save(panel: &Panel, m: &mut Model) -> iced::Task<Message> {
    let Some(ws) = m.workspace(&panel.ws) else {
        return iced::Task::none();
    };
    let saved = orchestration_of(ws);
    let next = normalize_draft(&panel.draft);
    if !draft_problems(&panel.draft).is_empty() || same_settings(&next, &saved) {
        return iced::Task::none();
    }
    let name = ws.name.clone();
    let stored = stored_orchestration(&next);
    m.apply(StoreAction::UpdateWorkspace(
        panel.ws.clone(),
        Patch::new(move |ws: &mut Workspace| ws.orchestration = stored.clone()),
    ));
    let text = hangar_i18n::t_with(
        m.locale(),
        Key::OrchSaved,
        &args!("workspace" => name.as_str()),
    );
    iced::Task::batch([
        iced::Task::done(Message::Intent(Intent::Notice(Notice::Agent(
            text.into_owned(),
        )))),
        iced::Task::done(Message::Intent(Intent::CloseOverlay)),
    ])
}

fn msg(message: PanelMsg) -> Message {
    Message::OrchestrationUi(Msg::Panel(message))
}

/// An option of the permission list.
#[derive(Debug, Clone, PartialEq)]
struct Mode {
    id: &'static str,
    label: String,
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// `Overlay::Orchestration`.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>, ws_id: &'a WsId) -> Element<'a, Message> {
    let t = ctx.theme;
    let (Some(panel), Some(ws)) = (
        st.panel.as_ref().filter(|panel| &panel.ws == ws_id),
        ctx.model.workspace(ws_id),
    ) else {
        return space().into();
    };
    let draft = &panel.draft;
    let problems = draft_problems(draft);
    let next = normalize_draft(draft);
    let blocked = !problems.is_empty();
    let changed = !same_settings(&next, &orchestration_of(ws));
    let at_defaults = same_settings(&next, &orchestration_defaults());

    let body = column![
        typography::label(t, ctx.t(Key::OrchIntro)),
        status(ctx, ws),
        form::section(t, ctx.t(Key::OrchSectionWorkers)).group(vec![
            count_row(
                ctx,
                Key::OrchMaxWorkers,
                Key::OrchMaxWorkersHint,
                &panel.workers,
                orchestration_defaults().max_workers,
                PanelMsg::Workers,
            ),
            count_row(
                ctx,
                Key::OrchMaxAttempts,
                Key::OrchMaxAttemptsHint,
                &panel.attempts,
                orchestration_defaults().max_attempts,
                PanelMsg::Attempts,
            ),
            permission_row(ctx, draft),
        ]),
        form::section(t, ctx.t(Key::OrchSectionDeliveries)).group(vec![
            form::row(
                t,
                ctx.t(Key::OrchAutoMerge),
                input::switch(t, draft.auto_merge).on_toggle(|on| msg(PanelMsg::AutoMerge(on)))
            )
            .hint(ctx.t(Key::OrchAutoMergeHint))
            .into(),
            text_row(
                ctx,
                Key::OrchCheckCommand,
                ctx.t(Key::OrchCheckHint),
                &draft.check_command,
                label(ctx, Key::OrchCheckPlaceholder),
                None,
                PanelMsg::Check,
            ),
            text_row(
                ctx,
                Key::OrchBranch,
                ctx.t(Key::OrchBranchHint),
                &draft.branch,
                label(ctx, Key::OrchBranchPlaceholder),
                (problems.get(Field::Branch) == Some(Problem::Branch))
                    .then(|| ctx.t(Key::OrchBranchInvalid).into_owned()),
                PanelMsg::Branch,
            ),
        ]),
        form::section(t, ctx.t(Key::OrchSectionModels))
            .hint(ctx.t(Key::OrchModelsHint))
            .group(vec![
                model_row(ctx, &problems, draft, ModelField::Orchestrator),
                model_row(ctx, &problems, draft, ModelField::Worker),
                model_row(ctx, &problems, draft, ModelField::Reviewer),
            ]),
    ]
    .spacing(tokens::space::XXL);

    let sub = format!("{} · {}", ws.name, ws.cwd);
    let reset = button::ghost(t, ctx.t(Key::OrchReset))
        .tip(ctx.t(Key::OrchResetHint))
        .on_press_maybe((!at_defaults).then(|| msg(PanelMsg::Reset)));
    let cancel =
        button::ghost(t, ctx.t(Key::CreateCancel)).on_press(Message::Intent(Intent::CloseOverlay));
    let save = button::primary(t, ctx.t(Key::OrchSave))
        .on_press_maybe((!blocked && changed).then(|| msg(PanelMsg::Save)));

    let shown = dialog::dialog(t, ctx.t(Key::OrchTitle), body)
        .subtitle(sub)
        .note(ctx.t(Key::OrchApplies))
        .width(640.0)
        .body_max_height(480.0)
        .body_padding(Padding::from([tokens::space::XL, tokens::space::XXL]))
        .close_tip(ctx.t(Key::McpClose))
        .on_close(Message::Intent(Intent::CloseOverlay))
        .action(reset)
        .action(cancel)
        .action(save);
    dialog::over(
        t,
        space().width(Length::Fill).height(Length::Fill),
        Some(shown),
    )
    .into()
}

/// A label without arguments, for the widgets that keep a `&str` for the
/// whole frame. The dictionary lends such a label as a `'static` borrow
/// (hangar-i18n, `t`), so nothing is allocated.
fn label(ctx: Ctx<'_>, key: Key) -> &'static str {
    match ctx.t(key) {
        std::borrow::Cow::Borrowed(text) => text,
        std::borrow::Cow::Owned(text) => Box::leak(text.into_boxed_str()),
    }
}

/// "Pinned: claude-1", "2 workers running", or "No orchestrator" with the way
/// to make one.
fn status<'a>(ctx: Ctx<'a>, ws: &'a Workspace) -> Element<'a, Message> {
    let t = ctx.theme;
    let orchestrator = orchestrator_of(ws);
    let workers = workers_of(ws).len();
    let tint = if orchestrator.is_some() {
        t.colors.accent_hi
    } else {
        t.colors.fg_muted
    };

    let words: Element<'a, Message> = match orchestrator {
        Some(pane) => column![
            typography::body(
                t,
                ctx.i18n
                    .t_with(Key::OrchStatusPinned, &args!("name" => pane.name.as_str()))
            )
            .font(fonts::ui_semibold()),
            typography::hint(
                t,
                if workers > 0 {
                    ctx.i18n.tn(Key::OrchStatusWorkers, workers, &[])
                } else {
                    ctx.t(Key::OrchStatusNoWorker)
                }
            ),
        ]
        .spacing(2)
        .into(),
        None => column![
            typography::body(t, ctx.t(Key::OrchStatusNone)).font(fonts::ui_semibold()),
            typography::hint(t, ctx.t(Key::OrchStatusNoneHint)),
        ]
        .spacing(2)
        .into(),
    };

    let mut line = row![
        icon(Icon::Orchestrator, 16.0).color(tint),
        container(words).width(Length::Fill),
    ]
    .spacing(tokens::space::XL)
    .align_y(Alignment::Center);
    if orchestrator.is_none() {
        line = line.push(
            button::secondary(t, ctx.t(Key::CmdWorkspaceNewOrchestrator))
                .icon(Icon::Plus)
                .small()
                .on_press(msg(PanelMsg::CreateOrchestrator)),
        );
    }
    container(line)
        .padding(Padding::from([tokens::space::XL, tokens::space::XXL]))
        .width(Length::Fill)
        .style(style::container::card(t))
        .into()
}

/// A stepper row: the number field, and the default beside it.
fn count_row<'a>(
    ctx: Ctx<'a>,
    label_key: Key,
    hint_key: Key,
    field: &'a NumberField,
    default: u32,
    on_message: fn(NumberMsg) -> PanelMsg,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let control = row![
        container(input::number_input(t, field, move |message| msg(
            on_message(message)
        )))
        .width(130.0),
        typography::hint(
            t,
            ctx.i18n
                .t_with(Key::OrchDefault, &args!("value" => default))
        ),
    ]
    .spacing(tokens::space::LG)
    .align_y(Alignment::Center);
    form::row(t, ctx.t(label_key), control)
        .hint(ctx.t(hint_key))
        .into()
}

fn permission_row<'a>(ctx: Ctx<'a>, draft: &'a Draft) -> Element<'a, Message> {
    let t = ctx.theme;
    let mut options = vec![Mode {
        id: "",
        label: ctx.t(Key::OrchPermissionTheirs).into_owned(),
    }];
    for mode in ORCHESTRATION_PERMISSION_MODES {
        options.push(Mode {
            id: mode,
            label: ctx
                .t(match mode {
                    "auto" => Key::OrchPermissionAuto,
                    "acceptEdits" => Key::OrchPermissionAcceptEdits,
                    _ => Key::OrchPermissionBypassPermissions,
                })
                .into_owned(),
        });
    }
    let chosen = options
        .iter()
        .find(|option| option.id == draft.permission_mode)
        .cloned();
    let select = input::select(t, options, chosen, |mode: Mode| {
        msg(PanelMsg::Permission(mode.id.to_owned()))
    });
    form::row(
        t,
        ctx.t(Key::OrchPermissionMode),
        container(select).width(300.0),
    )
    .hint(ctx.t(Key::OrchPermissionHint))
    .into()
}

/// A text row: the check command, the branch.
fn text_row<'a>(
    ctx: Ctx<'a>,
    label_key: Key,
    hint: impl Into<String>,
    value: &'a str,
    placeholder: &'static str,
    error: Option<String>,
    on_input: fn(String) -> PanelMsg,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let field = input::text_field(t, value)
        .placeholder(placeholder)
        .mono()
        .error(error)
        .on_input(move |text| msg(on_input(text)))
        .on_submit(msg(PanelMsg::Save));
    form::row(t, ctx.t(label_key), container(field).width(300.0))
        .hint(hint)
        .into()
}

/// A model row: the name, the default beside it, and the names Claude Code
/// answers to, one press away (the `datalist` of the web panel).
fn model_row<'a>(
    ctx: Ctx<'a>,
    problems: &super::form::Problems,
    draft: &'a Draft,
    which: ModelField,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let (label_key, value, default, field) = match which {
        ModelField::Orchestrator => (
            Key::OrchOrchestratorModel,
            &draft.orchestrator_model,
            DEFAULT_ORCHESTRATOR_MODEL,
            Field::OrchestratorModel,
        ),
        ModelField::Worker => (
            Key::OrchWorkerModel,
            &draft.worker_model,
            DEFAULT_WORKER_MODEL,
            Field::WorkerModel,
        ),
        ModelField::Reviewer => (
            Key::OrchReviewerModel,
            &draft.reviewer_model,
            DEFAULT_REVIEWER_MODEL,
            Field::ReviewerModel,
        ),
    };
    let error = (problems.get(field) == Some(Problem::Model))
        .then(|| ctx.t(Key::OrchModelInvalid).into_owned());
    let input = input::text_field(t, value)
        .placeholder(default)
        .mono()
        .error(error)
        .on_input(move |text| msg(PanelMsg::Model(which, text)))
        .on_submit(msg(PanelMsg::Save));
    let mut suggestions = row![].spacing(tokens::space::LG);
    for name in MODEL_SUGGESTIONS {
        suggestions = suggestions.push(button::link(
            t,
            name,
            msg(PanelMsg::Model(which, name.to_owned())),
        ));
    }
    let control = column![container(input).width(300.0), suggestions].spacing(tokens::space::SM);
    form::row(t, ctx.t(label_key), control)
        .hint(
            ctx.i18n
                .t_with(Key::OrchDefault, &args!("value" => default)),
        )
        .into()
}
