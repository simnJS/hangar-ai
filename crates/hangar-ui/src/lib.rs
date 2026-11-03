//! The widgets every part of the window is built from, styled by the active
//! theme, so a view never picks a colour of its own.
//!
//! See docs/native/PLAN.md, "hangar-ui".

pub mod badge;
pub mod button;
pub mod dialog;
pub mod empty;
pub mod form;
pub mod glyph;
pub mod icon;
pub mod input;
pub mod keycap;
pub mod keys;
pub mod layer;
pub mod list;
pub mod matching;
pub mod menu;
pub mod menu_nav;
pub mod palette;
pub mod panel;
pub mod section;
pub mod shortcut;
pub mod shortcut_field;
pub mod split;
pub mod tabs;
pub mod text;
pub mod tip;
pub mod toast;

pub use icon::{Icon, icon};

#[cfg(test)]
mod testing;
