//! P19: Orchestration engine.
//!
//! Ports lib/{workerControl,orchestratorEvents,orchestratorBrief,managerEvents,
//! projects}.ts. Read inventory-frontend.md §5/1, Appendix A6 and
//! crates/hangar-app/PACKAGES.md.

pub mod brief;
pub mod brief_text;
pub mod events;
pub mod manager_feed;
pub mod port;
pub mod projects;
pub mod roles;
pub mod text;
pub mod worker;

#[cfg(test)]
pub(crate) mod testkit;

use iced::{Subscription, Task};

use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::Region;
use crate::state::Model;

#[derive(Debug, Default)]
pub struct State;

#[derive(Debug, Default)]
pub struct Live;

#[derive(Debug, Clone)]
pub enum Msg {}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {}
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    Subscription::none()
}

pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    None
}

pub fn on_intent(st: &mut State, m: &mut Model, sv: &Services, intent: &Intent) -> Option<Task<Message>> {
    None
}

pub fn on_event(st: &mut State, m: &mut Model, sv: &Services, event: &CoreEvent) -> Option<Task<Message>> {
    None
}
