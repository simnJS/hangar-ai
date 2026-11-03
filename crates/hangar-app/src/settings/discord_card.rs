//! The Discord card of Settings › Discord (`DiscordCard.tsx`): the presence
//! as Discord will render it, and whether the link is live.
//!
//! Worth the pixels: the card is published to other people, and the settings
//! that shape it (name the workspace? name the agents?) are only meaningful
//! once you can see what they leave on screen. The status line answers the
//! other question a toggle cannot: nothing appeared, is that this app or is
//! Discord simply closed?
//!
//! While the category is shown and the feature is on, core is asked how the
//! link stands every two seconds, and the elapsed time of the card moves on
//! its own every second.

use std::sync::LazyLock;
use std::time::Duration;

use hangar_core::discord::{self as core, Presence};
use hangar_i18n::{Key, args};
use iced::widget::{column, container, image, row, stack, text};
use iced::{Alignment, Element, Length, Padding, Subscription, Task};

use hangar_theme::tokens::{font_size, radius, space};
use hangar_theme::{fonts, style};
use hangar_ui::text as typography;

use super::discord::{PresenceInput, build_presence, now_ms};
use crate::ctx::Ctx;
use crate::message::Message;
use crate::services::Services;

/// How often the connection state is re-read while the page is open.
const POLL: Duration = Duration::from_secs(2);

/// The very file Discord fetches from the repository, bundled in: the preview
/// stays right without a round trip, and there is only ever one logo to
/// update.
static LOGO: LazyLock<image::Handle> = LazyLock::new(|| {
    image::Handle::from_bytes(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/logo.png"
        ))
        .as_slice(),
    )
});

#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    /// Ask core how the link stands.
    Poll,
    /// A second went by: the elapsed time moves.
    Tick,
}

#[derive(Debug, Default)]
pub struct State {
    connected: bool,
    error: Option<String>,
    /// Unix milliseconds, for the elapsed time.
    now: i64,
}

/// How the link reads (`dc-status--{tone}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Off,
    On,
    Waiting,
}

pub fn tone(enabled: bool, connected: bool) -> Tone {
    if !enabled {
        Tone::Off
    } else if connected {
        Tone::On
    } else {
        Tone::Waiting
    }
}

/// `hh:mm:ss` since `started_at`, never negative.
pub fn elapsed_since(started_at: i64, now: i64) -> String {
    let total = ((now - started_at) / 1000).max(0);
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        total / 60 % 60,
        total % 60
    )
}

impl State {
    /// The category is shown: how the link stands now.
    pub fn enter(&mut self, sv: &Services, enabled: bool) {
        *self = State {
            now: now_ms(),
            ..State::default()
        };
        self.poll(sv, enabled);
    }

    fn poll(&mut self, sv: &Services, enabled: bool) {
        if !enabled {
            self.connected = false;
            self.error = None;
            return;
        }
        let status = core::discord_presence_status(sv.core().core().discord());
        self.connected = status.connected;
        self.error = status.error;
    }
}

pub fn update(st: &mut State, sv: &Services, enabled: bool, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Poll => st.poll(sv, enabled),
        Msg::Tick => st.now = now_ms(),
    }
    Task::none()
}

/// The clocks, while the card is on screen.
pub fn subscription(enabled: bool) -> Subscription<Message> {
    let tick = iced::time::every(Duration::from_secs(1))
        .map(|_| Message::Settings(super::Msg::Discord(Msg::Tick)));
    if enabled {
        Subscription::batch([
            tick,
            iced::time::every(POLL).map(|_| Message::Settings(super::Msg::Discord(Msg::Poll))),
        ])
    } else {
        tick
    }
}

/// The card, built as if it were on so that the toggles can be judged before
/// publishing anything; it greys out while off. `started_at` is the launch of
/// the app, which is what the presence counts from.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>, started_at: i64) -> Element<'a, Message> {
    let settings = ctx.model.settings();
    let enabled = settings.discord_presence;
    let mut preview_settings = settings.clone();
    preview_settings.discord_presence = true;
    let Some(presence) = build_presence(&PresenceInput {
        settings: &preview_settings,
        workspace: ctx.model.active_workspace(),
        focused_pane: None,
        t: ctx.i18n,
        started_at,
    }) else {
        return column![].into();
    };
    column![
        card(ctx, &presence, st.now, enabled),
        status_line(st, ctx, enabled)
    ]
    .spacing(space::LG)
    .width(Length::Fill)
    .into()
}

fn card<'a>(ctx: Ctx<'a>, presence: &Presence, now: i64, enabled: bool) -> Element<'a, Message> {
    let t = ctx.theme;
    let c = &t.colors;
    // Off is a preview, not a state: dimmed enough to read as inactive,
    // legible enough to judge the toggles by.
    let dim: f32 = if enabled { 1.0 } else { 0.5 };
    let art = {
        let logo = image(LOGO.clone()).width(70).height(70).opacity(dim);
        let framed = container(logo)
            .width(72)
            .height(72)
            .center(Length::Shrink)
            .style(style::container::well(t));
        match presence
            .small_text
            .as_deref()
            .and_then(|name| name.chars().next())
        {
            Some(initial) => {
                let accent = c.accent;
                let ink = c.accent_fg;
                let badge = container(
                    text(initial.to_string())
                        .size(font_size::CAPTION)
                        .font(fonts::ui_semibold())
                        .color(ink),
                )
                .width(22)
                .height(22)
                .center(Length::Shrink)
                .style(move |_: &iced::Theme| container::Style {
                    background: Some(iced::Background::Color(accent)),
                    border: iced::Border {
                        radius: radius::FULL.into(),
                        ..iced::Border::default()
                    },
                    ..container::Style::default()
                });
                stack![
                    framed,
                    container(badge)
                        .width(72)
                        .height(72)
                        .align_x(Alignment::End)
                        .align_y(Alignment::End)
                        .padding(Padding {
                            right: -6.0,
                            bottom: -6.0,
                            ..Padding::ZERO
                        })
                ]
                .into()
            }
            None => Element::from(framed),
        }
    };

    let mut lines = column![
        typography::heading(t, ctx.t(Key::DiscordCardHead).into_owned()),
        typography::ui(t, "Hangar.AI".to_string()).font(fonts::ui_semibold()),
        typography::muted(t, presence.details.clone()),
        typography::muted(t, presence.state.clone()),
        typography::hint(
            t,
            ctx.i18n
                .t_with(
                    Key::DiscordElapsed,
                    &args!("time" => elapsed_since(presence.started_at.unwrap_or(now), now)),
                )
                .into_owned(),
        ),
    ]
    .spacing(2);
    if let Some(label) = &presence.button_label {
        lines = lines.push(
            container(typography::ui(t, label.clone()))
                .padding(Padding::from([3.0, 12.0]))
                .style(style::container::chip(t)),
        );
    }

    container(
        row![art, lines.width(Length::Fill)]
            .spacing(14)
            .align_y(Alignment::Center),
    )
    .padding(14)
    .max_width(400)
    .style(style::container::card(t))
    .into()
}

fn status_line<'a>(st: &'a State, ctx: Ctx<'a>, enabled: bool) -> Element<'a, Message> {
    let t = ctx.theme;
    let c = &t.colors;
    let tone = tone(enabled, st.connected);
    let label = match tone {
        Tone::Off => ctx.t(Key::DiscordStatusOff),
        Tone::On => ctx.t(Key::DiscordStatusConnected),
        Tone::Waiting => ctx.t(Key::DiscordStatusWaiting),
    }
    .into_owned();
    let dot_color = match tone {
        Tone::Off => c.fg_subtle,
        Tone::On => c.success,
        Tone::Waiting => c.warning,
    };
    let dot = container(iced::widget::Space::new())
        .width(7)
        .height(7)
        .style(move |_: &iced::Theme| container::Style {
            background: (tone != Tone::Off).then_some(iced::Background::Color(dot_color)),
            border: iced::Border {
                radius: radius::FULL.into(),
                width: if tone == Tone::Off { 1.5 } else { 0.0 },
                color: dot_color,
            },
            ..container::Style::default()
        });
    let words = if tone == Tone::On {
        typography::ui(t, label).style(style::text::primary(t))
    } else {
        typography::ui(t, label).style(style::text::muted(t))
    };
    let mut line = row![dot, words]
        .spacing(space::LG)
        .align_y(Alignment::Center);
    if tone == Tone::Waiting {
        // The error of the link is jargon on its own line; it earns a tip, not
        // a row.
        let hint = typography::hint(t, ctx.t(Key::DiscordStatusWaitingHint).into_owned());
        line = line.push(match &st.error {
            Some(error) => hangar_ui::tip::over(t, hint, error.clone()),
            None => hint.into(),
        });
    }
    container(line)
        .padding(Padding::from([0.0, space::XS]))
        .into()
}

#[cfg(test)]
mod tests;
