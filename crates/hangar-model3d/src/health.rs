//! Whether the viewport can show anything, and if it cannot, why.
//!
//! The drawing happens where the widget cannot report from: iced calls the
//! widget's `prepare` while it presents the frame, with a device it owns, and
//! never on iced's software renderer at all. So what happens there is written
//! to a cell the viewer shares with the widget, and the app reads it back with
//! [`ModelViewer::rendering`](crate::ModelViewer::rendering) to put a sentence
//! over the blank viewport (the old viewer's "3D rendering is not available
//! here" and "the graphics card dropped the 3D view").

use std::sync::{Arc, Mutex, MutexGuard};

/// How the viewport is drawing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rendering {
    /// Nothing has been drawn yet, or the viewer was just restarted: no
    /// verdict.
    Starting,
    /// The window's GPU draws the model.
    Drawing,
    /// The window is drawn by iced's software renderer, which has no way to
    /// run the 3D drawing: the viewport stays empty. Pictures taken for the
    /// capture button and for agents still work, they bring their own device.
    Software,
    /// The GPU could not be set up for the 3D view; the reason is the
    /// device's own words.
    Unavailable(String),
    /// The GPU refused this model (a texture it cannot take, no memory left);
    /// the reason is the device's own words. The next model starts afresh.
    Failed(String),
    /// The device was lost: a reset, a driver crash, an unplugged GPU.
    Lost(String),
}

impl Rendering {
    /// The viewport shows nothing, and there is something to tell.
    pub fn is_problem(&self) -> bool {
        !matches!(self, Rendering::Starting | Rendering::Drawing)
    }
}

/// What the window's renderer found when it ran the widget's `prepare`.
#[derive(Debug)]
pub(crate) enum Prepared {
    /// The picture was made; textures may still be on their way.
    Drew {
        uploading: bool,
    },
    Unavailable(String),
    Failed(String),
    Lost(String),
}

/// What the widget is to do after looking.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Review {
    /// The verdict changed since the app was last told: publish an event so
    /// it looks again.
    pub changed: bool,
    /// Another frame is wanted: there is no verdict yet, or textures are
    /// still going up.
    pub redraw: bool,
}

/// The widget has made this many pictures' worth of primitives, and the
/// window's renderer has run none of them, before the window is called
/// software-drawn. Draw and present are one step in iced, so even one would
/// do; three leaves room for a frame lost to a resize.
const SOFTWARE_AFTER: u32 = 3;

#[derive(Debug)]
struct State {
    rendering: Rendering,
    /// Primitives the widget made, and `prepare`s the renderer ran.
    draws: u32,
    prepares: u32,
    uploading: bool,
    /// What the app was last told.
    told: Rendering,
}

#[derive(Debug)]
pub(crate) struct Health(Mutex<State>);

impl Health {
    pub fn new() -> Arc<Health> {
        Arc::new(Health(Mutex::new(State {
            rendering: Rendering::Starting,
            draws: 0,
            prepares: 0,
            uploading: false,
            told: Rendering::Starting,
        })))
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn rendering(&self) -> Rendering {
        self.lock().rendering.clone()
    }

    /// The model on show was refused, and nothing has changed since: drawing
    /// it again would only fail again.
    pub fn failed(&self) -> bool {
        matches!(self.lock().rendering, Rendering::Failed(_))
    }

    /// The widget made a primitive for a frame.
    pub fn drawn(&self) {
        let mut state = self.lock();
        state.draws = state.draws.saturating_add(1);
    }

    /// The window's renderer ran `prepare`.
    pub fn prepared(&self, outcome: Prepared) {
        let mut state = self.lock();
        state.prepares = state.prepares.saturating_add(1);
        state.uploading = false;
        match outcome {
            Prepared::Drew { uploading } => {
                state.uploading = uploading;
                state.rendering = Rendering::Drawing;
            }
            Prepared::Unavailable(reason) => state.rendering = Rendering::Unavailable(reason),
            Prepared::Failed(reason) => state.rendering = Rendering::Failed(reason),
            Prepared::Lost(reason) => state.rendering = Rendering::Lost(reason),
        }
    }

    /// Looks at what has happened since the last look.
    pub fn review(&self) -> Review {
        let mut state = self.lock();
        if state.rendering == Rendering::Starting
            && state.prepares == 0
            && state.draws >= SOFTWARE_AFTER
        {
            state.rendering = Rendering::Software;
        }
        let changed = state.rendering != state.told;
        if changed {
            state.told = state.rendering.clone();
        }
        Review {
            changed,
            redraw: state.uploading || (state.rendering == Rendering::Starting && state.draws > 0),
        }
    }

    /// A new model is on show: a failure to draw the last one is not its.
    pub fn new_model(&self) {
        let mut state = self.lock();
        if matches!(state.rendering, Rendering::Failed(_)) {
            state.rendering = Rendering::Starting;
        }
    }

    /// Starts over: the verdict is forgotten and drawn again.
    pub fn restart(&self) {
        let mut state = self.lock();
        state.rendering = Rendering::Starting;
        state.draws = 0;
        state.prepares = 0;
        state.uploading = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn says_nothing_until_something_has_been_drawn() {
        let health = Health::new();
        assert_eq!(health.rendering(), Rendering::Starting);
        assert_eq!(
            health.review(),
            Review {
                changed: false,
                redraw: false
            }
        );
    }

    #[test]
    fn keeps_asking_for_frames_until_it_knows() {
        let health = Health::new();
        health.drawn();
        assert!(health.review().redraw, "a verdict is still wanted");
        health.prepared(Prepared::Drew { uploading: false });
        let review = health.review();
        assert_eq!(health.rendering(), Rendering::Drawing);
        assert!(review.changed, "the app is told it draws");
        assert!(!review.redraw, "and nothing more is wanted");
        assert!(!health.review().changed, "told once");
    }

    #[test]
    fn calls_a_window_whose_renderer_never_runs_prepare_software_drawn() {
        let health = Health::new();
        for frame in 1..SOFTWARE_AFTER {
            health.drawn();
            let review = health.review();
            assert_eq!(health.rendering(), Rendering::Starting, "frame {frame}");
            assert!(review.redraw && !review.changed);
        }
        health.drawn();
        let review = health.review();
        assert_eq!(health.rendering(), Rendering::Software);
        assert!(review.changed);
        assert!(
            !review.redraw,
            "no more frames for a viewport that stays empty"
        );
    }

    #[test]
    fn one_prepare_is_enough_to_rule_out_software() {
        let health = Health::new();
        health.drawn();
        health.prepared(Prepared::Drew { uploading: false });
        for _ in 0..10 {
            health.drawn();
        }
        assert!(health.review().changed);
        assert_eq!(health.rendering(), Rendering::Drawing);
    }

    #[test]
    fn keeps_frames_coming_while_textures_go_up() {
        let health = Health::new();
        health.drawn();
        health.prepared(Prepared::Drew { uploading: true });
        assert!(health.review().redraw);
        health.prepared(Prepared::Drew { uploading: false });
        assert!(!health.review().redraw);
    }

    #[test]
    fn reports_a_gpu_that_cannot_be_set_up_and_a_device_that_was_lost() {
        let health = Health::new();
        health.drawn();
        health.prepared(Prepared::Unavailable("no adapter".into()));
        assert_eq!(
            health.rendering(),
            Rendering::Unavailable("no adapter".into())
        );
        assert!(health.review().changed);
        assert!(health.rendering().is_problem());

        health.prepared(Prepared::Lost("Unknown: reset".into()));
        assert_eq!(health.rendering(), Rendering::Lost("Unknown: reset".into()));
        assert!(health.review().changed);
    }

    #[test]
    fn a_failure_to_draw_a_model_is_forgotten_with_the_model() {
        let health = Health::new();
        health.drawn();
        health.prepared(Prepared::Failed("texture too large".into()));
        assert_eq!(
            health.rendering(),
            Rendering::Failed("texture too large".into())
        );
        assert!(health.review().changed);
        health.new_model();
        assert_eq!(health.rendering(), Rendering::Starting);
        assert!(health.review().changed, "the app is told it is over");
        // A lost device is not the model's doing, and stays.
        health.prepared(Prepared::Lost("gone".into()));
        health.new_model();
        assert_eq!(health.rendering(), Rendering::Lost("gone".into()));
    }

    #[test]
    fn a_restart_forgets_the_verdict_and_starts_counting_again() {
        let health = Health::new();
        for _ in 0..SOFTWARE_AFTER {
            health.drawn();
        }
        health.review();
        assert_eq!(health.rendering(), Rendering::Software);
        health.restart();
        assert_eq!(health.rendering(), Rendering::Starting);
        assert!(
            health.review().changed,
            "the app is told the message is gone"
        );
        health.drawn();
        assert_eq!(
            health.rendering(),
            Rendering::Starting,
            "counting from zero"
        );
    }
}
