//! A category of the schema, drawn: sections with their title and hint, a
//! card of rows or a column of bare blocks, and one control per kind of row
//! (`Row`, `Control` and `ActionControl` of `SettingsPage.tsx`).
//!
//! Built from hangar-ui's form widgets, so a row looks like every other row of
//! the app and the page draws no colour of its own.

use hangar_i18n::Key;
use iced::widget::{column, container, mouse_area};
use iced::{Element, Length, Padding};

use hangar_theme::tokens::space;
use hangar_ui::{button, form, input, text as typography};

use super::change::{Change, Target, Value};
use super::schema::{Act, Block, Category, Choice, Item, Kind, Section};
use super::{Msg, State};
use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::Settings;

fn send(msg: Msg) -> Message {
    Message::Settings(msg)
}

fn changed(target: &Target, value: Value) -> Message {
    send(Msg::Change(Change::Set(target.clone(), value)))
}

/// The decimals a slider's step has: 0.05 has two, 0.5 one, 5 none.
fn decimals(step: f64) -> i32 {
    (0..=6)
        .find(|digits| {
            let scaled = step * 10f64.powi(*digits);
            (scaled - scaled.round()).abs() < 1e-9
        })
        .unwrap_or(6)
}

/// What a slider reports as the value it chose. iced's slider works in `f32`,
/// where `1.0 + 3 * 0.05` is `1.1500000238…`: rounded to the decimals of its
/// step, the setting is stored as `1.15` and not as that.
pub fn snap(raw: f32, step: f64) -> f64 {
    let factor = 10f64.powi(decimals(step));
    (f64::from(raw) * factor).round() / factor
}

/// The text a field shows for the setting it edits. The field borrows it,
/// which is why it comes from the settings and not from the schema, built
/// anew for every frame.
pub fn text_of<'a>(settings: &'a Settings, target: &Target) -> &'a str {
    match target {
        Target::ClaudeCommand => settings.agent_commands.claude.as_deref().unwrap_or(""),
        Target::VoiceCleanupModel => &settings.voice_cleanup_model,
        Target::VoiceCleanupHint => &settings.voice_cleanup_hint,
        Target::DiscordAppId => &settings.discord_app_id,
        _ => "",
    }
}

/// A placeholder from the schema, which holds it borrowed for `'static`.
fn placeholder(item: std::borrow::Cow<'static, str>) -> &'static str {
    match item {
        std::borrow::Cow::Borrowed(words) => words,
        std::borrow::Cow::Owned(_) => "",
    }
}

/// The control of a row, and whether the row is one a click on its text
/// reaches (a switch).
fn control<'a>(
    ctx: Ctx<'a>,
    st: &'a State,
    item: &Item,
    kind: Kind,
) -> (Element<'a, Message>, bool, Option<Message>) {
    let t = ctx.theme;
    let settings = ctx.model.settings();
    match kind {
        Kind::Select {
            value,
            choices,
            target,
        } => {
            let selected: Option<Choice> = choices.iter().find(|c| c.value == value).cloned();
            let target = target.clone();
            (
                input::select(t, choices, selected, move |choice: Choice| {
                    changed(&target, Value::Text(choice.value))
                }),
                false,
                None,
            )
        }
        Kind::Segmented {
            value,
            choices,
            target,
        } => {
            let options: Vec<(String, &'static str)> = choices
                .iter()
                .map(|choice| (choice.value.clone(), choice.static_label()))
                .collect();
            (
                input::segmented(t, &options, &value, move |picked: String| {
                    changed(&target, Value::Text(picked))
                }),
                false,
                None,
            )
        }
        Kind::Toggle { value, target } => {
            let flip = changed(&target, Value::Bool(!value));
            let control = input::switch(t, value)
                .on_toggle(move |on| changed(&target, Value::Bool(on)))
                .into();
            (control, true, Some(flip))
        }
        Kind::Input {
            placeholder: hint,
            target,
            ..
        } => {
            let shown = text_of(settings, &target);
            (
                crate::keymap::guard(
                    ctx,
                    input::text_field(t, shown)
                        .placeholder(placeholder(hint))
                        .on_input(move |text| changed(&target, Value::Text(text))),
                ),
                false,
                None,
            )
        }
        Kind::Slider {
            value,
            min,
            max,
            step,
            display,
            target,
        } => (
            input::slider(t, min as f32..=max as f32, value as f32, move |raw| {
                changed(&target, Value::Number(snap(raw, step)))
            })
            .step(step as f32)
            .display(display)
            .into(),
            false,
            None,
        ),
        Kind::Text { value } => (typography::mono(t, value).into(), false, None),
        Kind::Action {
            label,
            confirm,
            danger,
            act,
        } => (
            action(ctx, st, item, label, confirm, danger, act),
            false,
            None,
        ),
        Kind::Custom { .. } => (column![].into(), false, None),
    }
}

/// A button, or the pair a two-step one arms into (`ActionControl`).
fn action<'a>(
    ctx: Ctx<'a>,
    st: &'a State,
    item: &Item,
    label: String,
    confirm: Option<String>,
    danger: bool,
    act: Act,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let id = item.id.clone();
    let run = send(Msg::Act(act));
    let Some(confirm) = confirm else {
        return if danger {
            button::danger(t, label).on_press(run).into()
        } else {
            button::secondary(t, label).on_press(run).into()
        };
    };
    if st.armed.as_deref() == Some(id.as_str()) {
        let sure = if danger {
            button::danger(t, confirm)
        } else {
            button::primary(t, confirm)
        };
        iced::widget::row![
            button::ghost(t, ctx.t(Key::CreateCancel).into_owned()).on_press(send(Msg::Disarm)),
            sure.on_press(run),
        ]
        .spacing(space::MD)
        .into()
    } else {
        button::secondary(t, label)
            .on_press(send(Msg::Arm(id)))
            .into()
    }
}

/// What a custom block draws: each is another module's.
fn block<'a>(ctx: Ctx<'a>, st: &'a State, block: Block) -> Element<'a, Message> {
    match block {
        Block::ProjectsFolder => super::previews::projects_folder(ctx),
        Block::Themes { active } => super::previews::theme_grid(ctx, &active),
        Block::TerminalPreview => super::previews::terminal_preview(ctx),
        Block::Fonts => super::previews::font_grid(ctx),
        Block::Mobile => super::mobile::view(&st.mobile, ctx),
        Block::Kits => crate::kits::settings_view(&ctx.pkg.kits, ctx),
        Block::Accounts => crate::accounts::settings_page(&ctx.pkg.accounts, ctx),
        Block::VoiceModel => super::voice_card::model_view(&st.voice, ctx),
        Block::VoiceKey => super::voice_card::key_view(&st.voice, ctx),
        Block::DiscordCard => super::discord_card::view(&st.discord, ctx, st.launched_at),
        Block::Shortcuts => super::shortcuts::view(&st.shortcuts, ctx),
        Block::Nodes => crate::nodes::settings_page(&ctx.pkg.nodes, ctx),
    }
}

/// One row of a card.
fn row<'a>(ctx: Ctx<'a>, st: &'a State, item: Item) -> Element<'a, Message> {
    let t = ctx.theme;
    if let Kind::Custom { block: wanted, .. } = &item.kind {
        let content = block(ctx, st, wanted.clone());
        return form::block(t, item.label.as_deref(), item.hint.as_deref(), content);
    }
    let label = item.label.clone().unwrap_or_default();
    let hint = item.hint.clone();
    let wide = matches!(
        item.kind,
        Kind::Select { .. } | Kind::Input { .. } | Kind::Slider { .. }
    );
    let (control, toggles, flip) = control(ctx, st, &item, item.kind.clone());
    let mut line = form::row(t, label, control);
    if let Some(hint) = hint {
        line = line.hint(hint);
    }
    if wide {
        line = line.wide();
    }
    let line: Element<'a, Message> = line.into();
    // A click on a switch's text reaches the switch, as a `<label>` made it.
    match (toggles, flip) {
        (true, Some(flip)) => mouse_area(line).on_press(flip).into(),
        _ => line,
    }
}

/// A section: its head when it has something to say that its page does not,
/// then a card of rows, or the bare blocks when all of them are boxes of their
/// own.
fn section<'a>(
    ctx: Ctx<'a>,
    st: &'a State,
    section: Section,
    category_label: &str,
) -> Element<'a, Message> {
    let t = ctx.theme;
    // A group named like its page would only repeat the title above it; its
    // hint still has something to say.
    let titled = section.title != category_label;
    let bare = section
        .items
        .iter()
        .all(|item| matches!(item.kind, Kind::Custom { bare: true, .. }));

    let mut stack = column![].spacing(space::MD);
    if titled || section.hint.is_some() {
        let mut head = column![].spacing(2);
        if titled {
            head = head.push(
                typography::heading(t, section.title).size(hangar_theme::tokens::font_size::BODY),
            );
        }
        if let Some(hint) = section.hint {
            head = head.push(typography::hint(t, hint));
        }
        stack = stack.push(container(head).padding(Padding::from([0.0, space::XS])));
    }

    if bare {
        let blocks = section
            .items
            .into_iter()
            .filter_map(|item| match item.kind {
                Kind::Custom { block: wanted, .. } => Some(block(ctx, st, wanted)),
                _ => None,
            });
        stack = stack.push(column(blocks).spacing(space::LG));
    } else {
        let rows = section
            .items
            .into_iter()
            .map(|item| row(ctx, st, item))
            .collect();
        stack = stack.push(form::group(t, rows));
    }
    stack.into()
}

/// The content of a category: its sections, spaced apart.
pub fn page<'a>(ctx: Ctx<'a>, st: &'a State, category: Category) -> Element<'a, Message> {
    let label = category.label;
    let sections = category
        .sections
        .into_iter()
        .map(|one| section(ctx, st, one, &label))
        .collect();
    container(form::page(sections))
        .max_width(680)
        .width(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests;
