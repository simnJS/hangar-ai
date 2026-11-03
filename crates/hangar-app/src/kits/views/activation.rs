//! Enabling a kit, drawn: `KitActivation` and `KitActivationView` of
//! `KitActivation.tsx`.
//!
//! A dialog that says what a kit brings, what its tabs ask for, which setup
//! commands it wants run, which its plugin runs by itself and which its tabs
//! may run, before anything is stored. Refusing changes nothing.
//!
//! It opens over another dialog (the workspace's Kits dialog, the settings),
//! as a layer of its own; Escape and the veil close this one only, because
//! they are the top overlay's (`Overlay::KitActivation`).
//!
//! While a setup command awaits the person's word on how it went, the dialog
//! steps aside: a smaller card in the window's corner and no veil, so that the
//! pane the command runs in can be watched and used (`.kit-act-peek`).

use iced::widget::{Space, column, container, row, text};
use iced::{Alignment, Element, Length};

use hangar_core::kits::KitInfo;
use hangar_i18n::Key;
use hangar_theme::{style, tokens};
use hangar_ui::badge::Tone;
use hangar_ui::dialog::{self, dialog};
use hangar_ui::{button, layer, text as typography};

use super::activation_model::{
    ActivationInput, ActivationModel, CommandModel, Footer, HookList, Loadable, RunGroupModel,
    RunItemModel, StepAction, StepModel, TabPerms,
};
use super::{block, code_block, msg, note, notice, stack, tag};
use crate::ctx::Ctx;
use crate::kits::icon::kit_icon;
use crate::kits::setup::current_os;
use crate::kits::{Msg, State, Target};
use crate::message::{Intent, Message};
use crate::state::WsId;

/// `Overlay::KitActivation`: the dialog, or nothing when the state it was
/// opened with is gone (the kit was removed under it, or it was closed by
/// way of another door).
pub fn view<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    kit_id: &'a str,
    _ws: Option<&'a WsId>,
) -> Element<'a, Message> {
    let Some(activation) = st.activation.as_ref().filter(|a| a.kit == kit_id) else {
        return Space::new().into();
    };
    let Some(kit) = ctx.model.live.kits.find(kit_id) else {
        return Space::new().into();
    };
    let entry = activation.entry(ctx.model, &st.draft);
    let workspace_name = activation
        .setup_workspace()
        .and_then(|ws| ctx.model.workspace(ws))
        .map(|ws| ws.name.as_str());
    let input = ActivationInput {
        kit,
        inspection: activation.inspection.as_ref(),
        steps: activation.steps.as_deref(),
        entry: entry.as_ref(),
        // The workspace is named when the kit is enabled on that workspace; for
        // "everywhere" it is only where a setup step runs.
        workspace_name: match activation.target {
            Target::Workspace(_) => workspace_name,
            Target::Draft | Target::Everywhere => None,
        },
        everywhere: activation.target == Target::Everywhere,
        can_run: activation.setup_workspace().is_some(),
        os: current_os(),
        running: activation.running.as_deref(),
        ran: activation.ran.as_ref(),
        error: activation.error.as_deref(),
    };
    let model = ActivationModel::build(ctx.i18n, &input);
    let peek = activation.ran.is_some();
    let close = Message::Intent(Intent::CloseOverlay);

    let mut card = dialog(ctx.theme, model.title.clone(), body(ctx, kit, &model))
        .on_close(close.clone())
        .close_tip(ctx.t(Key::ModalClose))
        .width(if peek { 460.0 } else { 640.0 })
        .body_max_height(if peek { 280.0 } else { 520.0 });
    match &model.footer {
        Footer::Decide {
            refuse,
            accept,
            accept_enabled,
        } => {
            card = card
                .action(button::ghost(ctx.theme, refuse.clone()).on_press(close))
                .action(
                    button::primary(ctx.theme, accept.clone())
                        .on_press_maybe(accept_enabled.then(|| msg(Msg::Accept))),
                );
        }
        Footer::Close(label) => {
            card = card.action(button::secondary(ctx.theme, label.clone()).on_press(close));
        }
    }

    if peek {
        let dismiss = Message::Intent(Intent::CloseOverlay);
        layer::dock(Space::new(), Some(card.into()))
            .on_dismiss(dismiss)
            .into()
    } else {
        dialog::over(ctx.theme, Space::new(), Some(card)).into()
    }
}

/// The dialog's body.
pub fn body<'a>(ctx: Ctx<'a>, kit: &KitInfo, model: &ActivationModel) -> Element<'a, Message> {
    let mut parts: Vec<Element<'a, Message>> = Vec::new();
    if let Some(target) = &model.target {
        parts.push(typography::body(ctx.theme, target.clone()).into());
    }
    if let Some(head) = &model.head {
        let mut line = row![
            kit_icon(ctx, kit, 18.0),
            typography::body(ctx.theme, head.name.clone()).font(hangar_theme::fonts::ui_semibold()),
            typography::mono(ctx.theme, head.version.clone()),
        ]
        .spacing(tokens::space::MD)
        .align_y(Alignment::Center);
        if let Some(description) = &head.description {
            line = line.push(typography::label(ctx.theme, format!("— {description}")));
        }
        parts.push(line.wrap().into());
    }

    parts.push(brings(ctx, model));
    parts.push(perms(ctx, model));
    parts.push(setup(ctx, model));
    parts.push(runs(ctx, model));
    parts.push(commands(ctx, model));
    if let Some(error) = &model.error {
        parts.push(notice(ctx, Tone::Danger, error.clone()));
    }
    column(parts)
        .spacing(tokens::space::XXL)
        .width(Length::Fill)
        .into()
}

/// A section that may still be read, or have nothing: its words, or what
/// `ready` draws.
fn loadable<'a, T>(
    ctx: Ctx<'a>,
    body: &Loadable<T>,
    ready: impl FnOnce(&T) -> Vec<Element<'a, Message>>,
) -> Vec<Element<'a, Message>> {
    match body {
        Loadable::Loading(words) | Loadable::Empty(words) if words.is_empty() => Vec::new(),
        Loadable::Loading(words) | Loadable::Empty(words) => vec![note(ctx, words.clone())],
        Loadable::Ready(value) => ready(value),
    }
}

fn brings<'a>(ctx: Ctx<'a>, model: &ActivationModel) -> Element<'a, Message> {
    let b = &model.brings;
    let mut parts = loadable(ctx, &b.body, |groups| {
        groups
            .iter()
            .map(|group| {
                let chips = group.chips.iter().map(|chip| {
                    let mut line = row![
                        text(chip.name.clone())
                            .size(tokens::font_size::UI)
                            .font(hangar_theme::fonts::mono())
                    ]
                    .spacing(tokens::space::SM)
                    .align_y(Alignment::Center);
                    if let Some(kind) = &chip.kind {
                        line = line.push(typography::subtle(ctx.theme, kind.clone()));
                    }
                    container(line)
                        .padding([tokens::space::XXS, tokens::space::MD])
                        .style(style::container::chip(ctx.theme))
                        .into()
                });
                column![
                    row![
                        typography::ui(ctx.theme, group.title.clone())
                            .font(hangar_theme::fonts::ui_medium()),
                        typography::muted(ctx.theme, group.count.to_string()),
                    ]
                    .spacing(tokens::space::SM),
                    iced::widget::Row::with_children(chips)
                        .spacing(tokens::space::SM)
                        .wrap()
                        .vertical_spacing(tokens::space::SM),
                ]
                .spacing(tokens::space::SM)
                .into()
            })
            .collect()
    });
    parts.extend(b.tab_kind_hint.iter().map(|hint| note(ctx, hint.clone())));
    parts.push(note(ctx, b.claude_only.clone()));
    block(ctx, &b.heading, parts)
}

fn perms<'a>(ctx: Ctx<'a>, model: &ActivationModel) -> Element<'a, Message> {
    let parts = loadable(ctx, &model.perms.body, |tabs| {
        tabs.iter().map(|tab| tab_perms(ctx, tab)).collect()
    });
    block(ctx, &model.perms.heading, parts)
}

fn tab_perms<'a>(ctx: Ctx<'a>, tab: &TabPerms) -> Element<'a, Message> {
    let mut items: Vec<Element<'a, Message>> = vec![
        row![
            typography::ui(ctx.theme, tab.title.clone()).font(hangar_theme::fonts::ui_medium()),
            typography::subtle(ctx.theme, tab.kind.clone()),
        ]
        .spacing(tokens::space::MD)
        .align_y(Alignment::Center)
        .into(),
    ];
    for item in &tab.items {
        let mut line = row![typography::body(ctx.theme, item.label.clone()).width(Length::Shrink)]
            .spacing(tokens::space::MD)
            .align_y(Alignment::Center);
        if let Some(item_tag) = &item.tag {
            line = line.push(tag(ctx, item_tag));
        }
        items.push(line.wrap().into());
    }
    if let Some(secrets) = &tab.secrets {
        items.push(notice(ctx, Tone::Warning, secrets.clone()));
    }
    column(items).spacing(tokens::space::SM).into()
}

fn setup<'a>(ctx: Ctx<'a>, model: &ActivationModel) -> Element<'a, Message> {
    let parts = loadable(ctx, &model.setup.body, |(hint, steps)| {
        let mut parts = vec![note(ctx, hint.clone())];
        parts.extend(steps.iter().map(|step| step_block(ctx, step)));
        parts
    });
    block(ctx, &model.setup.heading, parts)
}

/// What a step, a hook or a command is drawn in: a card with a head, notes and
/// the command.
fn card<'a>(ctx: Ctx<'a>, parts: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    container(
        iced::widget::Column::with_children(parts)
            .spacing(tokens::space::MD)
            .width(Length::Fill),
    )
    .padding([tokens::space::LG, tokens::space::XL])
    .width(Length::Fill)
    .style(style::container::card(ctx.theme))
    .into()
}

/// The head of a card: a title that breaks anywhere, and a tag.
fn card_head<'a>(
    ctx: Ctx<'a>,
    title: &str,
    tag_of: Option<&super::activation_model::Tag>,
) -> Element<'a, Message> {
    let mut head = row![
        typography::body(ctx.theme, title.to_owned())
            .font(hangar_theme::fonts::ui_medium())
            .wrapping(text::Wrapping::WordOrGlyph)
            .width(Length::Fill)
    ]
    .spacing(tokens::space::MD)
    .align_y(Alignment::Center);
    if let Some(found) = tag_of {
        head = head.push(tag(ctx, found));
    }
    head.into()
}

fn step_block<'a>(ctx: Ctx<'a>, step: &StepModel) -> Element<'a, Message> {
    let mut parts = vec![card_head(ctx, &step.title, Some(&step.tag))];
    if let Some(description) = &step.description {
        parts.push(note(ctx, description.clone()));
    }
    parts.push(typography::subtle(ctx.theme, step.os_label.clone()).into());
    parts.push(code_block(ctx, &step.command));
    match &step.action {
        StepAction::None => {}
        StepAction::Ask {
            opened,
            confirm,
            not_yet,
        } => {
            parts.push(note(ctx, opened.clone()));
            parts.push(
                row![
                    button::primary(ctx.theme, confirm.clone())
                        .on_press(msg(Msg::Confirm(step.id.clone()))),
                    button::ghost(ctx.theme, not_yet.clone()).on_press(msg(Msg::NotYet)),
                ]
                .spacing(tokens::space::MD)
                .into(),
            );
        }
        StepAction::Run {
            label,
            enabled,
            needs_workspace,
        } => {
            let mut line = row![
                button::secondary(ctx.theme, label.clone())
                    .on_press_maybe(enabled.then(|| msg(Msg::Run(step.id.clone()))))
            ]
            .spacing(tokens::space::MD)
            .align_y(Alignment::Center);
            if let Some(words) = needs_workspace {
                line = line.push(note(ctx, words.clone()));
            }
            parts.push(line.into());
        }
    }
    card(ctx, parts)
}

fn runs<'a>(ctx: Ctx<'a>, model: &ActivationModel) -> Element<'a, Message> {
    let parts = loadable(ctx, &model.runs.body, |body| {
        let mut parts: Vec<Element<'a, Message>> = Vec::new();
        parts.extend(body.changed.iter().map(|words| note(ctx, words.clone())));
        parts.extend(body.live.iter().map(|words| note(ctx, words.clone())));
        if let Some(list) = &body.hooks {
            parts.push(hook_list(ctx, list));
        }
        parts.extend(body.groups.iter().map(|group| run_group(ctx, group)));
        parts
    });
    block(ctx, &model.runs.heading, parts)
}

fn group_head<'a>(ctx: Ctx<'a>, title: &str, count: usize) -> Element<'a, Message> {
    row![
        typography::ui(ctx.theme, title.to_owned()).font(hangar_theme::fonts::ui_medium()),
        typography::muted(ctx.theme, count.to_string()),
    ]
    .spacing(tokens::space::SM)
    .into()
}

fn hook_list<'a>(ctx: Ctx<'a>, list: &HookList) -> Element<'a, Message> {
    let mut parts = vec![
        group_head(ctx, &list.title, list.count),
        note(ctx, list.hint.clone()),
    ];
    for item in &list.items {
        parts.push(card(
            ctx,
            vec![
                card_head(ctx, &item.title, item.tag.as_ref()),
                code_block(ctx, &item.command),
            ],
        ));
    }
    stack(parts)
}

fn run_group<'a>(ctx: Ctx<'a>, group: &RunGroupModel) -> Element<'a, Message> {
    let mut parts = vec![
        group_head(ctx, &group.title, group.count),
        note(ctx, group.when.clone()),
    ];
    parts.extend(group.items.iter().map(|item| run_item(ctx, item)));
    stack(parts)
}

fn run_item<'a>(ctx: Ctx<'a>, item: &RunItemModel) -> Element<'a, Message> {
    let mut parts = vec![card_head(ctx, &item.title, item.tag.as_ref())];
    if let Some(description) = &item.description {
        parts.push(note(ctx, description.clone()));
    }
    if let Some(line) = &item.line {
        parts.push(code_block(ctx, line));
    }
    for words in [&item.env, &item.headers].into_iter().flatten() {
        parts.push(note(ctx, words.clone()));
    }
    if let Some(words) = &item.unreadable {
        // Said louder than a note, never left out.
        parts.push(
            typography::ui(ctx.theme, words.clone())
                .font(hangar_theme::fonts::ui_medium())
                .into(),
        );
    }
    card(ctx, parts)
}

fn commands<'a>(ctx: Ctx<'a>, model: &ActivationModel) -> Element<'a, Message> {
    let parts = loadable(ctx, &model.commands.body, |body| {
        let mut parts: Vec<Element<'a, Message>> = Vec::new();
        parts.extend(body.changed.iter().map(|words| note(ctx, words.clone())));
        parts.extend(body.live.iter().map(|words| note(ctx, words.clone())));
        parts.extend(body.items.iter().map(|item| command_block(ctx, item)));
        parts
    });
    block(ctx, &model.commands.heading, parts)
}

fn command_block<'a>(ctx: Ctx<'a>, command: &CommandModel) -> Element<'a, Message> {
    let mut parts = vec![card_head(ctx, &command.name, command.tag.as_ref())];
    parts.push(note(ctx, command.description.clone()));
    parts.push(code_block(ctx, &command.line));
    parts.push(note(ctx, command.hint.clone()));
    if let Some(words) = &command.kit_dir {
        parts.push(note(ctx, words.clone()));
    }
    for rule in &command.rules {
        let mut line = row![
            typography::mono(ctx.theme, rule.placeholder.clone()),
            typography::mono(ctx.theme, rule.pattern.clone())
                .style(style::text::primary(ctx.theme))
                .wrapping(text::Wrapping::WordOrGlyph),
        ]
        .spacing(tokens::space::MD);
        if let Some(words) = &rule.description {
            line = line.push(typography::hint(ctx.theme, words.clone()));
        }
        parts.push(line.wrap().into());
    }
    card(ctx, parts)
}
