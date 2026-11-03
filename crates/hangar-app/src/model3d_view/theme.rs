//! The viewport's colours from the window's theme (`readViewerTheme`,
//! `lib/model3d/viewerTheme.ts`).
//!
//! The old viewer read the tokens the active theme had set on the document;
//! here the theme is a value, and hangar-model3d derives the floor, the axes
//! and the wireframe from the five colours it is given, so the viewport sits
//! in the same palette as the panel around it.

use hangar_model3d::ViewerTheme;
use hangar_theme::Theme;

use crate::state::Model;

/// The viewport's theme for a window drawn in `theme`: the page colour for
/// the background, the text colour and the accent for the floor and the
/// normals, danger and success for the X and Y axes.
pub fn viewer_theme(theme: &Theme) -> ViewerTheme {
    let colors = &theme.colors;
    ViewerTheme::derive(
        colors.bg,
        colors.fg,
        colors.accent,
        colors.danger,
        colors.success,
        theme.is_dark(),
    )
}

/// The theme a workspace is drawn in: its own, else the settings' (the same
/// rule as `Model::theme_in`, asked for a workspace instead of a window, since
/// an agent's snapshot of a workspace's model may be taken while another
/// workspace is on screen).
pub fn viewer_theme_of(model: &Model, workspace: &str) -> ViewerTheme {
    let id = model
        .workspace(workspace)
        .and_then(|ws| ws.theme_id.as_deref())
        .unwrap_or(&model.state().settings.theme_id);
    viewer_theme(hangar_theme::by_id_or_default(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_theme::by_id;

    #[test]
    fn the_background_is_the_pages_and_the_axes_are_the_themes_hues() {
        let theme = by_id("graphite").expect("a bundled theme");
        let viewer = viewer_theme(theme);
        assert_eq!(viewer.background, theme.colors.bg);
        assert_eq!(viewer.axis_x, theme.colors.danger);
        assert_eq!(viewer.axis_y, theme.colors.success);
        assert_eq!(viewer.axis_z, theme.colors.accent);
        assert_eq!(viewer.normals, theme.colors.accent);
        assert_eq!(viewer.dark, theme.is_dark());
    }

    #[test]
    fn a_light_theme_makes_a_light_viewport() {
        let light = hangar_theme::themes()
            .iter()
            .find(|theme| !theme.is_dark())
            .expect("a bundled light theme");
        let viewer = viewer_theme(light);
        assert!(!viewer.dark);
        // The floor's lines step toward the text colour, which is dark here.
        assert!(viewer.grid_major.r < viewer.background.r);
    }

    #[test]
    fn a_workspaces_own_theme_wins_over_the_settings() {
        use crate::state::{AppState, Workspace};
        let light = hangar_theme::themes()
            .iter()
            .find(|theme| !theme.is_dark())
            .expect("a bundled light theme");
        let mut model = Model::default();
        model.hydrate(AppState {
            workspaces: vec![
                Workspace {
                    id: "own".into(),
                    theme_id: Some(light.id.into()),
                    ..Workspace::default()
                },
                Workspace {
                    id: "plain".into(),
                    ..Workspace::default()
                },
            ],
            ..AppState::default()
        });
        assert!(!viewer_theme_of(&model, "own").dark);
        let fallback = hangar_theme::by_id_or_default(&model.state().settings.theme_id);
        assert_eq!(viewer_theme_of(&model, "plain").dark, fallback.is_dark());
        // A workspace that is gone is drawn in the settings' theme too.
        assert_eq!(viewer_theme_of(&model, "gone").dark, fallback.is_dark());
    }
}
