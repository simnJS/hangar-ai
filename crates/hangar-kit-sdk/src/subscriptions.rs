//! What a kit wants to hear without asking: timers, and Hangar's events. A
//! kit says all of it after every update, as iced's `subscription` does, so
//! nothing has to be stopped by hand.

use std::rc::Rc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::Value;

use hangar_kit_ui::abi::{self, MIN_TIMER_MS};
use hangar_kit_ui::api::{self, TabParamsAnswer, Theme, Visibility};

type EventHandler<M> = Rc<dyn Fn(Value) -> Option<M>>;

pub struct Subscriptions<M> {
    pub(crate) timers: Vec<(String, u32, M)>,
    pub(crate) events: Vec<(String, EventHandler<M>)>,
}

impl<M> Default for Subscriptions<M> {
    fn default() -> Self {
        Subscriptions {
            timers: Vec::new(),
            events: Vec::new(),
        }
    }
}

impl<M: 'static> Subscriptions<M> {
    pub fn none() -> Self {
        Subscriptions::default()
    }

    /// `message` every `period`, under `id`: a timer that keeps its id and its
    /// period keeps running across updates. 100 ms at the shortest.
    pub fn every(mut self, id: impl Into<String>, period: Duration, message: M) -> Self {
        let every_ms = period
            .as_millis()
            .clamp(MIN_TIMER_MS as u128, u32::MAX as u128) as u32;
        self.timers.push((id.into(), every_ms, message));
        self
    }

    /// `panes.changed`: the workspace's panes changed. Needs `panes.read`;
    /// without it the tab simply hears nothing.
    pub fn panes_changed(self, message: M) -> Self
    where
        M: Clone,
    {
        self.event(api::event::PANES_CHANGED, move |_| Some(message.clone()))
    }

    /// `tab.params`: the tab was asked for again, or a singleton got new
    /// params.
    pub fn tab_params(self, f: impl Fn(TabParamsAnswer) -> M + 'static) -> Self {
        self.typed(api::event::TAB_PARAMS, f)
    }

    /// `theme`: the scheme or the language changed.
    pub fn theme(self, f: impl Fn(Theme) -> M + 'static) -> Self {
        self.typed(api::event::THEME, f)
    }

    /// `tab.visibility`: the tab came to the front or went behind. A tab
    /// that polls slows down while hidden.
    pub fn visibility(self, f: impl Fn(bool) -> M + 'static) -> Self {
        self.typed(api::event::TAB_VISIBILITY, move |visibility: Visibility| {
            f(visibility.visible)
        })
    }

    /// Any event by its name, its data as JSON: one a newer Hangar sends and
    /// this SDK does not type yet.
    pub fn event(
        mut self,
        name: impl Into<String>,
        f: impl Fn(Value) -> Option<M> + 'static,
    ) -> Self {
        self.events.push((name.into(), Rc::new(f)));
        self
    }

    fn typed<T: DeserializeOwned>(self, name: &str, f: impl Fn(T) -> M + 'static) -> Self {
        self.event(name, move |data| serde_json::from_value(data).ok().map(&f))
    }

    pub(crate) fn wire(&self) -> abi::Subscriptions {
        abi::Subscriptions {
            events: self.events.iter().map(|(name, _)| name.clone()).collect(),
            timers: self
                .timers
                .iter()
                .map(|(id, every_ms, _)| abi::Timer {
                    id: id.clone(),
                    every_ms: *every_ms,
                })
                .collect(),
        }
    }
}
