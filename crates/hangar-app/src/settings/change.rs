//! What a row of the page edits and how the edit is written: the `set(key,
//! value)`, `patchWorkspace(patch)` and `resetSettings()` of the TS schema's
//! context, as data.
//!
//! A row names its [`Target`]; the control that draws it reports a [`Value`];
//! together they are a [`Change`], which rides in a message and becomes a
//! store action in [`action`]. Keeping the three apart is what lets the
//! schema be a plain description that tests read, with no closures in it.
//!
//! Every write goes through `Model::apply` (PACKAGES.md, rule 3): the
//! [`StoreAction`] returned here carries a [`Patch`] that sets the field, and
//! the store merges it as `updateSettings({[key]: value})` did.

use hangar_i18n::Locale;
use serde_json::Value as Json;

use super::experiments::with_experiment;
use crate::state::types::{AgentId, CursorStyle, Settings, VoiceEngine, Workspace};
use crate::state::{Patch, StoreAction};

/// The value a control reports.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Number(f64),
    Text(String),
}

/// A setting a row edits.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Target {
    // General
    Locale,
    ProjectsFolder,
    AutoResume,
    ClaudeBridge,
    LaunchDelayMs,
    HibernateIdle,
    HibernateAfterMin,
    AgentPaneControl,
    AgentCommand(AgentId),
    // Appearance
    UiScale,
    NavigatorOpen,
    InspectorOpen,
    /// The workspace pins the theme it shows (on), or follows the global one.
    ThemeScope,
    /// A theme card: the workspace's own when it pins one, else the global.
    Theme,
    // Terminal
    FontFamily,
    FontSize,
    LineHeight,
    LetterSpacing,
    Padding,
    CursorStyle,
    CursorBlink,
    Scrollback,
    // Shell
    ShellDefault,
    ShellWorkspace,
    // Notifications
    NotifyOnIdle,
    NotifyIdleMs,
    NotifyOnlyWhenAway,
    // Voice
    VoiceEnabled,
    VoiceEngine,
    VoiceCloudModel,
    VoiceLanguage,
    VoiceSubmit,
    VoiceCleanup,
    VoiceCleanupModel,
    VoiceCleanupHint,
    // Discord
    DiscordPresence,
    DiscordShowWorkspace,
    DiscordShowAgents,
    DiscordAppId,
    // Experimental
    Experiment(String),
}

/// One edit.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Set(Target, Value),
    /// The plaintext Groq key an older version left in `state.json` is
    /// forgotten (`set("voiceApiKey", undefined)`).
    DropLegacyVoiceKey,
    /// `updateSettings(DEFAULT_SETTINGS)`.
    Reset,
}

impl Change {
    pub fn set(target: Target, value: Value) -> Change {
        Change::Set(target, value)
    }

    pub fn toggle(target: Target, on: bool) -> Change {
        Change::Set(target, Value::Bool(on))
    }

    pub fn number(target: Target, value: f64) -> Change {
        Change::Set(target, Value::Number(value))
    }

    pub fn text(target: Target, value: impl Into<String>) -> Change {
        Change::Set(target, Value::Text(value.into()))
    }
}

/// What a change needs to know about the window it is made in: the active
/// workspace (the theme and the shell can be its own) and the global theme
/// the workspace falls back on.
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    pub workspace: Option<&'a Workspace>,
    pub theme_id: &'a str,
}

impl Scope<'_> {
    /// The theme the window shows: the workspace's own, else the global one
    /// (`effectiveThemeId`).
    pub fn effective_theme(&self) -> &str {
        self.workspace
            .and_then(|ws| ws.theme_id.as_deref())
            .unwrap_or(self.theme_id)
    }

    /// The workspace pins its own theme (`pinnedThemeId !== null`).
    pub fn pinned(&self) -> bool {
        self.workspace.is_some_and(|ws| ws.theme_id.is_some())
    }
}

/// The store action for a change, or `None` when it would change nothing it
/// can name (a value of the wrong kind, a workspace row with no workspace).
pub fn action(scope: Scope, change: &Change) -> Option<StoreAction> {
    match change {
        Change::Reset => Some(StoreAction::ResetSettings),
        Change::DropLegacyVoiceKey => Some(settings_patch(|s| s.voice_api_key = None)),
        Change::Set(Target::ThemeScope, Value::Bool(on)) => {
            let ws = scope.workspace?;
            let theme = on.then(|| scope.effective_theme().to_string());
            Some(StoreAction::UpdateWorkspace(
                ws.id.clone(),
                Patch::new(move |ws: &mut Workspace| ws.theme_id = theme.clone()),
            ))
        }
        Change::Set(Target::Theme, Value::Text(id)) if scope.pinned() => {
            let ws = scope.workspace?;
            let id = id.clone();
            Some(StoreAction::UpdateWorkspace(
                ws.id.clone(),
                Patch::new(move |ws: &mut Workspace| ws.theme_id = Some(id.clone())),
            ))
        }
        Change::Set(Target::ShellWorkspace, Value::Text(id)) => {
            let ws = scope.workspace?;
            let id = (!id.is_empty()).then(|| id.clone());
            Some(StoreAction::UpdateWorkspace(
                ws.id.clone(),
                Patch::new(move |ws: &mut Workspace| ws.shell_id = id.clone()),
            ))
        }
        Change::Set(target, value) => {
            // Checked here so that a change that cannot apply is refused
            // before it reaches the store.
            apply(&mut Settings::default(), target, value).then(|| {
                let (target, value) = (target.clone(), value.clone());
                settings_patch(move |s| {
                    apply(s, &target, &value);
                })
            })
        }
    }
}

fn settings_patch(edit: impl Fn(&mut Settings) + Send + Sync + 'static) -> StoreAction {
    StoreAction::UpdateSettings(Patch::new(edit))
}

/// `Number(value)` of the select that stores minutes as text.
fn minutes(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// A whole number for a count the person drags a slider over.
fn count(value: f64) -> u64 {
    if value.is_finite() && value > 0.0 {
        value.round() as u64
    } else {
        0
    }
}

/// Writes `value` into the field `target` names. `false` when the value is
/// not of the kind the field holds, or the target is not a field of the
/// global settings (a workspace's, handled by [`action`]).
pub fn apply(settings: &mut Settings, target: &Target, value: &Value) -> bool {
    use Target as T;
    use Value::{Bool, Number, Text};
    match (target, value) {
        (T::Locale, Text(id)) => settings.locale = id.parse::<Locale>().ok(),
        (T::ProjectsFolder, Text(path)) => settings.projects_folder = Some(path.clone()),
        (T::AutoResume, Bool(on)) => settings.auto_resume = *on,
        (T::ClaudeBridge, Bool(on)) => settings.claude_bridge = *on,
        (T::LaunchDelayMs, Number(ms)) => settings.launch_delay_ms = count(*ms),
        (T::HibernateIdle, Bool(on)) => settings.hibernate_idle = *on,
        (T::HibernateAfterMin, Text(text)) => match minutes(text) {
            Some(value) => settings.hibernate_after_min = value,
            None => return false,
        },
        (T::AgentPaneControl, Bool(on)) => settings.agent_pane_control = *on,
        (T::AgentCommand(agent), Text(command)) => {
            // Cleared means back to the default, and storing "" would pin this
            // agent to a command it no longer follows. What is stored is not
            // trimmed: it is what the person typed.
            if command.trim().is_empty() {
                settings.agent_commands.remove(agent.as_str());
            } else {
                settings
                    .agent_commands
                    .insert(agent.as_str().to_string(), Json::String(command.clone()));
            }
        }
        (T::UiScale, Number(percent)) => settings.ui_scale = *percent,
        (T::NavigatorOpen, Bool(on)) => settings.navigator_open = *on,
        (T::InspectorOpen, Bool(on)) => settings.inspector_open = *on,
        (T::Theme, Text(id)) => settings.theme_id = id.clone(),
        (T::FontFamily, Text(stack)) => settings.font_family = stack.clone(),
        (T::FontSize, Number(px)) => settings.font_size = *px,
        (T::LineHeight, Number(factor)) => settings.line_height = *factor,
        (T::LetterSpacing, Number(px)) => settings.letter_spacing = *px,
        (T::Padding, Number(px)) => settings.padding = *px,
        (T::CursorStyle, Text(style)) => match cursor_style(style) {
            Some(style) => settings.cursor_style = style,
            None => return false,
        },
        (T::CursorBlink, Bool(on)) => settings.cursor_blink = *on,
        (T::Scrollback, Number(lines)) => settings.scrollback = count(*lines),
        (T::ShellDefault, Text(id)) => {
            settings.shell_id = (!id.is_empty()).then(|| id.clone());
        }
        (T::NotifyOnIdle, Bool(on)) => settings.notify_on_idle = *on,
        (T::NotifyIdleMs, Number(ms)) => settings.notify_idle_ms = count(*ms),
        (T::NotifyOnlyWhenAway, Bool(on)) => settings.notify_only_when_away = *on,
        (T::VoiceEnabled, Bool(on)) => settings.voice_enabled = *on,
        (T::VoiceEngine, Text(engine)) => match voice_engine(engine) {
            Some(engine) => settings.voice_engine = engine,
            None => return false,
        },
        (T::VoiceCloudModel, Text(model)) => settings.voice_cloud_model = model.clone(),
        (T::VoiceLanguage, Text(code)) => settings.voice_language = code.clone(),
        (T::VoiceSubmit, Bool(on)) => settings.voice_submit = *on,
        (T::VoiceCleanup, Bool(on)) => settings.voice_cleanup = *on,
        (T::VoiceCleanupModel, Text(model)) => settings.voice_cleanup_model = model.trim().into(),
        (T::VoiceCleanupHint, Text(words)) => settings.voice_cleanup_hint = words.clone(),
        (T::DiscordPresence, Bool(on)) => settings.discord_presence = *on,
        (T::DiscordShowWorkspace, Bool(on)) => settings.discord_show_workspace = *on,
        (T::DiscordShowAgents, Bool(on)) => settings.discord_show_agents = *on,
        (T::DiscordAppId, Text(id)) => settings.discord_app_id = id.trim().into(),
        (T::Experiment(id), Bool(on)) => {
            settings.experiments = with_experiment(&settings.experiments, id, *on);
        }
        _ => return false,
    }
    true
}

/// The id the cursor style segmented control reports.
pub fn cursor_style_id(style: CursorStyle) -> &'static str {
    match style {
        CursorStyle::Bar => "bar",
        CursorStyle::Block => "block",
        CursorStyle::Underline => "underline",
    }
}

fn cursor_style(id: &str) -> Option<CursorStyle> {
    [CursorStyle::Bar, CursorStyle::Block, CursorStyle::Underline]
        .into_iter()
        .find(|style| cursor_style_id(*style) == id)
}

/// The id the voice engine segmented control reports.
pub fn voice_engine_id(engine: VoiceEngine) -> &'static str {
    match engine {
        VoiceEngine::Local => "local",
        VoiceEngine::Groq => "groq",
    }
}

fn voice_engine(id: &str) -> Option<VoiceEngine> {
    [VoiceEngine::Local, VoiceEngine::Groq]
        .into_iter()
        .find(|engine| voice_engine_id(*engine) == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn applied(settings: Settings, change: Change) -> Settings {
        let mut settings = settings;
        let scope = Scope {
            workspace: None,
            theme_id: "graphite",
        };
        match action(scope, &change) {
            Some(StoreAction::UpdateSettings(patch)) => patch.apply(&mut settings),
            other => panic!("{change:?} was not a settings patch: {other:?}"),
        }
        settings
    }

    fn set(target: Target, value: Value) -> Settings {
        applied(Settings::default(), Change::Set(target, value))
    }

    fn workspace(theme: Option<&str>) -> Workspace {
        Workspace {
            id: "w1".into(),
            name: "Shop".into(),
            theme_id: theme.map(str::to_string),
            ..Workspace::default()
        }
    }

    fn workspace_after(ws: &Workspace, theme: &str, change: Change) -> Workspace {
        let scope = Scope {
            workspace: Some(ws),
            theme_id: theme,
        };
        let Some(StoreAction::UpdateWorkspace(id, patch)) = action(scope, &change) else {
            panic!("{change:?} was not a workspace patch");
        };
        assert_eq!(id, ws.id);
        let mut next = ws.clone();
        patch.apply(&mut next);
        next
    }

    #[test]
    fn the_language_is_a_locale_and_nothing_follows_the_system() {
        assert_eq!(
            set(Target::Locale, Value::Text("fr".into())).locale,
            Some(Locale::Fr)
        );
        let french = Settings {
            locale: Some(Locale::Fr),
            ..Settings::default()
        };
        // "" is Auto: back to following the system.
        assert_eq!(
            applied(french, Change::text(Target::Locale, "")).locale,
            None
        );
    }

    #[test]
    fn the_projects_folder_is_stored_as_typed() {
        let s = set(Target::ProjectsFolder, Value::Text("/code ".into()));
        assert_eq!(s.projects_folder.as_deref(), Some("/code "));
    }

    #[test]
    fn toggles_write_their_flag() {
        for (target, read) in [
            (
                Target::AutoResume,
                (|s: &Settings| s.auto_resume) as fn(&Settings) -> bool,
            ),
            (Target::ClaudeBridge, |s| s.claude_bridge),
            (Target::HibernateIdle, |s| s.hibernate_idle),
            (Target::AgentPaneControl, |s| s.agent_pane_control),
            (Target::NavigatorOpen, |s| s.navigator_open),
            (Target::InspectorOpen, |s| s.inspector_open),
            (Target::CursorBlink, |s| s.cursor_blink),
            (Target::NotifyOnIdle, |s| s.notify_on_idle),
            (Target::NotifyOnlyWhenAway, |s| s.notify_only_when_away),
            (Target::VoiceEnabled, |s| s.voice_enabled),
            (Target::VoiceSubmit, |s| s.voice_submit),
            (Target::VoiceCleanup, |s| s.voice_cleanup),
            (Target::DiscordPresence, |s| s.discord_presence),
            (Target::DiscordShowWorkspace, |s| s.discord_show_workspace),
            (Target::DiscordShowAgents, |s| s.discord_show_agents),
        ] {
            let before = read(&Settings::default());
            let off = set(target.clone(), Value::Bool(!before));
            assert_eq!(read(&off), !before, "{target:?}");
            let back = applied(off, Change::toggle(target.clone(), before));
            assert_eq!(read(&back), before, "{target:?}");
        }
    }

    #[test]
    fn sliders_write_their_number() {
        assert_eq!(
            set(Target::LaunchDelayMs, Value::Number(1500.0)).launch_delay_ms,
            1500
        );
        assert_eq!(set(Target::UiScale, Value::Number(115.0)).ui_scale, 115.0);
        assert_eq!(set(Target::FontSize, Value::Number(15.0)).font_size, 15.0);
        assert_eq!(
            set(Target::LineHeight, Value::Number(1.45)).line_height,
            1.45
        );
        assert_eq!(
            set(Target::LetterSpacing, Value::Number(-0.5)).letter_spacing,
            -0.5
        );
        assert_eq!(set(Target::Padding, Value::Number(4.0)).padding, 4.0);
        assert_eq!(
            set(Target::Scrollback, Value::Number(20_000.0)).scrollback,
            20_000
        );
        assert_eq!(
            set(Target::NotifyIdleMs, Value::Number(5000.0)).notify_idle_ms,
            5000
        );
    }

    #[test]
    fn a_count_is_whole_and_never_negative() {
        assert_eq!(count(1499.6), 1500);
        assert_eq!(count(-3.0), 0);
        assert_eq!(count(f64::NAN), 0);
    }

    #[test]
    fn the_hibernate_delay_is_stored_as_a_number_of_minutes() {
        assert_eq!(
            set(Target::HibernateAfterMin, Value::Text("30".into())).hibernate_after_min,
            30.0
        );
        assert_eq!(
            set(Target::HibernateAfterMin, Value::Text("7.5".into())).hibernate_after_min,
            7.5
        );
        assert!(
            action(
                Scope {
                    workspace: None,
                    theme_id: "graphite"
                },
                &Change::text(Target::HibernateAfterMin, "soon")
            )
            .is_none()
        );
    }

    #[test]
    fn a_launch_command_is_kept_as_typed_and_dropped_when_blank() {
        let typed = set(
            Target::AgentCommand(AgentId::Claude),
            Value::Text(" claude --verbose".into()),
        );
        assert_eq!(
            typed.agent_commands["claude"],
            Json::String(" claude --verbose".into())
        );
        let cleared = applied(
            typed,
            Change::text(Target::AgentCommand(AgentId::Claude), "   "),
        );
        assert!(cleared.agent_commands.is_empty());
        // The other agents' commands stay.
        let two = applied(
            set(
                Target::AgentCommand(AgentId::Codex),
                Value::Text("codex".into()),
            ),
            Change::text(Target::AgentCommand(AgentId::Claude), "c"),
        );
        let none = applied(two, Change::text(Target::AgentCommand(AgentId::Claude), ""));
        assert_eq!(none.agent_commands.len(), 1);
        assert!(none.agent_commands.contains_key("codex"));
    }

    #[test]
    fn the_font_and_the_cursor_are_chosen_by_name() {
        assert_eq!(
            set(Target::FontFamily, Value::Text("Hack, monospace".into())).font_family,
            "Hack, monospace"
        );
        assert_eq!(
            set(Target::CursorStyle, Value::Text("block".into())).cursor_style,
            CursorStyle::Block
        );
        assert_eq!(cursor_style_id(CursorStyle::Underline), "underline");
        assert!(
            action(
                Scope {
                    workspace: None,
                    theme_id: "graphite"
                },
                &Change::text(Target::CursorStyle, "blob")
            )
            .is_none()
        );
    }

    #[test]
    fn the_default_shell_is_none_when_set_to_auto() {
        let chosen = set(Target::ShellDefault, Value::Text("bash".into()));
        assert_eq!(chosen.shell_id.as_deref(), Some("bash"));
        let auto = applied(chosen, Change::text(Target::ShellDefault, ""));
        assert_eq!(auto.shell_id, None);
    }

    #[test]
    fn the_voice_settings_write_their_field() {
        let s = set(Target::VoiceEngine, Value::Text("groq".into()));
        assert_eq!(s.voice_engine, VoiceEngine::Groq);
        assert_eq!(
            set(
                Target::VoiceCloudModel,
                Value::Text("whisper-large-v3".into())
            )
            .voice_cloud_model,
            "whisper-large-v3"
        );
        assert_eq!(
            set(Target::VoiceLanguage, Value::Text("fr".into())).voice_language,
            "fr"
        );
        // The model name is trimmed on every change; the vocabulary is not.
        assert_eq!(
            set(Target::VoiceCleanupModel, Value::Text("  llama  ".into())).voice_cleanup_model,
            "llama"
        );
        assert_eq!(
            set(
                Target::VoiceCleanupHint,
                Value::Text(" Hangar, Tauri ".into())
            )
            .voice_cleanup_hint,
            " Hangar, Tauri "
        );
    }

    #[test]
    fn the_discord_application_id_is_trimmed() {
        assert_eq!(
            set(Target::DiscordAppId, Value::Text(" 123 ".into())).discord_app_id,
            "123"
        );
    }

    #[test]
    fn an_experiment_is_turned_on_after_the_others_and_off_again() {
        let one = set(Target::Experiment("a".into()), Value::Bool(true));
        let two = applied(one, Change::toggle(Target::Experiment("b".into()), true));
        assert_eq!(two.experiments, ["a", "b"]);
        let off = applied(two, Change::toggle(Target::Experiment("a".into()), false));
        assert_eq!(off.experiments, ["b"]);
    }

    #[test]
    fn dropping_the_legacy_key_removes_the_field() {
        let legacy = Settings {
            voice_api_key: Some("gsk_old".into()),
            ..Settings::default()
        };
        assert_eq!(
            applied(legacy, Change::DropLegacyVoiceKey).voice_api_key,
            None
        );
    }

    #[test]
    fn a_reset_is_the_stores() {
        let scope = Scope {
            workspace: None,
            theme_id: "graphite",
        };
        assert!(matches!(
            action(scope, &Change::Reset),
            Some(StoreAction::ResetSettings)
        ));
    }

    #[test]
    fn a_value_of_the_wrong_kind_changes_nothing() {
        let scope = Scope {
            workspace: None,
            theme_id: "graphite",
        };
        assert!(action(scope, &Change::text(Target::AutoResume, "yes")).is_none());
        assert!(action(scope, &Change::toggle(Target::FontSize, true)).is_none());
    }

    #[test]
    fn a_theme_card_sets_the_global_theme_while_nothing_is_pinned() {
        let s = set(Target::Theme, Value::Text("nord".into()));
        assert_eq!(s.theme_id, "nord");
        // A workspace that follows the global theme changes the global one.
        let ws = workspace(None);
        let scope = Scope {
            workspace: Some(&ws),
            theme_id: "graphite",
        };
        assert!(matches!(
            action(scope, &Change::text(Target::Theme, "nord")),
            Some(StoreAction::UpdateSettings(_))
        ));
    }

    #[test]
    fn a_theme_card_sets_the_workspaces_own_theme_when_it_pins_one() {
        let ws = workspace(Some("dracula"));
        let next = workspace_after(&ws, "graphite", Change::text(Target::Theme, "nord"));
        assert_eq!(next.theme_id.as_deref(), Some("nord"));
    }

    #[test]
    fn pinning_the_theme_pins_the_one_the_window_shows() {
        let follows = workspace(None);
        let pinned = workspace_after(
            &follows,
            "graphite",
            Change::toggle(Target::ThemeScope, true),
        );
        assert_eq!(pinned.theme_id.as_deref(), Some("graphite"));
        // Unpinning goes back to following the global theme.
        let released = workspace_after(
            &pinned,
            "graphite",
            Change::toggle(Target::ThemeScope, false),
        );
        assert_eq!(released.theme_id, None);
        // Pinning when the workspace already shows its own keeps that one.
        let own = workspace(Some("dracula"));
        let again = workspace_after(&own, "graphite", Change::toggle(Target::ThemeScope, true));
        assert_eq!(again.theme_id.as_deref(), Some("dracula"));
    }

    #[test]
    fn the_workspaces_own_shell_is_none_when_it_follows_the_default() {
        let ws = workspace(None);
        let chosen = workspace_after(&ws, "graphite", Change::text(Target::ShellWorkspace, "zsh"));
        assert_eq!(chosen.shell_id.as_deref(), Some("zsh"));
        let follow = workspace_after(
            &chosen,
            "graphite",
            Change::text(Target::ShellWorkspace, ""),
        );
        assert_eq!(follow.shell_id, None);
    }

    #[test]
    fn a_workspace_row_with_no_workspace_changes_nothing() {
        let scope = Scope {
            workspace: None,
            theme_id: "graphite",
        };
        assert!(action(scope, &Change::toggle(Target::ThemeScope, true)).is_none());
        assert!(action(scope, &Change::text(Target::ShellWorkspace, "zsh")).is_none());
    }
}
