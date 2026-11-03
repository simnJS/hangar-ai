//! P11: Usage statistics.
//!
//! Ports StatsView.tsx, StatsCharts.tsx, lib/{stats,statsModel}.ts. Read
//! inventory-frontend.md §2.2 Stats and crates/hangar-app/PACKAGES.md. Planned
//! files of this directory (§8.1): mod, model, view, ranked, sessions, tables,
//! charts/{columns,heatmap,sparkline,readout,ticks}.
//!
//! This is the stub P0 handed over: the surface the shell calls, with
//! placeholder bodies. The directory is this package's own; replace the
//! bodies, add files, keep the signatures the shell calls.

use iced::{Element, Subscription, Task};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{Region, placeholder};
use crate::state::Model;

/// The package's UI state; lives in `App.pkg.stats`.
#[derive(Debug, Default)]
pub struct State;

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.stats`.
/// The usage cache (60 s fresh), shared with P20's `remote_stats`.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::Stats(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {}
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    Subscription::none()
}

/// What takes the focus when F6 reaches region `Main`, which this package
/// owns.
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
    None
}

/// Region `main` in the stats view.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    placeholder::stub(ctx, "stats::view")
}
