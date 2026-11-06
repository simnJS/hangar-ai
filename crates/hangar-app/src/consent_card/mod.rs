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
//! The next question takes the place of the one answered, under the very same
//! buttons. A click that comes within `SETTLE` of a question appearing there is
//! ignored: the second click of a double-click on Clone would otherwise approve
//! a request nobody has read yet, perhaps from another agent for another
//! repository.
//!
//! The card speaks the person's language, the app's (the setting, else the
//! system's), as `useLocale` did; the trace in the activity log is in the
//! system's, like the rest of that log.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
use crate::shell::{Notification, WindowKind};
use crate::state::Model;

/// The card's width, shrunk to fit a narrow window (`min(420px, 100vw - 32px)`).
const WIDTH: f32 = 420.0;
/// The column of the facts' names.
const NAMES: f32 = 100.0;
/// How long a question has to have been on the card before a click answers it:
/// longer than a double-click, shorter than reading a line.
pub const SETTLE: Duration = Duration::from_millis(500);

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
    /// The question on the card, and since when it is there.
    shown: Option<(String, Instant)>,
}

impl State {
    /// Takes the new queue, and notes when its first question came on the card.
    fn show(&mut self, pending: Arc<Vec<ConsentRequest>>, now: Instant) {
        let head = pending.first().map(|request| request.id.as_str());
        if self.shown.as_ref().map(|(id, _)| id.as_str()) != head {
            self.shown = head.map(|id| (id.to_string(), now));
        }
        self.pending = pending;
    }

    /// Whether a click on `id` at `now` answers it: it is the question on the
    /// card, and has been for `SETTLE` at least.
    fn settled(&self, id: &str, now: Instant) -> bool {
        self.shown
            .as_ref()
            .is_some_and(|(shown, since)| shown == id && now.duration_since(*since) >= SETTLE)
    }
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

pub fn update(st: &mut State, m: &mut Model, _sv: &Services, msg: Msg) -> Task<Message> {
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
            st.show(pending, Instant::now());
            Task::batch(tasks)
        }
        Msg::Answer { id, allow } => {
            let now = Instant::now();
            // A click that came before the question could be read, the second
            // of a double-click above all: not an answer.
            if !st.settled(&id, now) {
                return Task::none();
            }
            consent::queue().answer(&id, allow);
            // Show the next question at once, without waiting for the queue's
            // own message.
            st.show(consent::queue().pending(), now);
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

pub fn subscription(_st: &State, _m: &Model) -> Subscription<Message> {
    Subscription::batch([
        Subscription::run(pending_stream),
        iced::event::listen_with(focus_events),
    ])
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it.
pub fn on_intent(
    _st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    _intent: &Intent,
) -> Option<Task<Message>> {
    None
}

/// Every core event, after the shell's own handling of it.
pub fn on_event(
    _st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    _event: &CoreEvent,
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
        .width(Length::Fixed(
            WIDTH.min((ctx.metrics.body_width - 32.0).max(0.0)),
        ))
        .style(style::container::toast(t, Tone::Warning))
        .into()
}

#[cfg(test)]
mod tests {
    use hangar_i18n::Locale;
    use iced::widget::container;

    use crate::workspace_dialog::testing::{Harness, messages};

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
        assert_eq!(ids(newly_seen(&mut told, std::slice::from_ref(&a))), ["c1"]);
        assert!(newly_seen(&mut told, std::slice::from_ref(&a)).is_empty());
        assert_eq!(ids(newly_seen(&mut told, &[a.clone(), b.clone()])), ["c2"]);
        // The first is answered: only the second stays on the list.
        assert!(newly_seen(&mut told, std::slice::from_ref(&b)).is_empty());
        assert_eq!(told.len(), 1);
        assert!(newly_seen(&mut told, &[]).is_empty());
        assert!(told.is_empty());
    }

    /// The corner of the window the shell puts the card in.
    fn corner<'a>(card: Element<'a, Message>) -> Element<'a, Message> {
        container(card)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(16)
            .align_x(Alignment::End)
            .align_y(Alignment::End)
            .into()
    }

    fn waiting(requests: Vec<ConsentRequest>) -> State {
        State {
            pending: Arc::new(requests),
            ..State::default()
        }
    }

    fn answered(messages: &[Message], id: &str, allow: bool) -> bool {
        messages.iter().any(|message| {
            matches!(message, Message::Consent(Msg::Answer { id: asked, allow: said })
                if asked == id && *said == allow)
        })
    }

    #[test]
    fn the_card_shows_what_would_be_cloned_from_whom_and_where() {
        let h = Harness::new(Locale::En, "graphite");
        let st = waiting(vec![request(Some("Ava"))]);
        let mut ui = h.show(corner(view(&st, h.ctx())));
        for text in [
            "Ava wants to clone a repository",
            "Repository",
            "https://github.com/evil/tool.git",
            "Owner",
            "evil",
            "Cloned into",
            "C:\\dev\\tool",
            "It is not yours: it belongs to someone else.",
            "Installing its dependencies runs its code on this machine. Its orchestrator tells you what the install would run before it installs anything.",
            "Refuse",
            "Clone",
        ] {
            assert!(ui.find(text).is_ok(), "{text} is on the card");
        }
        h.shot(&mut ui, "consent-card-en-graphite");
    }

    #[test]
    fn clone_and_refuse_answer_the_request_on_show() {
        let h = Harness::new(Locale::En, "graphite");
        let st = waiting(vec![request(Some("Ava"))]);
        let mut ui = h.show(corner(view(&st, h.ctx())));
        ui.click("Clone").expect("the button is drawn");
        assert!(answered(&messages(ui), "c1", true));
        let mut ui = h.show(corner(view(&st, h.ctx())));
        ui.click("Refuse").expect("the button is drawn");
        assert!(answered(&messages(ui), "c1", false));
    }

    #[test]
    fn only_the_first_request_is_shown_and_the_others_are_counted() {
        let h = Harness::new(Locale::En, "graphite");
        let second = ConsentRequest {
            id: "c2".into(),
            url: "https://github.com/other/second.git".into(),
            ..request(Some("Max"))
        };
        let st = waiting(vec![request(Some("Ava")), second]);
        let mut ui = h.show(corner(view(&st, h.ctx())));
        assert!(ui.find("Ava wants to clone a repository").is_ok());
        assert!(ui.find("https://github.com/other/second.git").is_err());
        assert!(ui.find("1 more waiting").is_ok());
        ui.click("Clone").expect("the button is drawn");
        assert!(answered(&messages(ui), "c1", true));
    }

    #[test]
    fn a_click_right_after_a_question_appears_does_not_answer_it() {
        let first = request(Some("Ava"));
        let second = ConsentRequest {
            id: "c2".into(),
            url: "https://github.com/other/second.git".into(),
            ..request(Some("Max"))
        };
        let start = Instant::now();
        let mut st = State::default();
        assert!(!st.settled("c1", start), "nothing shown");
        st.show(Arc::new(vec![first.clone(), second.clone()]), start);
        assert!(!st.settled("c1", start));
        assert!(!st.settled("c1", start + SETTLE - Duration::from_millis(1)));
        assert!(st.settled("c1", start + SETTLE));
        assert!(!st.settled("c2", start + SETTLE), "not the one on the card");

        // The first is answered, and the second takes its place under the same
        // buttons: the second click of a double-click lands on it.
        let answered = start + Duration::from_secs(3);
        st.show(Arc::new(vec![second.clone()]), answered);
        assert!(!st.settled("c2", answered + Duration::from_millis(80)));
        assert!(st.settled("c2", answered + SETTLE));

        // The queue's own message with the same head does not restart the wait.
        let later = answered + Duration::from_millis(200);
        st.show(Arc::new(vec![second, first]), later);
        assert!(st.settled("c2", answered + SETTLE));

        st.show(Arc::new(Vec::new()), later);
        assert!(st.shown.is_none());
    }

    #[test]
    fn nothing_is_drawn_while_nothing_waits() {
        let h = Harness::new(Locale::En, "graphite");
        let st = State::default();
        let mut ui = h.show(corner(view(&st, h.ctx())));
        assert!(ui.find("Clone").is_err() && ui.find("Refuse").is_err());
    }

    #[test]
    fn an_unknown_asker_and_an_empty_reason_leave_out_what_they_would_say() {
        let h = Harness::new(Locale::En, "graphite");
        let st = waiting(vec![ConsentRequest {
            reason: String::new(),
            ..request(None)
        }]);
        let mut ui = h.show(corner(view(&st, h.ctx())));
        assert!(ui.find("An agent wants to clone a repository").is_ok());
        assert!(ui.find("It is not yours: .").is_err());
    }

    #[test]
    fn the_card_speaks_french_in_a_light_theme() {
        let h = Harness::new(Locale::Fr, "github-light");
        let st = waiting(vec![request(Some("Ava"))]);
        let mut ui = h.show(corner(view(&st, h.ctx())));
        for text in [
            "Ava veut cloner un dépôt",
            "Dépôt",
            "Propriétaire",
            "Refuser",
            "Cloner",
        ] {
            assert!(ui.find(text).is_ok(), "{text} is on the card");
        }
        h.shot(&mut ui, "consent-card-fr-github-light");
    }
}
