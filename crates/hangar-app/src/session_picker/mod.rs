//! P7: the session picker (`SessionPicker.tsx`, inventory §2.1/7): the
//! conversations a pane's agent can be relaunched on, newest first, with a
//! fresh one at the top.
//!
//! The shell pushes `Overlay::SessionPicker` (`Intent::OpenSessions`); this
//! package follows it in `on_intent`, asking `list_sessions` off the UI
//! thread, and picking one respawns the pane on it (`Intent::ReplacePane`
//! with its `sessionId`).

use std::time::{SystemTime, UNIX_EPOCH};

use iced::keyboard::Key as Pressed;
use iced::keyboard::key::Named;
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::{Id, Space, button, column, container, operation, row, rule, text};
use iced::{Alignment, Element, Length, Subscription, Task};

use hangar_core::sessions::{AgentSession, list_sessions};
use hangar_i18n::{Key, Translator, args};
use hangar_theme::tokens::{font_size, size, space};
use hangar_theme::{fonts, style};
use hangar_ui::icon::{Icon, icon};
use hangar_ui::keys::on_key;
use hangar_ui::menu_nav::{Step, step_index};
use hangar_ui::{button as buttons, dialog};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{Overlay, Region};
use crate::state::types::{AgentId, Pane};
use crate::state::{Model, PaneId, Patch, WsId};

/// The height of a row of the list.
const ROW_HEIGHT: f32 = 30.0;
/// The tallest the body grows before it scrolls (the dialog's own default,
/// named so that the arithmetic below does not depend on it).
const BODY_HEIGHT: f32 = 420.0;

/// The list's scrollable.
fn list_id() -> Id {
    Id::new("session-picker-list")
}

/// How far down the list the top of a row is: the body's padding, the
/// « New session » row, and for a session the hairline between them. The
/// rows are `space::XXS` apart.
fn row_top(row: usize) -> f32 {
    let pad = space::MD;
    if row == 0 {
        return pad;
    }
    let hairline = 2.0 * space::XS + 1.0;
    pad + ROW_HEIGHT
        + space::XXS
        + hairline
        + space::XXS
        + (row - 1) as f32 * (ROW_HEIGHT + space::XXS)
}

/// Where to scroll the list so that the keyboard's row is in view: the row
/// in the middle of the body, which iced stops at the ends of the list. (The
/// web app focused the row's button and the browser scrolled it into view;
/// iced cannot focus a button, and cannot say how far the person scrolled
/// with the wheel, so the row is centred rather than nudged.)
fn offset_for(row: usize) -> f32 {
    (row_top(row) + ROW_HEIGHT / 2.0 - BODY_HEIGHT / 2.0).max(0.0)
}

/// Brings the keyboard's row into view.
fn reveal(row: usize) -> Task<Message> {
    operation::scroll_to(
        list_id(),
        AbsoluteOffset {
            x: None,
            y: Some(offset_for(row)),
        },
    )
}

/// The package's UI state; lives in `App.pkg.session_picker`.
#[derive(Debug, Default)]
pub struct State {
    picker: Option<Picker>,
}

#[derive(Debug, Clone)]
struct Picker {
    ws: WsId,
    pane: PaneId,
    agent: AgentId,
    cwd: String,
    /// The session the pane is on.
    current: Option<String>,
    /// `None` until `list_sessions` answers.
    sessions: Option<Vec<AgentSession>>,
    /// The keyboard's row: 0 is « New session », then the sessions.
    cursor: usize,
}

impl Picker {
    fn is_for(&self, ws: &WsId, pane: &PaneId) -> bool {
        &self.ws == ws && &self.pane == pane
    }

    fn rows(&self) -> usize {
        1 + self.sessions.as_ref().map_or(0, Vec::len)
    }

    /// The session of a row; `None` is a new one.
    fn session_at(&self, row: usize) -> Option<String> {
        let at = row.checked_sub(1)?;
        self.sessions.as_ref()?.get(at).map(|s| s.id.clone())
    }
}

#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::SessionPicker(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    Loaded {
        ws: WsId,
        pane: PaneId,
        sessions: Vec<AgentSession>,
    },
    /// A row, picked: `None` for a new session.
    Pick(Option<String>),
    Step(Step),
    /// Enter: the keyboard's row.
    Choose,
    Close,
}

fn send(msg: Msg) -> Message {
    Message::SessionPicker(msg)
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    let _ = sv;
    let Some(picker) = &mut st.picker else {
        return Task::none();
    };
    match msg {
        Msg::Loaded { ws, pane, sessions } => {
            if !picker.is_for(&ws, &pane) {
                return Task::none();
            }
            // Once the list is in, the keyboard starts on the session the
            // pane is on rather than on « New session »: Return by reflex then
            // keeps the conversation instead of dropping it. Only while the
            // keyboard has not moved on its own.
            if picker.cursor == 0
                && let Some(at) = picker
                    .current
                    .as_ref()
                    .and_then(|id| sessions.iter().position(|s| &s.id == id))
            {
                picker.cursor = at + 1;
            }
            picker.sessions = Some(sessions);
            // The session the pane is on may be far down the list.
            reveal(picker.cursor)
        }
        Msg::Step(step) => {
            if let Some(next) = step_index(Some(picker.cursor), picker.rows(), step, false) {
                picker.cursor = next;
                return reveal(next);
            }
            Task::none()
        }
        Msg::Choose => {
            let id = picker.session_at(picker.cursor);
            pick(st, m, id)
        }
        Msg::Pick(id) => pick(st, m, id),
        Msg::Close => {
            st.picker = None;
            close(m)
        }
    }
}

/// Respawns the pane on the session picked, a new one for `None`.
fn pick(st: &mut State, m: &Model, id: Option<String>) -> Task<Message> {
    let Some(picker) = st.picker.take() else {
        return Task::none();
    };
    let respawn = Task::done(Message::Intent(Intent::ReplacePane {
        ws: picker.ws,
        pane: picker.pane,
        patch: Patch::new(move |pane: &mut Pane| pane.session_id = id.clone()),
    }));
    close(m).chain(respawn)
}

/// Closes the picker, if it is still the overlay on top: Escape may have
/// closed it already.
fn close(m: &Model) -> Task<Message> {
    match m.ui.overlays.last() {
        Some(Overlay::SessionPicker { .. }) => Task::done(Message::Intent(Intent::CloseOverlay)),
        _ => Task::none(),
    }
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    let _ = (st, m);
    Subscription::none()
}

/// The picker owns no region, so the shell never calls this.
pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    let _ = (st, m, region);
    None
}

/// Follows the overlay stack: the picker opens on a pane (and asks for its
/// sessions), or goes.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    let _ = (sv, intent);
    let wanted =
        m.ui.overlays
            .iter()
            .rev()
            .find_map(|overlay| match overlay {
                Overlay::SessionPicker { ws, pane } => Some((ws.clone(), pane.clone())),
                _ => None,
            });
    let Some((ws, pane)) = wanted else {
        st.picker = None;
        return None;
    };
    if st.picker.as_ref().is_some_and(|p| p.is_for(&ws, &pane)) {
        return None;
    }
    let workspace = m.workspace(&ws)?;
    let entry = workspace.panes.iter().find(|entry| entry.id == pane)?;
    let cwd = entry
        .cwd
        .clone()
        .filter(|cwd| !cwd.is_empty())
        .unwrap_or_else(|| workspace.cwd.clone());
    let picker = Picker {
        ws: ws.clone(),
        pane: pane.clone(),
        agent: entry.agent,
        cwd: cwd.clone(),
        current: entry.session_id.clone(),
        sessions: None,
        cursor: 0,
    };
    let agent = picker.agent.as_str().to_string();
    st.picker = Some(picker);
    Some(Task::perform(
        tokio::task::spawn_blocking(move || list_sessions(agent, cwd)),
        move |found| {
            send(Msg::Loaded {
                ws,
                pane,
                // An error is an empty list, as it was.
                sessions: found.unwrap_or_default(),
            })
        },
    ))
}

pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    let _ = (st, m, sv, event);
    None
}

/// How long ago, in words (`relative`).
fn relative(i18n: Translator, now_ms: u64, modified_ms: u64) -> String {
    let minutes = (now_ms.saturating_sub(modified_ms) as f64 / 60_000.0).round() as u64;
    if minutes < 1 {
        return i18n.t(Key::SessionsJustNow).into_owned();
    }
    if minutes < 60 {
        return i18n
            .t_with(Key::SessionsMinutesAgo, &args!("n" => minutes))
            .into_owned();
    }
    let hours = (minutes as f64 / 60.0).round() as u64;
    if hours < 24 {
        return i18n
            .t_with(Key::SessionsHoursAgo, &args!("n" => hours))
            .into_owned();
    }
    let days = (hours as f64 / 24.0).round() as u64;
    i18n.t_with(Key::SessionsDaysAgo, &args!("n" => days))
        .into_owned()
}

/// The interface font in italics, for « untitled ».
fn italic() -> iced::Font {
    iced::Font {
        style: iced::font::Style::Italic,
        ..fonts::ui()
    }
}

/// A session id as the pane header shortens it, with an ellipsis saying
/// there is more of it (`sessionIdLabel`).
fn id_label(id: &str) -> String {
    let short: String = id.chars().take(6).collect();
    if short.len() < id.len() {
        format!("{short}…")
    } else {
        id.to_string()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// `Overlay::SessionPicker`: the sessions a pane can resume.
pub fn view<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ws: &'a WsId,
    pane: &'a PaneId,
) -> Element<'a, Message> {
    let Some(picker) = st.picker.as_ref().filter(|p| p.is_for(ws, pane)) else {
        return Space::new().into();
    };
    let t = ctx.theme;
    let now = now_ms();

    // The current session wears a check, so the list says where the pane is
    // now; the keyboard's row is the accent, like the palette's.
    let line =
        |at: usize, id: Option<String>, label: Element<'a, Message>, meta: Element<'a, Message>| {
            let current = picker.current == id;
            let check: Element<'a, Message> = if current {
                icon(Icon::Check, 13.0).color(t.colors.accent_hi).into()
            } else {
                Space::new().width(13.0).into()
            };
            let content = row![
                container(check).width(16.0).center_x(16.0),
                container(label).width(Length::Fill),
                meta,
            ]
            .spacing(space::MD)
            .align_y(Alignment::Center);
            Element::from(
                button(container(content).center_y(Length::Fill))
                    .padding(iced::Padding {
                        top: 0.0,
                        right: space::LG,
                        bottom: 0.0,
                        left: space::MD,
                    })
                    .width(Length::Fill)
                    .height(30.0)
                    .style(style::button::palette_row(t, picker.cursor == at))
                    .on_press(send(Msg::Pick(id))),
            )
        };
    let chosen = |at: usize| picker.cursor == at;
    let quiet = |at: usize, words: String| -> Element<'a, Message> {
        let words = text(words).size(font_size::UI);
        if chosen(at) {
            words.into()
        } else {
            words.style(style::text::secondary(t)).into()
        }
    };

    let mut body = column![line(
        0,
        None,
        text(ctx.t(Key::SessionsNew)).size(font_size::BODY).into(),
        quiet(0, ctx.t(Key::SessionsNewHint).into_owned()),
    )]
    .spacing(space::XXS);

    match &picker.sessions {
        None => {
            body = body.push(empty_note(ctx, Key::SessionsLoading));
        }
        Some(sessions) if sessions.is_empty() => {
            body = body.push(empty_note(ctx, Key::SessionsEmpty));
        }
        Some(sessions) => {
            body = body.push(
                container(rule::horizontal(1).style(style::rule::hairline(t)))
                    .padding([space::XS, 0.0]),
            );
            for (at, session) in sessions.iter().enumerate() {
                let row_at = at + 1;
                let current = picker.current.as_deref() == Some(session.id.as_str());
                let label: Element<'a, Message> = if session.label.is_empty() {
                    // `<em>` in the web app.
                    let words = text(ctx.t(Key::SessionsUntitled).into_owned())
                        .size(font_size::UI)
                        .font(italic());
                    if chosen(row_at) {
                        words.into()
                    } else {
                        words.style(style::text::secondary(t)).into()
                    }
                } else {
                    let font = if current {
                        fonts::ui_semibold()
                    } else {
                        fonts::ui()
                    };
                    text(session.label.as_str())
                        .size(font_size::BODY)
                        .font(font)
                        .wrapping(text::Wrapping::None)
                        .into()
                };
                // When, in words; which, in the mono face of ids, shortened
                // as the pane header shortens it.
                let id = text(id_label(&session.id))
                    .size(font_size::CAPTION)
                    .font(fonts::mono());
                // The whole id on hover (`title={session.id}`).
                let whole = session.id.clone();
                let meta = row![
                    quiet(
                        row_at,
                        format!("{} · ", relative(ctx.i18n, now, session.modified_ms))
                    ),
                    hangar_ui::tip::over(
                        t,
                        if chosen(row_at) {
                            id
                        } else {
                            id.style(style::text::secondary(t))
                        },
                        whole,
                    ),
                ]
                .align_y(Alignment::Center);
                body = body.push(line(row_at, Some(session.id.clone()), label, meta.into()));
            }
        }
    }

    // Up and Down step through the list without wrapping, Enter picks, so
    // a session is a few keys away.
    let body = on_key(body, |key, modifiers| {
        if modifiers.control() || modifiers.alt() || modifiers.logo() {
            return None;
        }
        match key {
            Pressed::Named(Named::ArrowDown) => Some(send(Msg::Step(Step::Next))),
            Pressed::Named(Named::ArrowUp) => Some(send(Msg::Step(Step::Previous))),
            Pressed::Named(Named::Enter) => Some(send(Msg::Choose)),
            _ => None,
        }
    });

    let agent = picker.agent.info().label;
    let panel = dialog::dialog(
        t,
        ctx.i18n
            .t_with(Key::SessionsTitle, &args!("agent" => agent)),
        body,
    )
    .subtitle(picker.cwd.clone())
    .body_id(list_id())
    .body_max_height(BODY_HEIGHT)
    .width(size::DIALOG_LIST_WIDTH)
    .body_padding([space::MD, space::MD])
    .note(ctx.t(Key::SessionsNote))
    .action(buttons::ghost(t, ctx.t(Key::CreateCancel)).on_press(send(Msg::Close)))
    .close_tip(ctx.t(Key::ModalClose))
    .on_close(send(Msg::Close));
    dialog::over(
        t,
        Space::new().width(Length::Fill).height(Length::Fill),
        Some(panel),
    )
    .into()
}

fn empty_note<'a>(ctx: Ctx<'a>, key: Key) -> Element<'a, Message> {
    container(
        text(ctx.t(key))
            .size(font_size::UI)
            .style(style::text::secondary(ctx.theme)),
    )
    .padding([space::LG, space::LG])
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_i18n::Locale;

    const MINUTE: u64 = 60_000;

    #[test]
    fn ages_read_in_minutes_then_hours_then_days() {
        let en = Translator::new(Locale::En);
        let now = 1_000 * 24 * 60 * MINUTE;
        assert_eq!(relative(en, now, now - 20_000), "just now");
        // A number and its unit are kept together by a no-break space.
        assert_eq!(relative(en, now, now - 5 * MINUTE), "5\u{a0}min ago");
        assert_eq!(relative(en, now, now - 59 * MINUTE), "59\u{a0}min ago");
        assert_eq!(relative(en, now, now - 90 * MINUTE), "2\u{a0}h ago");
        assert_eq!(relative(en, now, now - 30 * 60 * MINUTE), "1\u{a0}d ago");
        let fr = Translator::new(Locale::Fr);
        assert_eq!(relative(fr, now, now - 5 * MINUTE), "il y a 5\u{a0}min");
    }

    #[test]
    fn a_session_id_is_shortened_with_an_ellipsis() {
        assert_eq!(id_label("0f3a9c2e-1111"), "0f3a9c…");
        assert_eq!(id_label("abc"), "abc");
        assert_eq!(id_label("abcdef"), "abcdef");
    }

    fn picker(sessions: Option<Vec<AgentSession>>) -> Picker {
        Picker {
            ws: "w".into(),
            pane: "p".into(),
            agent: AgentId::Claude,
            cwd: "/tmp".into(),
            current: Some("b".into()),
            sessions,
            cursor: 0,
        }
    }

    fn session(id: &str) -> AgentSession {
        AgentSession {
            id: id.into(),
            label: String::new(),
            modified_ms: 0,
        }
    }

    #[test]
    fn row_zero_is_a_new_session_and_the_others_follow_the_list() {
        let open = picker(Some(vec![session("a"), session("b")]));
        assert_eq!(open.rows(), 3);
        assert_eq!(open.session_at(0), None);
        assert_eq!(open.session_at(2).as_deref(), Some("b"));
        assert_eq!(open.session_at(7), None);
        assert_eq!(picker(None).rows(), 1);
    }

    #[test]
    fn the_top_of_the_list_needs_no_scrolling() {
        assert_eq!(offset_for(0), 0.0);
        // Rows that sit in the first screenful stay where they are.
        assert_eq!(offset_for(1), 0.0);
        assert_eq!(offset_for(5), 0.0);
    }

    #[test]
    fn a_row_far_down_is_centred_in_the_body() {
        let row = 40;
        let middle = row_top(row) + ROW_HEIGHT / 2.0;
        assert_eq!(offset_for(row), middle - BODY_HEIGHT / 2.0);
        // And the further down, the further the list is moved, by one row's
        // pitch at a time.
        let pitch = offset_for(row + 1) - offset_for(row);
        assert_eq!(pitch, ROW_HEIGHT + space::XXS);
    }

    #[test]
    fn sessions_start_below_the_new_session_row_and_its_hairline() {
        assert!(row_top(1) > row_top(0) + ROW_HEIGHT);
        assert_eq!(row_top(2) - row_top(1), ROW_HEIGHT + space::XXS);
    }
}
