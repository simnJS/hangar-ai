//! Experimental features: work that is not finished, ships off, and that
//! anyone can turn on from Settings › Experimental (`lib/experiments.ts`).
//!
//! To add one, add an entry to [`EXPERIMENTS`] and its title and hint to a
//! strings module (`src/strings/*`, then regenerate `hangar-i18n`): nothing
//! else draws or stores it. Code that wants to know whether it is on asks
//! [`is_experiment_enabled`]. The page appears on its own once the list is not
//! empty, and stays out of the way while it is (`schema.rs`).
//!
//! What is on is `settings.experiments`, a list of ids, so nothing is on for
//! someone who never opened the page, and an id that has since left the list
//! (a feature that became standard, or was dropped) is ignored rather than an
//! error. A feature that graduates loses its entry here and its check in the
//! code.

use hangar_i18n::Key;

use crate::state::Settings;

/// How far along a feature is; the page says so beside its title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Experimental,
    Beta,
}

impl Stage {
    /// `experiments.stage.{stage}`.
    pub fn label_key(self) -> Key {
        match self {
            Stage::Experimental => Key::ExperimentsStageExperimental,
            Stage::Beta => Key::ExperimentsStageBeta,
        }
    }
}

/// The desktop systems Hangar runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Windows,
    Macos,
}

impl Platform {
    /// What the page calls it (`PLATFORM_NAMES`): proper names, not words of
    /// the interface, so they are not translated.
    pub fn name(self) -> &'static str {
        match self {
            Platform::Linux => "Linux",
            Platform::Windows => "Windows",
            Platform::Macos => "macOS",
        }
    }

    /// The platform an operating system's name (`std::env::consts::OS`) is.
    /// `None` for anything that is not a desktop: a phone cannot run a
    /// feature the way a PC does.
    ///
    /// The web app read the user agent; the native build knows what it was
    /// compiled for, so the same cases hold with the system's own names.
    pub fn of_os(os: &str) -> Option<Platform> {
        match os {
            "linux" => Some(Platform::Linux),
            "windows" => Some(Platform::Windows),
            "macos" => Some(Platform::Macos),
            _ => None,
        }
    }

    /// The platform this build runs on (`currentPlatform`).
    pub fn current() -> Option<Platform> {
        Platform::of_os(std::env::consts::OS)
    }
}

/// One experimental feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Experiment {
    /// Stable, lowercase with dashes: it is what a saved settings file holds.
    pub id: &'static str,
    pub stage: Stage,
    pub title: Key,
    pub hint: Key,
    /// Where it can run. `None`: offered everywhere.
    pub platforms: Option<&'static [Platform]>,
}

/// Newest last. Empty until the first experiment lands.
pub const EXPERIMENTS: &[Experiment] = &[];

/// The experiments this machine can offer: those with no platform list, or
/// one that names it.
pub fn available_experiments(
    registry: &[Experiment],
    platform: Option<Platform>,
) -> Vec<Experiment> {
    registry
        .iter()
        .filter(|experiment| match experiment.platforms {
            None => true,
            Some(platforms) => platform.is_some_and(|platform| platforms.contains(&platform)),
        })
        .copied()
        .collect()
}

/// True when the person turned this experiment on and this machine can run
/// it. Anything unknown is off: a list left by an older version, a
/// hand-edited file, an id nobody registers.
///
/// A settings file that holds something else than a list reads as an empty
/// one before it gets here (`state/lenient.rs`), which is the TS's "a
/// non-array setting is off".
pub fn is_experiment_enabled(
    settings: &Settings,
    id: &str,
    registry: &[Experiment],
    platform: Option<Platform>,
) -> bool {
    if !settings.experiments.iter().any(|entry| entry == id) {
        return false;
    }
    available_experiments(registry, platform)
        .iter()
        .any(|experiment| experiment.id == id)
}

/// What code outside the settings asks: the shipped registry, this machine.
#[allow(dead_code)] // No experiment ships yet, so nothing asks; the first one will.
pub fn enabled(settings: &Settings, id: &str) -> bool {
    is_experiment_enabled(settings, id, EXPERIMENTS, Platform::current())
}

/// The list with `id` on or off, without a duplicate and in the order it was
/// turned on.
pub fn with_experiment(current: &[String], id: &str, on: bool) -> Vec<String> {
    let mut rest: Vec<String> = current
        .iter()
        .filter(|entry| entry.as_str() != id)
        .cloned()
        .collect();
    if on {
        rest.push(id.to_string());
    }
    rest
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::read_state;
    use serde_json::json;

    const EVERYWHERE: Experiment = Experiment {
        id: "demo",
        stage: Stage::Experimental,
        title: Key::ExperimentsTitle,
        hint: Key::ExperimentsWarning,
        platforms: None,
    };
    const LINUX_ONLY: Experiment = Experiment {
        id: "linux-demo",
        platforms: Some(&[Platform::Linux]),
        ..EVERYWHERE
    };
    const REGISTRY: [Experiment; 2] = [EVERYWHERE, LINUX_ONLY];

    fn with(ids: &[&str]) -> Settings {
        Settings {
            experiments: ids.iter().map(|id| id.to_string()).collect(),
            ..Settings::default()
        }
    }

    fn list(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    // `currentPlatform` of the TS read a user agent; the native build names
    // the system it was compiled for, so the cases are the same with the
    // names of `std::env::consts::OS`.
    #[test]
    fn the_platform_is_read_off_the_operating_system() {
        assert_eq!(Platform::of_os("linux"), Some(Platform::Linux));
        assert_eq!(Platform::of_os("windows"), Some(Platform::Windows));
        assert_eq!(Platform::of_os("macos"), Some(Platform::Macos));
    }

    #[test]
    fn a_phone_is_not_taken_for_a_desktop() {
        assert_eq!(Platform::of_os("android"), None);
        assert_eq!(Platform::of_os("ios"), None);
        assert_eq!(Platform::of_os(""), None);
    }

    #[test]
    fn this_build_knows_its_own_platform() {
        let expected = if cfg!(target_os = "linux") {
            Some(Platform::Linux)
        } else if cfg!(target_os = "windows") {
            Some(Platform::Windows)
        } else if cfg!(target_os = "macos") {
            Some(Platform::Macos)
        } else {
            None
        };
        assert_eq!(Platform::current(), expected);
    }

    #[test]
    fn a_feature_with_no_platform_list_is_offered_everywhere() {
        assert_eq!(
            available_experiments(&REGISTRY, Some(Platform::Windows)),
            vec![EVERYWHERE]
        );
    }

    #[test]
    fn a_platform_bound_feature_is_offered_only_there() {
        assert_eq!(
            available_experiments(&REGISTRY, Some(Platform::Linux)),
            REGISTRY.to_vec()
        );
    }

    #[test]
    fn only_the_unrestricted_ones_are_offered_when_the_platform_is_unknown() {
        assert_eq!(available_experiments(&REGISTRY, None), vec![EVERYWHERE]);
    }

    #[test]
    fn nothing_is_on_for_someone_who_never_turned_anything_on() {
        assert!(!is_experiment_enabled(
            &with(&[]),
            "demo",
            &REGISTRY,
            Some(Platform::Linux)
        ));
    }

    #[test]
    fn an_id_that_is_listed_and_registered_is_on() {
        assert!(is_experiment_enabled(
            &with(&["demo"]),
            "demo",
            &REGISTRY,
            Some(Platform::Linux)
        ));
    }

    #[test]
    fn a_machine_that_cannot_run_it_has_it_off_even_if_the_id_is_listed() {
        let settings = with(&["linux-demo"]);
        assert!(is_experiment_enabled(
            &settings,
            "linux-demo",
            &REGISTRY,
            Some(Platform::Linux)
        ));
        assert!(!is_experiment_enabled(
            &settings,
            "linux-demo",
            &REGISTRY,
            Some(Platform::Macos)
        ));
    }

    #[test]
    fn an_id_that_no_version_registers_any_more_is_ignored() {
        assert!(!is_experiment_enabled(
            &with(&["gone"]),
            "gone",
            &REGISTRY,
            Some(Platform::Linux)
        ));
    }

    #[test]
    fn a_settings_file_that_holds_something_else_than_a_list_has_nothing_on() {
        let state = read_state(json!({"settings": {"experiments": "demo"}}));
        assert!(!is_experiment_enabled(
            &state.settings,
            "demo",
            &REGISTRY,
            Some(Platform::Linux)
        ));
    }

    #[test]
    fn an_id_is_added_once_after_the_ones_already_on() {
        assert_eq!(with_experiment(&list(&["a"]), "b", true), list(&["a", "b"]));
        assert_eq!(
            with_experiment(&list(&["a", "b"]), "a", true),
            list(&["b", "a"])
        );
    }

    #[test]
    fn an_id_is_removed_and_the_others_stay() {
        assert_eq!(
            with_experiment(&list(&["a", "b"]), "a", false),
            list(&["b"])
        );
        assert_eq!(with_experiment(&list(&["a"]), "zzz", false), list(&["a"]));
    }

    #[test]
    fn nothing_is_the_start_of_the_list() {
        assert_eq!(with_experiment(&[], "a", true), list(&["a"]));
    }

    #[test]
    fn the_shipped_registry_has_unique_dash_separated_ids() {
        let mut seen = std::collections::HashSet::new();
        for experiment in EXPERIMENTS {
            assert!(seen.insert(experiment.id), "{} is registered twice", experiment.id);
            let well_formed = !experiment.id.is_empty()
                && experiment.id.split('-').all(|word| {
                    !word.is_empty()
                        && word
                            .chars()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                });
            assert!(well_formed, "{} is not kebab-case", experiment.id);
        }
    }
}
