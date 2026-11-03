//! A way to see the capture button's result in a running debug build, with no
//! person at the save dialog: with `HANGAR_DEV_MODEL3D_CAPTURE=<folder>` the
//! first model on screen is captured as the button does (twice the panel's
//! size, the person's own camera) once its viewport has been drawn, and the
//! PNG is written in that folder under the name the dialog would propose.
//!
//! ```text
//! HANGAR_DEV_PREVIEW=/work/app/demo/models/sample.glb \
//!   HANGAR_DEV_MODEL3D_CAPTURE=/tmp/captures scripts/native-dev.sh
//! ```
//!
//! A release build ignores the variable.

use std::path::PathBuf;
use std::time::Duration;

/// How often a model that is not yet drawn is looked at again.
pub const POLL: Duration = Duration::from_millis(500);

/// The folder captures are written to, if a debug build was asked to.
pub fn capture_folder() -> Option<PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }
    folder_in(std::env::var_os("HANGAR_DEV_MODEL3D_CAPTURE"))
}

fn folder_in(value: Option<std::ffi::OsString>) -> Option<PathBuf> {
    value.filter(|value| !value.is_empty()).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_or_empty_variable_asks_for_nothing() {
        assert_eq!(folder_in(None), None);
        assert_eq!(folder_in(Some("".into())), None);
        assert_eq!(
            folder_in(Some("/tmp/captures".into())),
            Some(PathBuf::from("/tmp/captures"))
        );
    }
}
