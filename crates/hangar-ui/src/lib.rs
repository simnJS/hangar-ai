//! The widgets every part of the window is built from, styled by the active
//! theme, so a view never picks a colour of its own.
//!
//! See docs/native/PLAN.md, "hangar-ui".

pub mod button;
pub mod glyph;
pub mod icon;
pub mod input;
pub mod layer;
pub mod matching;
pub mod menu;
pub mod menu_nav;
pub mod shortcut;
pub mod text;
pub mod tip;

pub use icon::{Icon, icon};

#[cfg(test)]
mod testing;
