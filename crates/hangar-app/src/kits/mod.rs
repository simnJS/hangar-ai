//! P14: Kits management.
//!
//! Ports KitActivation.tsx, settings/KitsSettings.tsx,
//! settings/KitsGallery.tsx,
//! lib/{kits,kitActivation,kitSetup,kitGallery,kitIcon}.ts,
//! strings/kitsManage.ts (inventory §2.4/1.1–1.3, PACKAGES.md).
//!
//! A kit is a folder with a `hangar-kit.json`, linked from disk or cloned from
//! a GitHub link, that brings a domain into Hangar: a Claude Code plugin,
//! tabs of its own, setup commands (docs/kits.md). This package is where a
//! person adds one (from a folder, from a GitHub link, from the library),
//! updates and removes it, reads what it will run and says yes to it, once for
//! a workspace or for all of them.
//!
//! What it gives the shell and the other packages:
//!
//! - `Live.installed`, the kits as `kits_list` says, refreshed on
//!   `CoreEvent::KitsChanged` (P16, the toolbar's Kits menu and the launch of
//!   a pane read it);
//! - `workspace_section` for the workspace dialog and `Overlay::KitsDialog`
//!   (P8), whose kits for a workspace being created are held here until it is
//!   made (`draft_kits`);
//! - `settings_view` for Settings › Kits (P13);
//! - `activation_view` for `Overlay::KitActivation`;
//! - `icon::kit_icon`, a kit's own icon at a size.
//!
//! Files: `model` (permissions, grants, which kits a pane starts with),
//! `activation` (the diff between what a kit asks and what a workspace
//! accepted), `setup` (typing a setup command into a pane), `gallery` (the
//! library's versions and filter), `icon` (reading and drawing the icon),
//! `update` (the messages), `views/` (the screens).

use std::collections::HashMap;
use std::path::PathBuf;

use iced::{Element, Subscription, Task};

use hangar_core::kits::{Inspection, KitInfo, StepStatus, UpdateInfo};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{Overlay, SettingsCategory};
use crate::state::types::WorkspaceKit;
use crate::state::{Model, PaneId, WsId};

pub mod activation;
pub mod gallery;
#[cfg(test)]
mod guarded;
pub mod icon;
pub mod model;
pub mod setup;
#[cfg(test)]
mod testing;
mod update;
mod views;

use gallery::Gallery;
use icon::IconBytes;
use setup::SetupRunResult;
pub use views::activation_model::StepRun;

/// What a kit's row says about its update, once asked (`UpdateNote`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateNote {
    Checking,
    None {
        version: String,
    },
    Available {
        current: String,
        latest: String,
    },
    Updating,
    Done {
        from: String,
        to: String,
        new_permissions: usize,
    },
    Folder,
    Reloaded {
        version: String,
    },
    Error(String),
}

/// Which way a kit is being added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adding {
    Folder,
    Git,
}

/// The last library install that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GalleryFailure {
    pub id: String,
    pub name: String,
    pub detail: String,
}

/// Settings › Kits: what the page remembers while it is open (`KitsSettings`).
#[derive(Debug, Default)]
pub struct Page {
    /// A confirmation after adding a kit.
    flash: Option<String>,
    notes: HashMap<String, UpdateNote>,
    /// The kit whose removal waits for its second click.
    removing: Option<String>,
    adding: Option<Adding>,
    folder_error: Option<String>,
    git_url: String,
    git_error: Option<String>,
    gallery: Option<Gallery>,
    gallery_error: Option<String>,
    gallery_refreshing: bool,
    gallery_query: String,
    /// The library kit being installed, or updated.
    gallery_busy: Option<String>,
    gallery_failure: Option<GalleryFailure>,
    /// The name of the kit whose change running agents have not got yet.
    restart_for: Option<String>,
}

/// What enabling a kit is for: which list the answer is written to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A workspace that exists: its own kits.
    Workspace(WsId),
    /// The kits of a workspace being created, held here until it is made.
    Draft,
    /// Every workspace (`Settings.kitsEverywhere`).
    Everywhere,
}

/// The activation dialog while it is open (`KitActivation`).
#[derive(Debug)]
pub struct Activation {
    kit: String,
    target: Target,
    /// Where a setup step runs: the workspace itself; the active one when the
    /// kit is being enabled everywhere; none while a workspace is being
    /// created, which has no pane to run in yet.
    ws: Option<WsId>,
    /// `None` until `kit_inspect` has answered.
    inspection: Option<Inspection>,
    /// `None` until `kit_setup_status` has answered.
    steps: Option<Vec<StepStatus>>,
    /// The step whose pane is being opened.
    running: Option<String>,
    /// A step that ran and awaits "it worked".
    ran: Option<StepRun>,
    error: Option<String>,
    /// The hashes the kit had when it was inspected; a change asks again, so
    /// that what is shown is what is accepted.
    seen: (Option<String>, Option<String>),
}

impl Activation {
    /// The workspace a setup step can run in.
    pub fn setup_workspace(&self) -> Option<&WsId> {
        self.ws.as_ref()
    }

    /// What the target holds for this kit now.
    pub fn entry(&self, m: &Model, draft: &[WorkspaceKit]) -> Option<WorkspaceKit> {
        let own = match &self.target {
            Target::Workspace(ws) => model::workspace_kits(m.workspace(ws)),
            Target::Draft => draft.to_vec(),
            Target::Everywhere => model::everywhere_kits(Some(m.settings())),
        };
        own.into_iter().find(|entry| entry.id == self.kit)
    }
}

/// The restart notice of a workspace's kits section.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Restart {
    ws: WsId,
    kit_name: String,
}

/// The package's UI state; lives in `App.pkg.kits`.
#[derive(Debug, Default)]
pub struct State {
    /// The kits of a workspace being created: the workspace dialog reads them
    /// with [`draft_kits`] when it makes the workspace. Emptied when the
    /// dialog opens.
    draft: Vec<WorkspaceKit>,
    /// A kit just enabled on a workspace whose agents may be running.
    restart: Option<Restart>,
    /// The workspace whose panes were just restarted.
    restarted: Option<WsId>,
    activation: Option<Activation>,
    page: Page,
}

/// The kits chosen for a workspace that is being created, as the section of
/// the workspace dialog holds them. Written to the new workspace only when
/// there are some (`kits` is absent rather than empty).
pub fn draft_kits(st: &State) -> &[WorkspaceKit] {
    &st.draft
}

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
    /// The kits' own icons, read from their folders (`icon::kit_icon` draws one).
    pub icons: icon::Icons,
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
    /// A kit's icon was read.
    IconLoaded {
        id: String,
        key: String,
        result: Result<IconBytes, String>,
    },
    /// Puts a command on the clipboard.
    Copy(String),

    // -- the section of a workspace's kits ------------------------------------------
    /// The switch of a kit: on asks first (the activation), off applies at
    /// once. `ws` is `None` for a workspace being created.
    Toggle {
        ws: Option<WsId>,
        kit: String,
        on: bool,
    },
    /// « Details » and « Review »: the activation of a kit already enabled.
    Open {
        ws: Option<WsId>,
        kit: String,
    },
    /// The switch of a kit that is enabled but no longer installed: off.
    RemoveGone {
        ws: Option<WsId>,
        kit: String,
    },
    RestartPanes(WsId),
    DismissRestart,

    // -- the activation dialog ---------------------------------------------------------
    Inspected(String, Result<Box<Inspection>, String>),
    StepsRead(String, Result<Vec<StepStatus>, String>),
    Accept,
    /// Run a setup step: select the workspace, add a pane, type the command.
    Run(String),
    /// The workspace is the one on screen: add the pane.
    SetupPane {
        kit: String,
        step: String,
    },
    /// The command was typed, or could not be.
    SetupDone {
        kit: String,
        step: String,
        ws: WsId,
        pane: PaneId,
        pane_name: String,
        result: SetupRunResult,
    },
    Confirm(String),
    Marked(String, Result<Vec<StepStatus>, String>),
    NotYet,

    // -- Settings › Kits ----------------------------------------------------------------
    AddFolder,
    FolderPicked(Option<PathBuf>),
    FolderAdded(Result<Box<KitInfo>, String>),
    GitUrl(String),
    AddGit,
    GitAdded(Result<Box<KitInfo>, String>),
    Check(String),
    Checked(String, Result<Option<UpdateInfo>, String>),
    Update(String),
    Updated {
        id: String,
        before: Box<KitInfo>,
        result: Result<Box<KitInfo>, String>,
    },
    AskRemove(Option<String>),
    Remove(String),
    Removed(String, Result<(), String>),
    ToggleEverywhere {
        kit: String,
        on: bool,
    },
    ReviewEverywhere(String),
    DismissEverywhereRestart,
    ManageWorkspace(WsId),

    // -- the library -----------------------------------------------------------------------
    Gallery(Result<Gallery, String>),
    GalleryRefresh,
    GalleryQuery(String),
    GalleryInstall(String),
    GalleryInstalled {
        id: String,
        name: String,
        result: Result<Box<KitInfo>, String>,
    },
    GalleryUpdate(String),
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    update::update(st, m, &update::Env::of(sv), msg)
}

/// Reads the kits once at start, and the library while Settings › Kits is
/// open (the web app read it when the page mounted).
pub fn subscription(_st: &State, m: &Model) -> Subscription<Message> {
    let mut subscriptions = vec![Subscription::run(first_read)];
    if m.ui
        .overlays
        .contains(&Overlay::Settings(SettingsCategory::Kits))
    {
        subscriptions.push(Subscription::run(read_gallery));
    }
    Subscription::batch(subscriptions)
}

fn first_read() -> impl iced::futures::Stream<Item = Message> {
    iced::futures::stream::once(async {
        Message::Kits(Msg::Installed(hangar_core::kits::kits_list().await))
    })
}

fn read_gallery() -> impl iced::futures::Stream<Item = Message> {
    iced::futures::stream::once(async {
        Message::Kits(Msg::Gallery(gallery::kit_gallery_list().await))
    })
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    _sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    update::on_intent(st, m, intent)
}

/// Every core event, after the shell's own handling of it.
pub fn on_event(
    _st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    match event {
        CoreEvent::KitsChanged => Some(update::read_installed()),
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
    views::activation::view(st, ctx, kit, ws)
}

/// The kits section of the workspace dialog and of `Overlay::KitsDialog`;
/// `ws` is `None` for a workspace being created.
pub fn workspace_section<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ws: Option<&'a WsId>,
) -> Element<'a, Message> {
    views::workspace_section::view(st, ctx, ws)
}

/// Settings › Kits (embedded by the settings).
pub fn settings_view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    views::settings::view(st, ctx)
}
