//! P14: Kits management.
//!
//! Ports KitActivation.tsx, settings/KitsSettings.tsx,
//! settings/KitsGallery.tsx,
//! lib/{kits,kitActivation,kitSetup,kitGallery,kitIcon}.ts,
//! strings/kitsManage.ts. Read inventory-frontend.md §2.4/1.1–1.3 and
//! crates/hangar-app/PACKAGES.md. Planned files of this directory (§8.1): mod,
//! model, activation, setup, gallery, icon,
//! views/{settings,gallery,activation,workspace_section}.

use iced::{Element, Subscription, Task};

use hangar_core::kits::KitInfo;

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{Region, placeholder};
use crate::state::{Model, WsId};

pub mod model;
#[cfg(test)]
mod testing;

/// The package's UI state; lives in `App.pkg.kits`.
#[derive(Debug, Default)]
pub struct State;

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.kits`.
///
/// The installed kits: `kits_list`, read once at start and again on every
/// `CoreEvent::KitsChanged` (a kit added, updated or removed, or a linked
/// folder's manifest edited). The kit tabs (P16), the toolbar's Kits menu and
/// the launch of a pane read it instead of asking core each time.
#[derive(Debug, Default)]
pub struct Live {
    /// `None` until the first answer (the toolbar's `kitsLoading`); after
    /// that the list as it stood at the last read, which a failed read keeps.
    pub installed: Option<Vec<KitInfo>>,
    /// Why the last read failed; cleared by the next one that works.
    pub error: Option<String>,
}

impl Live {
    /// The kits as last read, empty before the first read.
    pub fn kits(&self) -> &[KitInfo] {
        self.installed.as_deref().unwrap_or_default()
    }

    /// The installed kit with this id.
    pub fn find(&self, id: &str) -> Option<&KitInfo> {
        self.kits().iter().find(|kit| kit.id == id)
    }

    /// Whether the first read has not answered yet.
    pub fn loading(&self) -> bool {
        self.installed.is_none() && self.error.is_none()
    }
}

/// The package's messages, routed by the shell as `Message::Kits(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// `kits_list` answered.
    Installed(Result<Vec<KitInfo>, String>),
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Installed(result) => {
            match result {
                Ok(list) => {
                    m.live.kits.installed = Some(list);
                    m.live.kits.error = None;
                }
                // A read that fails keeps the list there was.
                Err(error) => m.live.kits.error = Some(error),
            }
            m.live.version += 1;
            Task::none()
        }
    }
}

/// Reads the kits once at start; later reads follow `KitsChanged`.
pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    Subscription::run(first_read)
}

fn first_read() -> impl iced::futures::Stream<Item = Message> {
    iced::futures::stream::once(async {
        Message::Kits(Msg::Installed(hangar_core::kits::kits_list().await))
    })
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
    match event {
        CoreEvent::KitsChanged => Some(Task::perform(hangar_core::kits::kits_list(), |result| {
            Message::Kits(Msg::Installed(result))
        })),
        _ => None,
    }
}

/// `Overlay::KitActivation`: enabling a kit, on a workspace or everywhere.
pub fn activation_view<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    kit: &'a str,
    ws: Option<&'a WsId>,
) -> Element<'a, Message> {
    placeholder::stub_modal(ctx, "kits::activation_view")
}

/// The kits section of the workspace dialog and of `Overlay::KitsDialog`;
/// `ws` is `None` for a workspace being created.
pub fn workspace_section<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ws: Option<&'a WsId>,
) -> Element<'a, Message> {
    placeholder::stub(ctx, "kits::workspace_section")
}

/// Settings › Kits (embedded by the settings).
pub fn settings_view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    placeholder::stub(ctx, "kits::settings_view")
}
