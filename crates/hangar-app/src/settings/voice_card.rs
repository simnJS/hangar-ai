//! The local dictation model and the Groq key, in Settings › Voice
//! (`VoiceCard.tsx`).
//!
//! Worth their own blocks rather than settings rows. A model is half a
//! gigabyte that has to arrive before dictation can do anything, and the two
//! questions that follow from it (is it here yet, how far along is it) are not
//! things a label and a control can answer. The key lives in the system
//! keychain and the window is never given it back once saved, so a field would
//! have nothing to show: the card says whether one is there, and offers to
//! replace or clear it.
//!
//! The card loads when the category is shown: the catalogue and the keychain's
//! answer, and a download's progress arrives as `CoreEvent::VoiceDownload`.

use hangar_core::voice::models::{self as models, Progress};
use hangar_core::voice::{self, key};
use hangar_i18n::{Key, args};
use iced::widget::{column, container, progress_bar, row};
use iced::{Element, Length, Padding, Task};

use hangar_theme::style;
use hangar_theme::tokens::space;
use hangar_ui::{button, input, text as typography};

use super::change::Change;
use super::format::format_bytes;
use crate::ctx::Ctx;
use crate::message::Message;
use crate::services::Services;
use crate::state::Model;

/// One entry of the local model catalogue (`VoiceModel`, hangar-core's
/// `ModelInfo`).
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceModel {
    pub id: String,
    pub label: String,
    pub approx_bytes: u64,
    pub languages: String,
    pub license: String,
    pub installed: bool,
}

impl From<models::ModelInfo> for VoiceModel {
    fn from(info: models::ModelInfo) -> Self {
        VoiceModel {
            id: info.id,
            label: info.label,
            approx_bytes: info.approx_bytes,
            languages: info.languages,
            license: info.license,
            installed: info.installed,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    /// The catalogue, as the PC lists it now.
    Models(Vec<VoiceModel>),
    Download(String),
    /// The download ended, one way or the other.
    Downloaded(Result<(), String>),
    /// The keychain's answer: is a key saved.
    KeyStatus(Result<bool, String>),
    KeyDraft(String),
    /// Save this key (what was typed, or the legacy one being moved).
    KeySave(String),
    KeySaved(Result<(), String>),
    Replace,
    CancelReplace,
    Arm(bool),
    KeyClear,
    KeyCleared(Result<(), String>),
}

#[derive(Debug, Default)]
pub struct State {
    models: Vec<VoiceModel>,
    progress: Option<Progress>,
    error: Option<String>,
    /// `None` until the keychain answers, and while it cannot be reached.
    saved: Option<bool>,
    unavailable: Option<String>,
    key_error: Option<String>,
    replacing: bool,
    draft: String,
    armed: bool,
    busy: bool,
}

fn wrap(msg: Msg) -> Message {
    Message::Settings(super::Msg::Voice(msg))
}

fn ask<T: Send + 'static>(
    job: impl std::future::Future<Output = Result<T, String>> + Send + 'static,
    then: fn(Result<T, String>) -> Msg,
) -> Task<Message> {
    Task::perform(job, move |answer| wrap(then(answer)))
}

/// `voice_models`, which looks at the disk to say what is installed.
pub(super) fn read_models(sv: &Services) -> Task<Message> {
    let host = sv.core().host().clone();
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                models::voice_models(&host)
                    .into_iter()
                    .map(VoiceModel::from)
                    .collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default()
        },
        |list| wrap(Msg::Models(list)),
    )
}

fn read_key_status() -> Task<Message> {
    ask(key::voice_key_status(), Msg::KeyStatus)
}

/// How far along a download is, in whole percent: `received` of `total`,
/// which can still be 0 when the size is not known yet.
pub fn percent(received: u64, total: u64) -> u32 {
    let ratio = received as f64 / total.max(1) as f64;
    ((ratio * 100.0).round() as u32).min(100)
}

impl State {
    /// The category is shown: the catalogue and the keychain.
    pub fn enter(&mut self, sv: &Services) -> Task<Message> {
        *self = State::default();
        Task::batch([read_models(sv), read_key_status()])
    }

    /// The model the card shows: the selected one, else the first (an id that
    /// no catalogue knows resolves to the one there is).
    fn model(&self, selected: &str) -> Option<&VoiceModel> {
        self.models
            .iter()
            .find(|model| model.id == selected)
            .or_else(|| self.models.first())
    }

    /// A download's progress. A finished one changes what the rest of the page
    /// may offer, so the catalogue is read again rather than patched in
    /// place.
    pub fn download_event(&mut self, progress: Progress) -> bool {
        let done = progress.done;
        self.progress = (!done).then_some(progress);
        done
    }

    /// Whether the key can be typed: none is saved, or one is being replaced.
    fn asking(&self) -> bool {
        self.saved == Some(false) || self.replacing
    }
}

/// A key typed (or a legacy one moved) is sent trimmed.
fn trimmed(key: &str) -> &str {
    key.trim()
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Models(list) => {
            st.models = list;
            Task::none()
        }
        Msg::Download(id) => {
            st.error = None;
            let size = st
                .models
                .iter()
                .find(|model| model.id == id)
                .map_or(0, |model| model.approx_bytes);
            st.progress = Some(Progress {
                id: id.clone(),
                received: 0,
                total: size,
                done: false,
            });
            let host = sv.core().host().clone();
            ask(models::voice_model_download(host, id), Msg::Downloaded)
        }
        Msg::Downloaded(answer) => {
            st.progress = None;
            if let Err(error) = answer {
                st.error = Some(error);
            }
            read_models(sv)
        }
        Msg::KeyStatus(answer) => {
            match answer {
                Ok(present) => {
                    st.saved = Some(present);
                    st.unavailable = None;
                }
                Err(error) => {
                    st.saved = None;
                    st.unavailable = Some(error);
                }
            }
            Task::none()
        }
        Msg::KeyDraft(draft) => {
            st.draft = draft;
            Task::none()
        }
        Msg::KeySave(key) => {
            st.busy = true;
            st.key_error = None;
            ask(key::voice_key_set(trimmed(&key).to_string()), Msg::KeySaved)
        }
        Msg::KeySaved(answer) => {
            st.busy = false;
            let refresh = read_key_status();
            match answer {
                Err(error) => {
                    st.key_error = Some(error);
                    refresh
                }
                Ok(()) => {
                    st.draft.clear();
                    st.replacing = false;
                    // Whatever the keychain now holds supersedes the plaintext
                    // copy.
                    let legacy = m
                        .settings()
                        .voice_api_key
                        .as_deref()
                        .is_some_and(|key| !key.trim().is_empty());
                    if legacy {
                        Task::batch([
                            refresh,
                            Task::done(Message::Settings(super::Msg::Change(
                                Change::DropLegacyVoiceKey,
                            ))),
                        ])
                    } else {
                        refresh
                    }
                }
            }
        }
        Msg::Replace => {
            st.replacing = true;
            Task::none()
        }
        Msg::CancelReplace => {
            st.replacing = false;
            st.draft.clear();
            Task::none()
        }
        Msg::Arm(armed) => {
            st.armed = armed;
            Task::none()
        }
        Msg::KeyClear => {
            st.armed = false;
            st.busy = true;
            st.key_error = None;
            ask(key::voice_key_clear(), Msg::KeyCleared)
        }
        Msg::KeyCleared(answer) => {
            st.busy = false;
            if let Err(error) = answer {
                st.key_error = Some(error);
            }
            read_key_status()
        }
    }
}

/// `voice_unload`: the resident model leaves memory.
pub fn unload(sv: &Services) {
    voice::voice_unload(sv.core().core().voice());
}

// ---- Drawing ------------------------------------------------------------------------------------

fn note<'a>(ctx: Ctx<'a>, words: impl Into<String>) -> Element<'a, Message> {
    typography::hint(ctx.theme, words.into()).into()
}

fn error_note<'a>(ctx: Ctx<'a>, words: impl Into<String>) -> Element<'a, Message> {
    typography::danger(ctx.theme, words.into()).into()
}

fn card<'a>(ctx: Ctx<'a>, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content.into())
        .padding(Padding::from([0.0, 0.0]))
        .width(Length::Fill)
        .style(style::container::plain(ctx.theme))
        .into()
}

/// The local model and how to get it.
pub fn model_view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    let locale = ctx.i18n.locale();
    let Some(model) = st.model(&ctx.model.settings().voice_model) else {
        return column![].into();
    };
    let downloading = st
        .progress
        .as_ref()
        .is_some_and(|progress| progress.id == model.id);
    let tag = if model.installed {
        ctx.t(Key::SettingsVoiceInstalled).into_owned()
    } else {
        format_bytes(locale, model.approx_bytes)
    };
    let tag = if model.installed {
        typography::ui(t, tag).style(style::text::success(t))
    } else {
        typography::ui(t, tag).style(style::text::muted(t))
    };

    let mut stack = column![
        row![
            typography::ui(t, model.label.clone()).font(hangar_theme::fonts::ui_semibold()),
            tag
        ]
        .spacing(space::LG)
        .align_y(iced::Alignment::Center),
        note(
            ctx,
            ctx.i18n.t_with(
                Key::SettingsVoiceModelHint,
                &args!("languages" => model.languages.as_str(), "license" => model.license.as_str()),
            ),
        ),
    ]
    .spacing(space::MD);

    match st.progress.as_ref().filter(|_| downloading) {
        Some(progress) => {
            let percent = percent(progress.received, progress.total);
            stack = stack
                .push(
                    progress_bar(0.0..=100.0, percent as f32)
                        .girth(6.0)
                        .style(style::progress_bar::accent(t)),
                )
                .push(note(
                    ctx,
                    ctx.i18n
                        .t_with(Key::SettingsVoiceDownloading, &args!("percent" => percent)),
                ));
        }
        None => {
            let label = if model.installed {
                ctx.t(Key::SettingsVoiceDownloadAgain).into_owned()
            } else {
                ctx.i18n
                    .t_with(
                        Key::SettingsVoiceDownload,
                        &args!("size" => format_bytes(locale, model.approx_bytes)),
                    )
                    .into_owned()
            };
            let mut actions =
                row![button::primary(t, label).on_press(wrap(Msg::Download(model.id.clone())))]
                    .spacing(space::LG)
                    .align_y(iced::Alignment::Center);
            if !model.installed {
                actions = actions.push(note(ctx, ctx.t(Key::SettingsVoiceMissing).into_owned()));
            }
            stack = stack.push(actions);
        }
    }
    if let Some(error) = &st.error {
        stack = stack.push(error_note(ctx, error.clone()));
    }
    card(ctx, stack)
}

/// The Groq key, which lives in the system keychain.
pub fn key_view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    let legacy = ctx
        .model
        .settings()
        .voice_api_key
        .as_deref()
        .map(str::trim)
        .unwrap_or("");

    let state_line = if st.unavailable.is_some() {
        ctx.t(Key::SettingsVoiceKeyUnavailableShort).into_owned()
    } else {
        match st.saved {
            None => String::new(),
            Some(true) => ctx.t(Key::SettingsVoiceKeySaved).into_owned(),
            Some(false) => ctx.t(Key::SettingsVoiceKeyNone).into_owned(),
        }
    };
    let state_line = if st.saved == Some(true) {
        typography::ui(t, state_line).style(style::text::success(t))
    } else {
        typography::ui(t, state_line).style(style::text::muted(t))
    };

    let mut stack = column![
        state_line,
        note(ctx, ctx.t(Key::SettingsVoiceKeyHint).into_owned()),
    ]
    .spacing(space::MD);

    if let Some(reason) = &st.unavailable {
        stack = stack.push(error_note(
            ctx,
            ctx.i18n
                .t_with(
                    Key::SettingsVoiceKeyUnavailable,
                    &args!("error" => reason.as_str()),
                )
                .into_owned(),
        ));
    }

    if !legacy.is_empty() {
        stack = stack.push(error_note(
            ctx,
            ctx.t(Key::SettingsVoiceKeyLegacy).into_owned(),
        ));
        let mut actions = row![].spacing(space::LG);
        if st.unavailable.is_none() {
            actions = actions.push(
                button::primary(t, ctx.t(Key::SettingsVoiceKeyLegacyMove).into_owned())
                    .on_press_maybe((!st.busy).then(|| wrap(Msg::KeySave(legacy.to_string())))),
            );
        }
        actions = actions.push(
            button::danger(t, ctx.t(Key::SettingsVoiceKeyLegacyDrop).into_owned()).on_press(
                Message::Settings(super::Msg::Change(Change::DropLegacyVoiceKey)),
            ),
        );
        stack = stack.push(actions);
    }

    if st.unavailable.is_none() && st.asking() {
        let typed = st.draft.trim();
        let save = (!st.busy && !typed.is_empty()).then(|| wrap(Msg::KeySave(st.draft.clone())));
        let mut actions = row![
            input::text_field(t, &st.draft)
                .placeholder(placeholder(ctx, Key::SettingsVoiceKeyPlaceholder))
                .secure()
                .on_input(|text| wrap(Msg::KeyDraft(text)))
                .on_submit(wrap(Msg::KeySave(st.draft.clone()))),
            button::primary(t, ctx.t(Key::SettingsVoiceKeySave).into_owned()).on_press_maybe(save),
        ]
        .spacing(space::LG)
        .align_y(iced::Alignment::Center);
        if st.replacing {
            actions = actions.push(
                button::ghost(t, ctx.t(Key::CreateCancel).into_owned())
                    .on_press(wrap(Msg::CancelReplace)),
            );
        }
        stack = stack.push(actions);
    }

    if st.unavailable.is_none() && st.saved == Some(true) && !st.replacing {
        let mut actions = row![
            button::secondary(t, ctx.t(Key::SettingsVoiceKeyReplace).into_owned())
                .on_press(wrap(Msg::Replace))
        ]
        .spacing(space::LG);
        if st.armed {
            actions = actions
                .push(
                    button::ghost(t, ctx.t(Key::CreateCancel).into_owned())
                        .on_press(wrap(Msg::Arm(false))),
                )
                .push(
                    button::danger(t, ctx.t(Key::SettingsVoiceKeyClearConfirm).into_owned())
                        .on_press_maybe((!st.busy).then(|| wrap(Msg::KeyClear))),
                );
        } else {
            actions = actions.push(
                button::secondary(t, ctx.t(Key::SettingsVoiceKeyClear).into_owned())
                    .on_press(wrap(Msg::Arm(true))),
            );
        }
        stack = stack.push(actions);
    }

    if let Some(error) = &st.key_error {
        stack = stack.push(error_note(ctx, error.clone()));
    }
    card(ctx, stack)
}

/// A plain entry of the dictionary, which the translator lends for the whole
/// program: what a text field wants as its placeholder.
fn placeholder(ctx: Ctx<'_>, key: Key) -> &'static str {
    match ctx.i18n.t(key) {
        std::borrow::Cow::Borrowed(words) => words,
        std::borrow::Cow::Owned(_) => "",
    }
}

#[cfg(test)]
mod tests;
