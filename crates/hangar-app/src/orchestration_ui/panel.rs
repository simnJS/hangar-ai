//! « Orchestration »: how a workspace's orchestrator works (`OrchestrationPanel.tsx`):
//! what its agents may do without asking, whether a delivery lands by
//! itself, what checks it, where it lands, and on which models. How many
//! workers run at once and how many tries a task gets are not here: the
//! orchestrator decides both, and Hangar sets no limit on either.
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
use hangar_ui::{Icon, button, dialog, form, icon, input, text as typography};

use super::form::{
    DEFAULT_ORCHESTRATOR_MODEL, DEFAULT_REVIEWER_MODEL, DEFAULT_WORKER_MODEL, Draft, Field,
    MODEL_SUGGESTIONS, Problem, draft_of, draft_problems, normalize_draft, same_settings,
    stored_orchestration,
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
    /// A mode of `ORCHESTRATION_PERMISSION_MODES`, or empty for the person's own.
    Permission(String),
    AutoMerge(bool),
    /// The "Shared build cache" switch (saved with the rest, by Save).
    BuildCache(bool),
    /// "Empty": asks first, and is refused while a worker of the workspace runs.
    EmptyCache,
    /// The confirmation said no.
    EmptyCancel,
    /// The confirmation said yes.
    EmptyConfirm,
    Check(String),
    Branch(String),
    Model(ModelField, String),
    /// Every field back to the value Hangar ships with.
    Reset,
    Save,
    /// "New orchestrator": closes the panel, then makes one.
    CreateOrchestrator,
}

/// The panel while it is open: the draft. Made again each time the panel
/// opens, so what was abandoned is gone, as the keyed React component's was.
#[derive(Debug)]
pub struct Panel {
    pub ws: WsId,
    draft: Draft,
    cache: Cache,
}

/// The workspace's build cache folder as the panel knows it: its size once
/// measured (off the UI thread), and whether "Empty" is waiting for a yes or
/// is deleting.
#[derive(Debug, Default)]
struct Cache {
    size: Option<u64>,
    asking: bool,
    emptying: bool,
}

impl Panel {
    pub fn open(m: &Model, ws: &str) -> Option<Panel> {
        let ws = m.workspace(ws)?;
        Some(Panel {
            ws: ws.id.clone(),
            draft: draft_of(ws),
            cache: Cache::default(),
        })
    }
}

/// Measures the workspace's cache folder on the blocking pool; the answer is
/// `Msg::CacheSize`.
pub fn measure(ws: &str) -> iced::Task<Message> {
    let ws = ws.to_owned();
    iced::Task::perform(
        async move {
            let dir = hangar_core::build_cache::dir_for(&ws);
            let size = tokio::task::spawn_blocking(move || hangar_core::build_cache::size_of(&dir))
                .await
                .unwrap_or(0);
            (ws, size)
        },
        |(ws, size)| Message::OrchestrationUi(Msg::CacheSize(ws, size)),
    )
}

/// The size came back.
pub fn cache_measured(st: &mut State, ws: &str, size: u64) {
    if let Some(panel) = st.panel.as_mut().filter(|panel| panel.ws == ws) {
        panel.cache.size = Some(size);
    }
}

/// The folder was emptied, or not: says so, and measures again.
pub fn cache_emptied(
    st: &mut State,
    m: &Model,
    ws: &str,
    result: Result<(), String>,
) -> iced::Task<Message> {
    if let Some(panel) = st.panel.as_mut().filter(|panel| panel.ws == ws) {
        panel.cache.emptying = false;
        panel.cache.asking = false;
    }
    let text = match result {
        Ok(()) => hangar_i18n::t(m.locale(), Key::OrchBuildCacheEmptied),
        Err(error) => hangar_i18n::t_with(
            m.locale(),
            Key::OrchBuildCacheEmptyFailed,
            &args!("error" => error.as_str()),
        ),
    };
    iced::Task::batch([
        iced::Task::done(Message::Intent(Intent::Notice(Notice::Agent(
            text.into_owned(),
        )))),
        measure(ws),
    ])
}

/// A worker of the workspace is running: its builds are in the cache.
fn refuse_while_busy(panel: &mut Panel, m: &Model) -> Option<iced::Task<Message>> {
    let ws = m.workspace(&panel.ws)?;
    if workers_of(ws).is_empty() {
        return None;
    }
    panel.cache.asking = false;
    let text = hangar_i18n::t_with(
        m.locale(),
        Key::OrchBuildCacheBusy,
        &args!("workspace" => ws.name.as_str()),
    );
    Some(iced::Task::done(Message::Intent(Intent::Notice(
        Notice::Problem(text.into_owned()),
    ))))
}

/// Deletes the folder on the blocking pool; the answer is `Msg::CacheEmptied`.
fn empty_cache(ws: &str) -> iced::Task<Message> {
    let ws = ws.to_owned();
    iced::Task::perform(
        async move {
            let dir = hangar_core::build_cache::dir_for(&ws);
            let result = tokio::task::spawn_blocking(move || hangar_core::build_cache::empty(&dir))
                .await
                .map_err(|error| error.to_string())
                .and_then(|done| done.map_err(|error| error.to_string()));
            (ws, result)
        },
        |(ws, result)| Message::OrchestrationUi(Msg::CacheEmptied(ws, result)),
    )
}

/// "412 MB", "1,5 Go".
fn size_text(locale: hangar_i18n::Locale, bytes: u64) -> String {
    let megabytes = bytes as f64 / (1024.0 * 1024.0);
    let french = locale == hangar_i18n::Locale::Fr;
    let (value, unit) = match (megabytes >= 1024.0, french) {
        (true, true) => (megabytes / 1024.0, "Go"),
        (true, false) => (megabytes / 1024.0, "GB"),
        (false, true) => (megabytes.round(), "Mo"),
        (false, false) => (megabytes.round(), "MB"),
    };
    let rounded = (value * 10.0).round() / 10.0;
    let number = if rounded.fract() == 0.0 {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.1}")
    };
    if french {
        format!("{} {unit}", number.replace('.', ","))
    } else {
        format!("{number} {unit}")
    }
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
        PanelMsg::Permission(mode) => panel.draft.permission_mode = mode,
        PanelMsg::AutoMerge(on) => panel.draft.auto_merge = on,
        PanelMsg::BuildCache(on) => panel.draft.shared_build_cache = on,
        PanelMsg::EmptyCache => {
            if let Some(refused) = refuse_while_busy(panel, m) {
                return refused;
            }
            panel.cache.asking = panel.cache.size != Some(0);
        }
        PanelMsg::EmptyCancel => panel.cache.asking = false,
        PanelMsg::EmptyConfirm => {
            if let Some(refused) = refuse_while_busy(panel, m) {
                return refused;
            }
            if !panel.cache.asking || panel.cache.emptying {
                return iced::Task::none();
            }
            panel.cache.asking = false;
            panel.cache.emptying = true;
            return empty_cache(&panel.ws);
        }
        PanelMsg::Check(text) => panel.draft.check_command = text,
        PanelMsg::Branch(text) => panel.draft.branch = text,
        PanelMsg::Model(which, text) => *model_field(&mut panel.draft, which) = text,
        PanelMsg::Reset => panel.draft = draft_of(&Workspace::default()),
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
            permission_row(ctx, draft),
            form::row(
                t,
                ctx.t(Key::OrchBuildCache),
                input::switch(t, draft.shared_build_cache)
                    .on_toggle(|on| msg(PanelMsg::BuildCache(on)))
            )
            .hint(ctx.t(Key::OrchBuildCacheHint))
            .into(),
            cache_row(ctx, &panel.cache),
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

/// The cache's size and what empties it: a button, which asks first.
fn cache_row<'a>(ctx: Ctx<'a>, cache: &Cache) -> Element<'a, Message> {
    let t = ctx.theme;
    let size = match cache.size {
        Some(bytes) => size_text(ctx.model.locale(), bytes),
        None => ctx.t(Key::OrchBuildCacheMeasuring).into_owned(),
    };
    let control: Element<'a, Message> = if cache.asking {
        row![
            typography::hint(t, ctx.t(Key::OrchBuildCacheEmptyAsk)),
            button::danger(t, ctx.t(Key::OrchBuildCacheEmptyConfirm))
                .small()
                .on_press(msg(PanelMsg::EmptyConfirm)),
            button::ghost(t, ctx.t(Key::CreateCancel))
                .small()
                .on_press(msg(PanelMsg::EmptyCancel)),
        ]
        .spacing(tokens::space::LG)
        .align_y(Alignment::Center)
        .into()
    } else {
        row![
            typography::body(t, size),
            button::secondary(t, ctx.t(Key::OrchBuildCacheEmpty))
                .small()
                .on_press_maybe(
                    (cache.size.is_some_and(|bytes| bytes > 0) && !cache.emptying)
                        .then(|| msg(PanelMsg::EmptyCache))
                ),
        ]
        .spacing(tokens::space::LG)
        .align_y(Alignment::Center)
        .into()
    };
    form::row(t, ctx.t(Key::OrchBuildCacheSize), control).into()
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

#[cfg(test)]
mod tests {
    //! The panel drawn as a window would lay it out.

    use super::*;
    use crate::app::Packages;
    use crate::navigator::testing::{ctx, model_of, simulate};
    use crate::state::types::AppState;

    /// Hangar sets no limit on the workers (owner's decision, 2026-11-04):
    /// the panel has no field for how many run at once or how many times a
    /// task is tried, and says the orchestrator decides.
    #[test]
    fn offers_no_limit_on_the_workers_and_says_the_orchestrator_decides() {
        let model = model_of(AppState {
            workspaces: vec![Workspace {
                id: "w1".into(),
                name: "Shop".into(),
                cwd: "/code/shop".into(),
                ..Workspace::default()
            }],
            ..AppState::default()
        });
        let st = State {
            panel: Panel::open(&model, "w1"),
            ..State::default()
        };
        let pkg = Packages::default();
        let ws: WsId = "w1".into();
        let mut ui = simulate(view(&st, ctx(&model, &pkg), &ws));
        for shown in [
            "Permissions",
            "Merge by itself",
            "Check command",
            "Target branch",
        ] {
            assert!(ui.find(shown).is_ok(), "{shown}");
        }
        for gone in ["Workers at once", "Attempts per task"] {
            assert!(ui.find(gone).is_err(), "{gone}");
        }
        let intro = "It decides how many workers run at once and how many times a task is tried.";
        assert!(
            ctx(&model, &pkg).t(Key::OrchIntro).contains(intro),
            "{intro}"
        );
    }

    fn shop(workers: bool) -> Model {
        let mut panes = vec![];
        if workers {
            panes.push(crate::state::types::Pane {
                id: "p1".into(),
                name: "Sam".into(),
                role: Some(crate::state::types::PaneRole::Worker),
                ..crate::state::types::Pane::default()
            });
        }
        model_of(AppState {
            workspaces: vec![Workspace {
                id: "w1".into(),
                name: "Shop".into(),
                cwd: "/code/shop".into(),
                panes,
                ..Workspace::default()
            }],
            ..AppState::default()
        })
    }

    fn opened(model: &Model) -> State {
        State {
            panel: Panel::open(model, "w1"),
            ..State::default()
        }
    }

    #[test]
    fn shows_the_build_cache_switch_its_size_and_empty() {
        let model = shop(false);
        let mut st = opened(&model);
        let pkg = Packages::default();
        let ws: WsId = "w1".into();
        {
            let mut ui = simulate(view(&st, ctx(&model, &pkg), &ws));
            assert!(ui.find("Shared build cache").is_ok());
            assert!(ui.find("Measuring…").is_ok());
        }
        cache_measured(&mut st, "w1", 3 * 1024 * 1024 * 1024);
        let mut ui = simulate(view(&st, ctx(&model, &pkg), &ws));
        assert!(ui.find("3 GB").is_ok());
        assert!(ui.find("Empty").is_ok());
    }

    #[test]
    fn the_switch_is_part_of_the_draft_and_on_is_what_gets_saved() {
        let mut model = shop(false);
        let mut st = opened(&model);
        assert!(!st.panel.as_ref().unwrap().draft.shared_build_cache);
        let _ = update(&mut st, &mut model, PanelMsg::BuildCache(true));
        let draft = &st.panel.as_ref().unwrap().draft;
        assert!(draft.shared_build_cache);
        assert!(normalize_draft(draft).shared_build_cache);
        // It is a change to save, and not the defaults any more.
        assert!(!same_settings(
            &normalize_draft(draft),
            &orchestration_defaults()
        ));
    }

    #[test]
    fn empty_asks_first_and_cancel_leaves_the_cache_alone() {
        let mut model = shop(false);
        let mut st = opened(&model);
        cache_measured(&mut st, "w1", 5_000_000);
        let _ = update(&mut st, &mut model, PanelMsg::EmptyCache);
        assert!(st.panel.as_ref().unwrap().cache.asking);
        let pkg = Packages::default();
        let ws: WsId = "w1".into();
        {
            let mut ui = simulate(view(&st, ctx(&model, &pkg), &ws));
            assert!(ui.find("Yes, empty").is_ok());
        }
        let _ = update(&mut st, &mut model, PanelMsg::EmptyCancel);
        let cache = &st.panel.as_ref().unwrap().cache;
        assert!(!cache.asking);
        assert!(!cache.emptying);
    }

    #[test]
    fn confirming_without_having_been_asked_does_nothing() {
        let mut model = shop(false);
        let mut st = opened(&model);
        cache_measured(&mut st, "w1", 5_000_000);
        let _ = update(&mut st, &mut model, PanelMsg::EmptyConfirm);
        assert!(!st.panel.as_ref().unwrap().cache.emptying);
    }

    #[test]
    fn empty_is_refused_while_a_worker_of_the_workspace_runs() {
        let mut model = shop(true);
        let mut st = opened(&model);
        cache_measured(&mut st, "w1", 5_000_000);
        let _ = update(&mut st, &mut model, PanelMsg::EmptyCache);
        assert!(!st.panel.as_ref().unwrap().cache.asking);
        // Nor does a yes go through, if a worker started after the question.
        st.panel.as_mut().unwrap().cache.asking = true;
        let _ = update(&mut st, &mut model, PanelMsg::EmptyConfirm);
        let cache = &st.panel.as_ref().unwrap().cache;
        assert!(!cache.asking);
        assert!(!cache.emptying);
    }

    #[test]
    fn sizes_read_in_megabytes_then_gigabytes() {
        use hangar_i18n::Locale;
        assert_eq!(size_text(Locale::En, 0), "0 MB");
        assert_eq!(size_text(Locale::En, 412 * 1024 * 1024), "412 MB");
        assert_eq!(size_text(Locale::En, 1536 * 1024 * 1024), "1.5 GB");
        assert_eq!(size_text(Locale::Fr, 1536 * 1024 * 1024), "1,5 Go");
        assert_eq!(size_text(Locale::Fr, 12 * 1024 * 1024), "12 Mo");
    }
}
