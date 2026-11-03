//! The notice line at the bottom of the window, and desktop notifications
//! (inventory §1.11, "Notices").
//!
//! A notice replaces the previous one and clears itself after six seconds:
//! each gets an id, and the `NoticeExpired(id)` scheduled with it clears it
//! only if it is still the one shown.

use std::time::Duration;

use iced::widget::{container, row, text};
use iced::{Element, Task};

use hangar_theme::{style, tokens};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::{Model, PaneId, WsId};

pub const NOTICE_DURATION: Duration = Duration::from_secs(6);

/// The texts come from `control.*`, `orch.*`, `hibernate.*`, `manager.*`,
/// `needsYou.*`, `window.*`, `voice.*`, already translated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// A problem: drawn with a warning mark.
    Problem(String),
    /// A workspace went to sleep: a moon.
    Asleep(String),
    /// An agent or Hangar acted on the layout.
    Agent(String),
}

impl Notice {
    pub fn text(&self) -> &str {
        match self {
            Notice::Problem(text) | Notice::Asleep(text) | Notice::Agent(text) => text,
        }
    }
}

/// A desktop notification (`notify_send`); a click on one that names a pane
/// comes back as `Intent::FocusPane`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub target: Option<(WsId, PaneId)>,
}

/// Shows `notice`, and schedules its end.
pub fn show(model: &mut Model, notice: Notice) -> Task<Message> {
    let id = model.ui.notice.as_ref().map_or(1, |(id, _)| id + 1);
    model.ui.notice = Some((id, notice));
    Task::perform(tokio::time::sleep(NOTICE_DURATION), move |()| {
        Message::NoticeExpired(id)
    })
}

/// Clears the notice `id`, unless another replaced it meanwhile.
pub fn expire(model: &mut Model, id: u64) {
    if model
        .ui
        .notice
        .as_ref()
        .is_some_and(|(shown, _)| *shown == id)
    {
        model.ui.notice = None;
    }
}

/// The pill, when there is a notice.
///
/// TODO(hangar-ui): the pill widget and the warning, moon and agent glyphs.
pub fn view<'a>(ctx: Ctx<'a>) -> Option<Element<'a, Message>> {
    let (_, notice) = ctx.model.ui.notice.as_ref()?;
    let theme = ctx.theme;
    let line = row![text(notice.text()).size(tokens::font_size::UI)]
        .spacing(tokens::space::SM)
        .padding([tokens::space::SM, tokens::space::XL]);
    Some(
        container(line)
            .style(style::container::tooltip(theme))
            .into(),
    )
}
