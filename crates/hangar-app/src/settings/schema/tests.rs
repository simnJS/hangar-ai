//! `schema.experiments.test.ts`, and the table of inventory §2.3/2 as a test:
//! every row of the old schema, with the same ids, controls and defaults.

use super::*;
use crate::settings::change::{Change, Scope, Value, action};
use crate::settings::experiments::{Experiment, Stage};
use crate::state::StoreAction;
use crate::state::types::Workspace;
use hangar_i18n::Locale;

fn shell(id: &str, label: &str) -> ShellInfo {
    ShellInfo {
        id: id.into(),
        label: label.into(),
        program: id.into(),
        args: Vec::new(),
    }
}

struct World {
    settings: Settings,
    shells: Vec<ShellInfo>,
    workspace: Option<Workspace>,
    version: String,
    experiments: Vec<Experiment>,
    locale: Locale,
    platform: Option<Platform>,
    keyboard: KeyboardPlatform,
}

impl World {
    fn new() -> World {
        World {
            settings: Settings::default(),
            shells: Vec::new(),
            workspace: None,
            version: "1.12.1".into(),
            experiments: Vec::new(),
            locale: Locale::En,
            platform: Some(Platform::Linux),
            keyboard: KeyboardPlatform::Other,
        }
    }

    fn build(&self) -> Vec<Category> {
        let keymap = KeymapIndex::on(self.keyboard, &self.settings.keybindings);
        build_categories(&Context {
            t: Translator::new(self.locale),
            settings: &self.settings,
            shells: &self.shells,
            workspace: self.workspace.as_ref(),
            version: &self.version,
            experiments: &self.experiments,
            platform: self.platform,
            keymap: &keymap,
        })
    }
}

fn workspace(name: &str) -> Workspace {
    Workspace {
        id: "w1".into(),
        name: name.into(),
        ..Workspace::default()
    }
}

fn items(category: &Category) -> Vec<&Item> {
    category.sections.iter().flat_map(|s| &s.items).collect()
}

fn category(built: &[Category], id: SettingsCategory) -> &Category {
    built
        .iter()
        .find(|category| category.id == id)
        .unwrap_or_else(|| panic!("no {id:?} category"))
}

fn find<'a>(built: &'a [Category], item_id: &str) -> &'a Item {
    built
        .iter()
        .flat_map(items)
        .find(|item| item.id == item_id)
        .unwrap_or_else(|| panic!("no row {item_id}"))
}

fn ids(category: &Category) -> Vec<&str> {
    items(category)
        .iter()
        .map(|item| item.id.as_str())
        .collect()
}

/// One line per row: its control, what it edits and the value it shows.
fn describe(item: &Item) -> String {
    match &item.kind {
        Kind::Select { value, target, .. } => format!("select {target:?} = {value:?}"),
        Kind::Segmented { value, target, .. } => format!("segmented {target:?} = {value:?}"),
        Kind::Toggle { value, target } => format!("toggle {target:?} = {value}"),
        Kind::Input {
            value,
            placeholder,
            target,
        } => format!("input {target:?} = {value:?} ({:?})", &**placeholder),
        Kind::Slider {
            value,
            min,
            max,
            step,
            display,
            target,
        } => format!("slider {target:?} = {value} [{min}..{max} by {step}] {display:?}"),
        Kind::Text { value } => format!("text {value:?}"),
        Kind::Action {
            confirm,
            danger,
            act,
            ..
        } => format!(
            "action {act:?}{}{}",
            if confirm.is_some() { " (confirm)" } else { "" },
            if *danger { " (danger)" } else { "" }
        ),
        Kind::Custom { bare, block } => {
            format!("custom {block:?}{}", if *bare { " (bare)" } else { "" })
        }
    }
}

fn table(built: &[Category], id: SettingsCategory) -> Vec<String> {
    items(category(built, id))
        .into_iter()
        .map(|item| format!("{}: {}", item.id, describe(item)))
        .collect()
}

#[test]
fn the_categories_come_in_the_order_of_the_page() {
    let built = World::new().build();
    let order: Vec<SettingsCategory> = built.iter().map(|c| c.id).collect();
    // No Experimental page while the registry is empty.
    let expected: Vec<SettingsCategory> = ORDER
        .iter()
        .copied()
        .filter(|id| *id != SettingsCategory::Experimental)
        .collect();
    assert_eq!(order, expected);
    assert_eq!(
        built.iter().map(|c| c.icon).collect::<Vec<_>>(),
        [
            Icon::Sliders,
            Icon::Appearance,
            Icon::Terminal,
            Icon::Prompt,
            Icon::Bell,
            Icon::Broadcast,
            Icon::Plug,
            Icon::Bolt,
            Icon::Mic,
            Icon::Gamepad,
            Icon::Keyboard,
            Icon::Info,
        ]
    );
}

#[test]
fn general_has_every_row_of_the_old_schema_with_its_default() {
    let built = World::new().build();
    assert_eq!(
        table(&built, SettingsCategory::General),
        [
            r#"locale: segmented Locale = """#,
            "projectsFolder: custom ProjectsFolder",
            "autoResume: toggle AutoResume = true",
            "claudeBridge: toggle ClaudeBridge = true",
            r#"launchDelay: slider LaunchDelayMs = 700 [0..4000 by 100] "700 ms""#,
            "hibernateIdle: toggle HibernateIdle = true",
            r#"hibernateAfter: select HibernateAfterMin = "10""#,
            "agentPaneControl: toggle AgentPaneControl = true",
            r#"agentCommand.claude: input ClaudeCommand = "" ("claude")"#,
        ]
    );
}

#[test]
fn appearance_terminal_shell_and_notifications_have_every_row_too() {
    let built = World::new().build();
    // The title bar's row is Linux's alone: elsewhere the system's stays.
    let mut appearance = vec![
        r#"uiScale: slider UiScale = 100 [90..130 by 5] "100 %""#,
        "navigatorOpen: toggle NavigatorOpen = true",
        "inspectorOpen: toggle InspectorOpen = true",
    ];
    if cfg!(target_os = "linux") {
        appearance.push("systemTitleBar: toggle SystemTitleBar = false");
    }
    appearance.push(r#"themePick: custom Themes { active: "graphite" }"#);
    assert_eq!(table(&built, SettingsCategory::Appearance), appearance);
    assert_eq!(
        table(&built, SettingsCategory::Terminal),
        [
            "termPreview: custom TerminalPreview (bare)",
            "font: custom Fonts",
            r#"fontSize: slider FontSize = 13 [9..24 by 1] "13 px""#,
            r#"lineHeight: slider LineHeight = 1.3 [1..2 by 0.05] "1.30""#,
            r#"letterSpacing: slider LetterSpacing = 0 [-1..3 by 0.5] "0 px""#,
            r#"padding: slider Padding = 10 [0..28 by 1] "10 px""#,
            r#"cursorStyle: segmented CursorStyle = "bar""#,
            "cursorBlink: toggle CursorBlink = true",
            r#"scrollback: slider Scrollback = 10000 [1000..50000 by 1000] "10,000""#,
        ]
    );
    assert_eq!(
        table(&built, SettingsCategory::Shell),
        [r#"shellDefault: select ShellDefault = """#]
    );
    assert_eq!(
        table(&built, SettingsCategory::Notifications),
        [
            "notifyOnIdle: toggle NotifyOnIdle = true",
            r#"notifyIdleMs: slider NotifyIdleMs = 3000 [1000..15000 by 500] "3.0 s""#,
            "notifyAway: toggle NotifyOnlyWhenAway = true",
        ]
    );
}

#[test]
fn the_entry_points_are_one_bare_block_each() {
    let built = World::new().build();
    assert_eq!(
        table(&built, SettingsCategory::Mobile),
        ["mobile: custom Mobile (bare)"]
    );
    assert_eq!(
        table(&built, SettingsCategory::Kits),
        ["kits: custom Kits (bare)"]
    );
    assert_eq!(
        table(&built, SettingsCategory::Accounts),
        ["accounts: custom Accounts (bare)"]
    );
    assert_eq!(
        table(&built, SettingsCategory::Shortcuts),
        ["keymap: custom Shortcuts (bare)"]
    );
}

#[test]
fn voice_has_every_row_of_the_old_schema_with_a_local_engine() {
    let built = World::new().build();
    assert_eq!(
        table(&built, SettingsCategory::Voice),
        [
            "voiceEnabled: toggle VoiceEnabled = false",
            r#"voiceShortcut: text "Ctrl+Alt+V""#,
            r#"voiceEngine: segmented VoiceEngine = "local""#,
            "voiceModel: custom VoiceModel",
            r#"voiceLanguage: select VoiceLanguage = """#,
            "voiceSubmit: toggle VoiceSubmit = false",
            "voiceUnload: action UnloadVoice",
            "voiceApiKey: custom VoiceKey",
            "voiceCleanup: toggle VoiceCleanup = false",
            r#"voiceCleanupModel: input VoiceCleanupModel = "" ("llama-3.1-8b-instant")"#,
            r#"voiceCleanupHint: input VoiceCleanupHint = "" ("Hangar.AI, pnpm, Tauri, simnJS")"#,
        ]
    );
}

#[test]
fn the_cloud_engine_swaps_the_model_card_for_a_model_choice_and_drops_the_unload_button() {
    let mut world = World::new();
    world.settings.voice_engine = VoiceEngine::Groq;
    let built = world.build();
    let voice = category(&built, SettingsCategory::Voice);
    let listed = ids(voice);
    assert!(!listed.contains(&"voiceModel"));
    assert!(!listed.contains(&"voiceUnload"));
    assert_eq!(
        describe(find(&built, "voiceCloudModel")),
        r#"select VoiceCloudModel = "whisper-large-v3-turbo""#
    );
    let Kind::Select { choices, .. } = &find(&built, "voiceCloudModel").kind else {
        panic!("not a select");
    };
    assert_eq!(
        choices.iter().map(|c| &*c.label).collect::<Vec<_>>(),
        [
            "Whisper large-v3 turbo — $0.04/h",
            "Whisper large-v3 — $0.111/h"
        ]
    );
    // The key card is always there, whatever the engine.
    assert!(listed.contains(&"voiceApiKey"));
}

#[test]
fn the_voice_language_offers_auto_then_eleven_codes() {
    let built = World::new().build();
    let Kind::Select { choices, .. } = &find(&built, "voiceLanguage").kind else {
        panic!("not a select");
    };
    let values: Vec<&str> = choices.iter().map(|c| c.value.as_str()).collect();
    assert_eq!(
        values,
        [
            "", "en", "fr", "de", "es", "it", "pt", "nl", "pl", "ru", "uk", "sv"
        ]
    );
    assert_eq!(choices[1].label, "en");
}

#[test]
fn discord_has_every_row_of_the_old_schema_with_its_default() {
    let built = World::new().build();
    assert_eq!(
        table(&built, SettingsCategory::Discord),
        [
            "discordCard: custom DiscordCard (bare)",
            "discordPresence: toggle DiscordPresence = false",
            "discordShowWorkspace: toggle DiscordShowWorkspace = true",
            "discordShowAgents: toggle DiscordShowAgents = true",
            r#"discordAppId: input DiscordAppId = "" ("Bundled application")"#,
        ]
    );
}

#[test]
fn about_shows_the_name_the_version_and_a_two_step_reset() {
    let built = World::new().build();
    assert_eq!(
        table(&built, SettingsCategory::About),
        [
            r#"appName: text "hangar-ai""#,
            r#"version: text "1.12.1""#,
            "reset: action ResetSettings (confirm) (danger)",
        ]
    );
    assert_eq!(find(&built, "appName").label.as_deref(), Some("Hangar.AI"));
}

#[test]
fn the_version_row_waits_for_its_value() {
    let mut world = World::new();
    world.version = String::new();
    let built = world.build();
    assert_eq!(
        ids(category(&built, SettingsCategory::About)),
        ["appName", "reset"]
    );
}

#[test]
fn the_rows_that_need_a_workspace_exist_only_with_one() {
    let without = World::new().build();
    for id in ["themeScope", "shellWorkspace"] {
        assert!(
            !without.iter().flat_map(items).any(|item| item.id == id),
            "{id}"
        );
    }
    let mut world = World::new();
    world.workspace = Some(workspace("Shop"));
    let with = world.build();
    let mut appearance = vec!["uiScale", "navigatorOpen", "inspectorOpen"];
    if cfg!(target_os = "linux") {
        appearance.push("systemTitleBar");
    }
    appearance.extend(["themeScope", "themePick"]);
    assert_eq!(
        ids(category(&with, SettingsCategory::Appearance)),
        appearance
    );
    assert_eq!(
        ids(category(&with, SettingsCategory::Shell)),
        ["shellDefault", "shellWorkspace"]
    );
    assert_eq!(
        describe(find(&with, "themeScope")),
        "toggle ThemeScope = false"
    );
    // The hint names the workspace it is about.
    assert!(
        find(&with, "themeScope")
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("Shop"))
    );
}

#[test]
fn a_pinned_theme_is_the_one_the_cards_show_as_active() {
    let mut world = World::new();
    let mut ws = workspace("Shop");
    ws.theme_id = Some("dracula".into());
    world.workspace = Some(ws);
    let built = world.build();
    assert_eq!(
        describe(find(&built, "themeScope")),
        "toggle ThemeScope = true"
    );
    assert_eq!(
        describe(find(&built, "themePick")),
        r#"custom Themes { active: "dracula" }"#
    );
    // Without a pin the cards show the global theme.
    let mut follows = World::new();
    follows.settings.theme_id = "nord".into();
    follows.workspace = Some(workspace("Shop"));
    assert_eq!(
        describe(find(&follows.build(), "themePick")),
        r#"custom Themes { active: "nord" }"#
    );
}

#[test]
fn the_theme_row_is_found_by_the_name_of_any_theme() {
    let built = World::new().build();
    let keywords = &find(&built, "themePick").keywords;
    assert!(keywords.starts_with("theme "));
    for theme in hangar_theme::themes() {
        assert!(keywords.contains(theme.name), "{}", theme.name);
    }
}

#[test]
fn the_shell_lists_what_was_detected_with_auto_first() {
    let mut world = World::new();
    world.shells = vec![shell("bash", "Bash"), shell("zsh", "Zsh")];
    let mut ws = workspace("Shop");
    ws.shell_id = Some("zsh".into());
    world.workspace = Some(ws);
    let built = world.build();
    let Kind::Select { choices, value, .. } = &find(&built, "shellDefault").kind else {
        panic!("not a select");
    };
    assert_eq!(value, "");
    assert_eq!(
        choices
            .iter()
            .map(|c| (c.value.as_str(), &*c.label))
            .collect::<Vec<_>>(),
        [("", "Auto (Bash)"), ("bash", "Bash"), ("zsh", "Zsh")]
    );
    let Kind::Select { choices, value, .. } = &find(&built, "shellWorkspace").kind else {
        panic!("not a select");
    };
    assert_eq!(value, "zsh");
    assert_eq!(choices[0].value, "");
    assert_eq!(choices.len(), 3);
    // The count of what was found is the section's hint, counted in its
    // language.
    assert_eq!(
        category(&built, SettingsCategory::Shell).sections[0]
            .hint
            .as_deref(),
        Some("2 detected")
    );
}

#[test]
fn auto_names_the_first_shell_or_says_there_is_none() {
    let none = World::new().build();
    let Kind::Select { choices, .. } = &find(&none, "shellDefault").kind else {
        panic!("not a select");
    };
    assert_eq!(choices[0].label, "Auto (none)");
    assert_eq!(
        category(&none, SettingsCategory::Shell).sections[0]
            .hint
            .as_deref(),
        Some("0 detected")
    );
}

#[test]
fn the_hibernate_delay_lists_the_usual_ones_and_the_current_value() {
    let usual = World::new().build();
    let Kind::Select { choices, value, .. } = &find(&usual, "hibernateAfter").kind else {
        panic!("not a select");
    };
    assert_eq!(value, "10");
    assert_eq!(
        choices
            .iter()
            .map(|c| (c.value.as_str(), &*c.label))
            .collect::<Vec<_>>(),
        [
            ("5", "5 min"),
            ("10", "10 min"),
            ("20", "20 min"),
            ("30", "30 min"),
            ("60", "60 min")
        ]
    );
    // A value edited into state.json by hand still shows, in its place.
    let mut world = World::new();
    world.settings.hibernate_after_min = 15.0;
    let hand_edited = world.build();
    let Kind::Select { choices, value, .. } = &find(&hand_edited, "hibernateAfter").kind else {
        panic!("not a select");
    };
    assert_eq!(value, "15");
    assert_eq!(
        choices.iter().map(|c| c.value.as_str()).collect::<Vec<_>>(),
        ["5", "10", "15", "20", "30", "60"]
    );
}

#[test]
fn the_hibernate_delay_stays_available_when_hibernation_is_off() {
    let mut world = World::new();
    world.settings.hibernate_idle = false;
    let built = world.build();
    // Nothing is ever disabled: the row is there, with its choices.
    assert!(matches!(
        find(&built, "hibernateAfter").kind,
        Kind::Select { .. }
    ));
}

#[test]
fn a_launch_command_row_shows_what_the_person_typed() {
    let mut world = World::new();
    world.settings.agent_commands.claude = Some("claude --verbose".into());
    let built = world.build();
    assert_eq!(
        describe(find(&built, "agentCommand.claude")),
        r#"input ClaudeCommand = "claude --verbose" ("claude")"#
    );
}

#[test]
fn a_hint_that_names_a_key_follows_the_rebinding() {
    let mut world = World::new();
    let built = world.build();
    let hint = |built: &[Category], id: &str| find(built, id).hint.clone().unwrap_or_default();
    assert!(
        hint(&built, "navigatorOpen").ends_with(" · Ctrl+Alt+N"),
        "{}",
        hint(&built, "navigatorOpen")
    );
    assert!(hint(&built, "inspectorOpen").ends_with(" · Ctrl+Alt+I"));
    world
        .settings
        .keybindings
        .insert("view.navigator".into(), serde_json::json!(["Ctrl+Shift+J"]));
    world
        .settings
        .keybindings
        .insert("view.inspector".into(), serde_json::json!([]));
    let rebound = world.build();
    assert!(hint(&rebound, "navigatorOpen").ends_with(" · Ctrl+Shift+J"));
    // Unbound: the hint says nothing of a key.
    assert!(!hint(&rebound, "inspectorOpen").contains('·'));
}

#[test]
fn the_voice_shortcut_row_is_the_live_binding_and_empty_when_unbound() {
    let mut world = World::new();
    world
        .settings
        .keybindings
        .insert("voice.dictate".into(), serde_json::json!(["F8"]));
    assert_eq!(
        describe(find(&world.build(), "voiceShortcut")),
        r#"text "F8""#
    );
    world
        .settings
        .keybindings
        .insert("voice.dictate".into(), serde_json::json!([]));
    assert_eq!(
        describe(find(&world.build(), "voiceShortcut")),
        r#"text """#
    );
}

#[test]
fn the_shortcuts_hint_names_the_platforms_modifier() {
    let mut world = World::new();
    let hint = |world: &World| {
        category(&world.build(), SettingsCategory::Shortcuts).sections[0]
            .hint
            .clone()
            .unwrap_or_default()
    };
    assert!(hint(&world).contains("Ctrl"), "{}", hint(&world));
    world.keyboard = KeyboardPlatform::Mac;
    assert!(hint(&world).contains("Cmd"), "{}", hint(&world));
}

#[test]
fn numbers_are_written_as_the_language_writes_them() {
    let mut world = World::new();
    world.locale = Locale::Fr;
    let built = world.build();
    let display = |id: &str| match &find(&built, id).kind {
        Kind::Slider { display, .. } => display.clone(),
        other => panic!("{id}: {other:?}"),
    };
    assert_eq!(display("lineHeight"), "1,30");
    assert_eq!(display("notifyIdleMs"), "3,0 s");
    assert_eq!(display("scrollback"), "10\u{202f}000");
    assert_eq!(display("uiScale"), "100 %");
}

#[test]
fn every_text_is_a_sentence_in_both_languages_with_no_placeholder_left() {
    for locale in Locale::ALL {
        let mut world = World::new();
        world.locale = locale;
        world.shells = vec![shell("bash", "Bash")];
        world.workspace = Some(workspace("Shop"));
        world.experiments = vec![Experiment {
            id: "demo",
            stage: Stage::Beta,
            title: Key::ExperimentsTitle,
            hint: Key::ExperimentsWarning,
            platforms: Some(&[Platform::Linux, Platform::Macos]),
        }];
        for engine in [VoiceEngine::Local, VoiceEngine::Groq] {
            world.settings.voice_engine = engine;
            for category in world.build() {
                let mut texts = vec![category.label.clone()];
                for section in &category.sections {
                    texts.push(section.title.clone());
                    texts.extend(section.hint.clone());
                    for item in &section.items {
                        texts.extend(item.label.clone());
                        texts.extend(item.hint.clone());
                        match &item.kind {
                            Kind::Select { choices, .. } | Kind::Segmented { choices, .. } => {
                                texts.extend(choices.iter().map(|c| c.label.to_string()))
                            }
                            Kind::Action { label, confirm, .. } => {
                                texts.push(label.clone());
                                texts.extend(confirm.clone());
                            }
                            Kind::Input { placeholder, .. } => texts.push(placeholder.to_string()),
                            _ => {}
                        }
                    }
                }
                for text in texts {
                    assert!(!text.is_empty(), "{locale:?} {:?}", category.id);
                    assert!(
                        !text.contains('{') && !text.contains('}'),
                        "{locale:?} {:?}: {text}",
                        category.id
                    );
                    // A key nobody wrote comes back as its own id.
                    assert!(
                        !text.starts_with("settings.") && !text.starts_with("mobile."),
                        "{locale:?}: {text}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_kits_and_accounts_pages_carry_their_own_titles_and_search_words() {
    let built = World::new().build();
    let kits = category(&built, SettingsCategory::Kits);
    assert_eq!(kits.label, "Kits");
    let intro = items(kits)[0].hint.clone().unwrap_or_default();
    assert!(!intro.is_empty());
    let accounts = category(&built, SettingsCategory::Accounts);
    assert!(items(accounts)[0].keywords.contains("claude account"));
    // Mobile's hint is there for the search, its keywords too.
    let mobile = items(category(&built, SettingsCategory::Mobile))[0];
    assert!(mobile.hint.is_some() && !mobile.keywords.is_empty());
    assert_eq!(mobile.label, None);
}

// ---- schema.experiments.test.ts -----------------------------------------------------------------

const DEMO: Experiment = Experiment {
    id: "demo-feature",
    stage: Stage::Beta,
    title: Key::ExperimentsTitle,
    hint: Key::ExperimentsWarning,
    platforms: Some(&[Platform::Linux, Platform::Macos]),
};

const OPEN_DEMO: Experiment = Experiment {
    platforms: None,
    ..DEMO
};

fn with_experiments(settings: Settings, experiments: &[Experiment]) -> Vec<Category> {
    let mut world = World::new();
    world.settings = settings;
    world.experiments = experiments.to_vec();
    world.build()
}

#[test]
fn experimental_is_absent_while_no_experiment_is_registered() {
    let built = with_experiments(Settings::default(), &[]);
    assert!(
        !built
            .iter()
            .any(|category| category.id == SettingsCategory::Experimental)
    );
}

#[test]
fn experimental_sits_before_about_once_something_is_registered() {
    let built = with_experiments(Settings::default(), &[OPEN_DEMO]);
    let order: Vec<SettingsCategory> = built.iter().map(|c| c.id).collect();
    let experimental = order
        .iter()
        .position(|id| *id == SettingsCategory::Experimental)
        .expect("the page exists");
    let about = order
        .iter()
        .position(|id| *id == SettingsCategory::About)
        .expect("about exists");
    assert_eq!(experimental, about - 1);
}

#[test]
fn an_experiment_is_turned_on_and_off_through_the_settings() {
    let built = with_experiments(Settings::default(), &[OPEN_DEMO]);
    let row = find(&built, "experiment.demo-feature");
    assert_eq!(
        describe(row),
        r#"toggle Experiment("demo-feature") = false"#
    );
    let Kind::Toggle { target, .. } = &row.kind else {
        panic!("not a toggle");
    };

    let scope = Scope {
        workspace: None,
        theme_id: "graphite",
    };
    let mut settings = Settings::default();
    let Some(StoreAction::UpdateSettings(patch)) =
        action(scope, &Change::Set(target.clone(), Value::Bool(true)))
    else {
        panic!("not a settings patch");
    };
    patch.apply(&mut settings);
    assert_eq!(settings.experiments, ["demo-feature"]);

    let other = Settings {
        experiments: vec!["demo-feature".into(), "other".into()],
        ..Settings::default()
    };
    let on = with_experiments(other.clone(), &[OPEN_DEMO]);
    let row = find(&on, "experiment.demo-feature");
    assert_eq!(describe(row), r#"toggle Experiment("demo-feature") = true"#);
    let mut after = other;
    let Some(StoreAction::UpdateSettings(patch)) =
        action(scope, &Change::Set(target.clone(), Value::Bool(false)))
    else {
        panic!("not a settings patch");
    };
    patch.apply(&mut after);
    // Turning one off leaves the others as they were.
    assert_eq!(after.experiments, ["other"]);
}

#[test]
fn a_feature_says_what_stage_it_is_at() {
    let built = with_experiments(Settings::default(), &[OPEN_DEMO]);
    let hint = find(&built, "experiment.demo-feature")
        .hint
        .clone()
        .unwrap_or_default();
    assert!(hint.contains("Beta"), "{hint}");
}

#[test]
fn a_platform_bound_feature_names_its_platforms_in_the_hint() {
    let built = with_experiments(Settings::default(), &[DEMO]);
    let hint = find(&built, "experiment.demo-feature")
        .hint
        .clone()
        .unwrap_or_default();
    assert_eq!(
        hint.split(" · ").take(2).collect::<Vec<_>>(),
        ["Beta", "Only on Linux, macOS"]
    );
}

#[test]
fn a_feature_this_machine_cannot_run_is_not_offered() {
    let mut world = World::new();
    world.platform = Some(Platform::Windows);
    world.experiments = vec![DEMO];
    assert!(
        !world
            .build()
            .iter()
            .any(|category| category.id == SettingsCategory::Experimental)
    );
}

#[test]
fn the_page_warns_that_the_features_are_unfinished() {
    let built = with_experiments(Settings::default(), &[OPEN_DEMO]);
    let page = category(&built, SettingsCategory::Experimental);
    assert_eq!(page.label, "Experimental");
    assert_eq!(page.sections[0].title, "Experimental features");
    assert!(
        page.sections[0]
            .hint
            .as_deref()
            .is_some_and(|hint| hint.starts_with("Unfinished work"))
    );
}
