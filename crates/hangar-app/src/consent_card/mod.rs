//! P8: Git clone consent card.
//!
//! Ports GitCloneConsent.tsx. The question an agent's `workspace_create` waits
//! on when the repository is not the person's: what would be cloned, from
//! whom, where, and that its dependencies run its code here. Clone or Refuse;
//! unanswered, the tool is told nobody decided and nothing is cloned
//! (`git_flow::consent`, which also keeps the trace of each answer).
//!
//! It is a card over the corner of the window, like the app's notices: it does
//! not take the keyboard from the terminal being typed in, so a Return meant
//! for an agent never lands on Clone (iced's buttons take no keyboard focus in
//! the first place). A system notification calls the person when the window
//! is not the one they are looking at.
//!
//! The card speaks the person's language, the app's (the setting, else the
//! system's), as `useLocale` did; the trace in the activity log is in the
//! system's, like the rest of that log.

use std::collections::HashSet;
use std::sync::Arc;

use hangar_i18n::{Key, Translator, args};
use hangar_theme::style::Tone;
use hangar_theme::{style, tokens};
use hangar_ui::{button, text as typography};
use iced::futures::stream;
use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, row, text};
use iced::{Alignment, Element, Length, Subscription, Task, window};

use crate::ctx::Ctx;
use crate::git_flow::consent::{self, ConsentRequest};
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{Notification, Region, WindowKind};
use crate::state::Model;

/// The card's width, shrunk to fit a narrow window (`min(420px, 100vw - 32px)`).
const WIDTH: f32 = 420.0;
/// The column of the facts' names.
const NAMES: f32 = 100.0;

/// The package's UI state; lives in `App.pkg.consent_card`.
#[derive(Debug, Default)]
pub struct State {
    /// The questions waiting, in the order they came: the card shows the
    /// first.
    pending: Arc<Vec<ConsentRequest>>,
    /// The requests the person was already told of by a notification.
    told: HashSet<String>,
    /// The main window does not have the system's focus.
    away: bool,
}

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.consent_card`.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::Consent(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The queue changed.
    Pending(Arc<Vec<ConsentRequest>>),
    /// The person's click on Clone or Refuse.
    Answer { id: String, allow: bool },
    /// A window gained or lost the system's focus.
    Focus(window::Id, bool),
}

/// Once per request, and only when the person is away from the window.
pub fn should_notify(notify_on_idle: bool, only_when_away: bool, window_focused: bool) -> bool {
    notify_on_idle && !(only_when_away && window_focused)
}

/// The requests not seen before, remembered from now on. An answered request
/// never comes back, so what is no longer pending is forgotten.
fn newly_seen(told: &mut HashSet<String>, pending: &[ConsentRequest]) -> Vec<ConsentRequest> {
    let fresh = pending
        .iter()
        .filter(|request| told.insert(request.id.clone()))
        .cloned()
        .collect();
    told.retain(|id| pending.iter().any(|request| request.id == *id));
    fresh
}

fn title_of(t: Translator, request: &ConsentRequest) -> String {
    match &request.by {
        Some(by) if !by.is_empty() => t
            .t_with(Key::GitConsentTitle, &args!("by" => by.as_str()))
            .into_owned(),
        _ => t.t(Key::GitConsentTitleUnknown).into_owned(),
    }
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Pending(pending) => {
            let t = Translator::new(m.locale());
            let settings = m.settings();
            let notify = should_notify(
                settings.notify_on_idle,
                settings.notify_only_when_away,
                !st.away,
            );
            // Each request is told once, whether or not a notification was
            // wanted for it: asking again later would be asking for the
            // wrong moment.
            let fresh = newly_seen(&mut st.told, &pending);
            let tasks: Vec<_> = fresh
                .iter()
                .filter(|_| notify)
                .map(|request| {
                    Task::done(Message::Intent(Intent::Notify(Notification {
                        title: title_of(t, request),
                        body: request.url.clone(),
                        target: None,
                    })))
                })
                .collect();
            st.pending = pending;
            Task::batch(tasks)
        }
        Msg::Answer { id, allow } => {
            consent::queue().answer(&id, allow);
            // Show the next question at once, without waiting for the queue's
            // own message.
            st.pending = consent::queue().pending();
            Task::none()
        }
        Msg::Focus(window, focused) => {
            if m.live.windows.get(&window) == Some(&WindowKind::Main) {
                st.away = !focused;
            }
            Task::none()
        }
    }
}

/// The queue's questions, now and at every change.
fn pending_stream() -> impl iced::futures::Stream<Item = Message> {
    let receiver = consent::queue().watch();
    stream::unfold((receiver, true), |(mut receiver, first)| async move {
        if !first {
            receiver.changed().await.ok()?;
        }
        let pending = receiver.borrow_and_update().clone();
        Some((Message::Consent(Msg::Pending(pending)), (receiver, false)))
    })
}

fn focus_events(
    event: iced::Event,
    _status: iced::event::Status,
    window: window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Window(window::Event::Focused) => {
            Some(Message::Consent(Msg::Focus(window, true)))
        }
        iced::Event::Window(window::Event::Unfocused) => {
            Some(Message::Consent(Msg::Focus(window, false)))
        }
        _ => None,
    }
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    Subscription::batch([
        Subscription::run(pending_stream),
        iced::event::listen_with(focus_events),
    ])
}

/// What takes the focus when F6 reaches a region this package owns. It owns
/// none, so the shell never calls it.
pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    None
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    None
}

/// Every core event, after the shell's own handling of it.
pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    None
}

/// The reason, as a sentence that ends once: the log line it comes from may
/// end on dots already.
fn reason_sentence(t: Translator, reason: &str) -> String {
    t.t_with(
        Key::GitConsentNotYours,
        &args!("reason" => reason.trim_end_matches('.')),
    )
    .into_owned()
}

/// The consent card, bottom right of the main window; never takes the
/// focus. Nothing while no clone waits for an answer.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let Some(request) = st.pending.first() else {
        return Space::new().width(0).height(0).into();
    };
    let t = ctx.theme;
    let words = ctx.i18n;

    let fact = |name: Key, value: Element<'a, Message>| -> Element<'a, Message> {
        row![
            container(typography::label(t, ctx.t(name)).size(tokens::font_size::UI_LG))
                .width(Length::Fixed(NAMES)),
            container(value).width(Length::Fill),
        ]
        .spacing(tokens::space::LG)
        .into()
    };
    let mono = |value: &'a str| -> Element<'a, Message> {
        typography::mono(t, value)
            .size(tokens::font_size::UI)
            .wrapping(Wrapping::WordOrGlyph)
            .into()
    };
    let facts = column![
        fact(Key::GitConsentRepo, mono(&request.url)),
        fact(
            Key::GitConsentOwner,
            typography::ui(t, request.owner.as_str())
                .size(tokens::font_size::UI_LG)
                .wrapping(Wrapping::WordOrGlyph)
                .into()
        ),
        fact(Key::GitConsentDestination, mono(&request.destination)),
    ]
    .spacing(tokens::space::XS);

    let mut content = column![
        text(title_of(words, request))
            .size(tokens::font_size::BODY)
            .font(hangar_theme::fonts::ui_semibold()),
        facts,
    ]
    .spacing(tokens::space::LG);
    if !request.reason.is_empty() {
        content = content.push(
            typography::hint(t, reason_sentence(words, &request.reason))
                .size(tokens::font_size::UI_LG),
        );
    }
    content = content.push(
        text(ctx.t(Key::GitConsentWarning))
            .size(tokens::font_size::UI_LG)
            .style(style::text::warning(t)),
    );

    let mut actions = row![].spacing(tokens::space::MD).align_y(Alignment::Center);
    if st.pending.len() > 1 {
        actions = actions.push(typography::hint(
            t,
            words
                .t_with(Key::GitConsentMore, &args!("n" => st.pending.len() - 1))
                .into_owned(),
        ));
    }
    actions = actions
        .push(Space::new().width(Length::Fill))
        .push(
            button::ghost(t, ctx.t(Key::GitConsentRefuse)).on_press(Message::Consent(
                Msg::Answer {
                    id: request.id.clone(),
                    allow: false,
                },
            )),
        )
        .push(
            button::primary(t, ctx.t(Key::GitConsentClone)).on_press(Message::Consent(
                Msg::Answer {
                    id: request.id.clone(),
                    allow: true,
                },
            )),
        );
    content = content.push(actions);

    container(content)
        .padding(tokens::space::XXL - 2.0)
        .width(Length::Fixed(WIDTH.min((ctx.metrics.body_width - 32.0).max(0.0))))
        .style(style::container::toast(t, Tone::Warning))
        .into()
}

#[cfg(test)]
mod tests {
    use hangar_i18n::Locale;

    use super::*;

    fn request(by: Option<&str>) -> ConsentRequest {
        ConsentRequest {
            id: "c1".into(),
            url: "https://github.com/evil/tool.git".into(),
            host: "github.com".into(),
            owner: "evil".into(),
            name: "tool".into(),
            destination: "C:\\dev\\tool".into(),
            reason: "it belongs to someone else.".into(),
            by: by.map(str::to_string),
        }
    }

    #[test]
    fn the_title_names_the_pane_that_asked_or_says_an_agent_did() {
        let en = Translator::new(Locale::En);
        assert_eq!(
            title_of(en, &request(Some("Ava"))),
            "Ava wants to clone a repository"
        );
        assert_eq!(
            title_of(en, &request(None)),
            "An agent wants to clone a repository"
        );
        assert_eq!(
            title_of(Translator::new(Locale::Fr), &request(Some("Ava"))),
            "Ava veut cloner un dépôt"
        );
    }

    #[test]
    fn the_reason_ends_once_whatever_it_ended_with() {
        let en = Translator::new(Locale::En);
        assert_eq!(
            reason_sentence(en, "it belongs to someone else..."),
            "It is not yours: it belongs to someone else."
        );
        assert_eq!(
            reason_sentence(en, "it belongs to someone else"),
            "It is not yours: it belongs to someone else."
        );
    }

    #[test]
    fn a_notification_calls_the_person_only_when_they_asked_for_it_and_are_away() {
        // notifyOnIdle off: never.
        assert!(!should_notify(false, true, false));
        assert!(!should_notify(false, false, false));
        // Only when away: not while the window has the focus.
        assert!(!should_notify(true, true, true));
        assert!(should_notify(true, true, false));
        // Whenever: even with the focus.
        assert!(should_notify(true, false, true));
    }

    #[test]
    fn each_request_is_told_once_and_forgotten_once_answered() {
        let mut told = HashSet::new();
        let a = request(Some("Ava"));
        let b = ConsentRequest {
            id: "c2".into(),
            ..request(Some("Max"))
        };
        let ids = |list: Vec<ConsentRequest>| -> Vec<String> {
            list.into_iter().map(|request| request.id).collect()
        };
        assert_eq!(ids(newly_seen(&mut told, &[a.clone()])), ["c1"]);
        assert!(newly_seen(&mut told, &[a.clone()]).is_empty());
        assert_eq!(ids(newly_seen(&mut told, &[a.clone(), b.clone()])), ["c2"]);
        // The first is answered: only the second stays on the list.
        assert!(newly_seen(&mut told, &[b.clone()]).is_empty());
        assert_eq!(told.len(), 1);
        assert!(newly_seen(&mut told, &[]).is_empty());
        assert!(told.is_empty());
    }
}
