//! The widgets every part of the window is built from, styled by the active
//! theme, so a view never picks a colour of its own.
//!
//! See docs/native/PLAN.md, "hangar-ui".

pub mod icon;
pub mod matching;
pub mod menu_nav;
pub mod shortcut;

pub use icon::{Icon, icon};
