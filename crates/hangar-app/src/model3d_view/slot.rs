//! What the 3D view holds for each model that is open: its viewer (the model in
//! memory, the camera, the display toggles), the load under way and how it is
//! going, and the word after the last capture.
//!
//! One slot per entry of the preview store, whichever workspace it is in and
//! whether it is on screen: the old window had one three.js scene for every
//! model of every workspace (`ModelView`), and a model an agent opens in a
//! workspace out of sight loads all the same, so the agent hears what the
//! viewer found and the person finds it ready.

use hangar_model3d::{Cancel, LoadProgress, ModelViewer};

use crate::preview::parts::Notice;
use crate::state::WsId;

/// A load that has begun: the folders its side files may come from are being
/// worked out, then the file is read on a thread of its own.
pub struct Flight {
    /// The entry's revision it loads: a newer one replaces it.
    pub revision: u64,
    /// Stops the read; absent while the folders are being worked out.
    pub cancel: Option<Cancel>,
    /// What the read last reported.
    pub progress: Option<LoadProgress>,
}

impl Flight {
    pub fn new(revision: u64) -> Flight {
        Flight {
            revision,
            cancel: None,
            progress: None,
        }
    }

    /// Stops the read, if it has started.
    pub fn cancel(&self) {
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
        }
    }
}

pub struct Slot {
    /// The workspace the model is open in.
    pub ws: WsId,
    pub viewer: ModelViewer,
    pub flight: Option<Flight>,
    /// The revision of the model the viewer holds; `None` while it holds none
    /// (a first load, or a model dropped to make room for others).
    pub loaded: Option<u64>,
    /// The revision that could not be loaded. It is not tried again until the
    /// file changes or the person asks (`failed` of `useModelLoads`): a
    /// broken file would otherwise be read again after every change of the
    /// store.
    pub failed: Option<u64>,
    /// The capture menu is open.
    pub menu: bool,
    /// The menu listing the animation clips is open.
    pub clips_menu: bool,
    /// A word after a capture, with the number that ends it.
    pub notice: Option<(u64, Notice)>,
    /// When it was last in front, on the view's own clock: what decides which
    /// models are dropped when there are too many.
    pub used: u64,
}

impl Slot {
    pub fn new(ws: WsId, display: hangar_model3d::DisplayOptions, reduced_motion: bool) -> Slot {
        let mut viewer = ModelViewer::new();
        viewer.set_display(display);
        viewer.set_reduced_motion(reduced_motion);
        Slot {
            ws,
            viewer,
            flight: None,
            loaded: None,
            failed: None,
            menu: false,
            clips_menu: false,
            notice: None,
            used: 0,
        }
    }

    /// The model is in memory and can be drawn.
    pub fn has_content(&self) -> bool {
        self.viewer.model().is_some()
    }

    /// Whether a load of `revision` is on its way.
    pub fn loading(&self, revision: u64) -> bool {
        self.flight
            .as_ref()
            .is_some_and(|flight| flight.revision == revision)
    }

    /// Stops the load under way and forgets it.
    pub fn abandon(&mut self) {
        if let Some(flight) = self.flight.take() {
            flight.cancel();
        }
    }

    /// Lets go of the model, keeping the slot (and with it the person's
    /// display choices).
    pub fn drop_model(&mut self) {
        self.viewer.clear();
        self.loaded = None;
    }
}

impl std::fmt::Debug for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Slot")
            .field("ws", &self.ws)
            .field("loaded", &self.loaded)
            .field("failed", &self.failed)
            .field(
                "loading",
                &self.flight.as_ref().map(|flight| flight.revision),
            )
            .finish_non_exhaustive()
    }
}
