//! Toasts: short notices that float over the window's corner and go away by
//! themselves.
//!
//! The state is a [`Toasts`] the app keeps; the view lays them over the page.
//!
//! ```ignore
//! use hangar_ui::toast::{self, Toast, Toasts};
//!
//! // In `update`:
//! self.toasts.push(Toast::success("Workspace saved").detail("storefront"), Instant::now());
//! Msg::ToastTick(now) => self.toasts.tick(now),
//! Msg::ToastDismiss(id) => self.toasts.dismiss(id),
//!
//! // In `subscription`: runs only while there is something to expire.
//! self.toasts.subscription().map(Msg::ToastTick)
//!
//! // In `view`:
//! toast::over(t, page, &self.toasts, Msg::ToastDismiss)
//! ```
//!
//! A toast goes after a few seconds, longer the more it matters: a warning
//! stays 7 s and an error 10 s. One with an action ("Undo") stays at least as
//! long as an error, since someone has to be able to reach it. At most five
//! show at once; a sixth pushes the oldest out.

use std::time::{Duration, Instant};

use hangar_theme::style::Tone;
use hangar_theme::tokens::space;
use hangar_theme::{Theme, style};
use iced::widget::{column, container, row, stack};
use iced::{Alignment, Element, Length, Padding, Subscription};

use crate::button::{icon_button, secondary};
use crate::icon::{Icon, icon};
use crate::text as typography;

/// The most toasts on screen at once.
pub const MAX_VISIBLE: usize = 5;

/// How wide a toast is.
const WIDTH: f32 = 340.0;

/// One notice.
#[derive(Debug, Clone)]
pub struct Toast<M> {
    pub tone: Tone,
    pub message: String,
    pub detail: Option<String>,
    pub action: Option<(String, M)>,
    /// How long it stays, or `None` for until it is dismissed.
    pub lifetime: Option<Duration>,
}

impl<M> Toast<M> {
    fn new(tone: Tone, message: impl Into<String>) -> Toast<M> {
        Toast {
            tone,
            message: message.into(),
            detail: None,
            action: None,
            lifetime: Some(match tone {
                Tone::Neutral | Tone::Accent | Tone::Success => Duration::from_secs(4),
                Tone::Warning => Duration::from_secs(7),
                Tone::Danger => Duration::from_secs(10),
            }),
        }
    }

    /// A plain fact.
    pub fn info(message: impl Into<String>) -> Toast<M> {
        Toast::new(Tone::Neutral, message)
    }

    pub fn success(message: impl Into<String>) -> Toast<M> {
        Toast::new(Tone::Success, message)
    }

    pub fn warning(message: impl Into<String>) -> Toast<M> {
        Toast::new(Tone::Warning, message)
    }

    pub fn error(message: impl Into<String>) -> Toast<M> {
        Toast::new(Tone::Danger, message)
    }

    /// A second line in the quieter text.
    pub fn detail(mut self, detail: impl Into<String>) -> Toast<M> {
        self.detail = Some(detail.into());
        self
    }

    /// A button after the message. Pressing it sends `message`; dismissing
    /// the toast is the app's to do in answer, as for any other message.
    pub fn action(mut self, label: impl Into<String>, message: M) -> Toast<M> {
        self.action = Some((label.into(), message));
        let floor = Duration::from_secs(10);
        self.lifetime = self.lifetime.map(|lifetime| lifetime.max(floor));
        self
    }

    /// Stays until it is dismissed.
    pub fn sticky(mut self) -> Toast<M> {
        self.lifetime = None;
        self
    }

    pub fn lifetime(mut self, lifetime: Duration) -> Toast<M> {
        self.lifetime = Some(lifetime);
        self
    }
}

/// A toast on screen, with the identity that dismisses it and the moment it
/// goes.
#[derive(Debug, Clone)]
pub struct Shown<M> {
    pub id: u64,
    pub toast: Toast<M>,
    expires: Option<Instant>,
}

/// The toasts on screen.
#[derive(Debug, Clone)]
pub struct Toasts<M> {
    shown: Vec<Shown<M>>,
    next: u64,
}

impl<M> Default for Toasts<M> {
    fn default() -> Self {
        Toasts {
            shown: Vec::new(),
            next: 0,
        }
    }
}

impl<M: Clone> Toasts<M> {
    pub fn new() -> Toasts<M> {
        Toasts::default()
    }

    /// Shows a toast, starting its lifetime at `now`. Returns its identity.
    pub fn push(&mut self, toast: Toast<M>, now: Instant) -> u64 {
        let id = self.next;
        self.next += 1;
        let expires = toast.lifetime.map(|lifetime| now + lifetime);
        self.shown.push(Shown { id, toast, expires });
        if self.shown.len() > MAX_VISIBLE {
            self.shown.remove(0);
        }
        id
    }

    pub fn dismiss(&mut self, id: u64) {
        self.shown.retain(|shown| shown.id != id);
    }

    /// Removes the toasts whose time has come.
    pub fn tick(&mut self, now: Instant) {
        self.shown
            .retain(|shown| shown.expires.is_none_or(|expires| expires > now));
    }

    /// The soonest moment a toast goes, if any will.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.shown.iter().filter_map(|shown| shown.expires).min()
    }

    pub fn len(&self) -> usize {
        self.shown.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shown.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Shown<M>> {
        self.shown.iter()
    }

    /// A clock that runs while a toast is waiting to go, and not otherwise:
    /// map its instants to the message that calls [`tick`](Toasts::tick).
    pub fn subscription(&self) -> Subscription<Instant> {
        if self.next_deadline().is_some() {
            iced::time::every(Duration::from_millis(250))
        } else {
            Subscription::none()
        }
    }

    /// The toasts as a stack in the window's bottom-right corner, newest at
    /// the bottom. Empty (and drawing nothing) when there are none.
    pub fn view<'a>(&'a self, t: &'a Theme, on_dismiss: impl Fn(u64) -> M + 'a) -> Element<'a, M>
    where
        M: 'a,
    {
        let mut toasts = column![].spacing(space::MD).align_x(Alignment::End);
        for shown in &self.shown {
            toasts = toasts.push(card(t, shown, on_dismiss(shown.id)));
        }
        container(toasts)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::End)
            .align_y(Alignment::End)
            .padding(space::XXL)
            .into()
    }
}

fn card<'a, M: Clone + 'a>(t: &'a Theme, shown: &'a Shown<M>, dismiss: M) -> Element<'a, M> {
    let toast = &shown.toast;
    let glyph = match toast.tone {
        Tone::Success => Icon::Check,
        Tone::Warning | Tone::Danger => Icon::Alert,
        Tone::Accent | Tone::Neutral => Icon::Info,
    };
    let tint = style::container::tone_text(t, toast.tone);
    let mut words = column![typography::body(t, toast.message.clone())].spacing(2);
    if let Some(detail) = &toast.detail {
        words = words.push(typography::hint(t, detail.clone()));
    }
    let mut line = row![
        container(icon(glyph, 14.0).color(tint)).padding(Padding {
            top: 1.0,
            ..Padding::ZERO
        }),
        words.width(Length::Fill),
    ]
    .spacing(space::LG)
    .align_y(Alignment::Start);
    if let Some((label, message)) = &toast.action {
        line = line.push(
            secondary(t, label.clone())
                .small()
                .on_press(message.clone()),
        );
    }
    line = line.push(
        icon_button(t, Icon::Close)
            .small()
            .icon_size(10.0)
            .on_press(dismiss),
    );
    container(line)
        .width(WIDTH)
        .padding(Padding::from([space::LG, space::XL]))
        .style(style::container::toast(t, toast.tone))
        .into()
}

/// `page` with the toasts over it.
pub fn over<'a, M: Clone + 'a>(
    t: &'a Theme,
    page: impl Into<Element<'a, M>>,
    toasts: &'a Toasts<M>,
    on_dismiss: impl Fn(u64) -> M + 'a,
) -> Element<'a, M> {
    if toasts.is_empty() {
        return page.into();
    }
    stack![page.into(), toasts.view(t, on_dismiss)]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests;
