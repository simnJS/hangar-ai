//! Desktop notifications (`lib/notify.ts`). A notification that names a pane
//! comes back clicked as `CoreEvent::NotificationActivated`, which the shell
//! turns into `Intent::FocusPane`.
//!
//! TODO(P0b): the window-focus rules of `notifyOnlyWhenAway` live with the
//! callers (P4's idle notices, P6's needs-you); this only sends.

use iced::Task;

use crate::message::Message;
use crate::services::Services;
use crate::shell::{Notice, Notification};

/// Sends `notification`; a failure becomes a notice.
pub fn send(services: &Services, notification: Notification) -> Task<Message> {
    let host = services.core().host().clone();
    Task::perform(
        crate::services::core::notify(host, notification),
        |result| match result {
            Ok(()) => Message::Noop,
            Err(error) => Message::Intent(crate::message::Intent::Notice(Notice::Problem(error))),
        },
    )
}
