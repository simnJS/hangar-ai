//! Hangar's window, natively.
//!
//! See docs/native/PLAN.md, "hangar-app".

// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {}
