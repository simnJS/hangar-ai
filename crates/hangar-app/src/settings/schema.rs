//! The settings, described as data (`schema.tsx`): a new option is one entry
//! in [`build_categories`], and the page draws its control, files it under its
//! category and matches it against the search box for free.
//!
//! The description holds no closures. A row names the [`Target`] it edits and
//! the control reports a [`Value`] (change.rs), so the whole schema can be
//! compared, searched and tested as plain values, and rebuilt on every frame
//! from the settings the way the TS rebuilt it on every render.
//!
//! Which rows exist is decided here and nowhere else, in code and not in a
//! condition table: the rows that need an active workspace, the voice rows
//! that depend on the engine, the Experimental page that only exists while
//! the registry is not empty, the version row that waits for its value.
//! Nothing is ever disabled (the hibernate delay stays enabled when
//! hibernation is off).

use std::borrow::Cow;

use hangar_i18n::{Key, Locale, Translator, args};
use hangar_ui::Icon;
use hangar_ui::shortcut::Platform as KeyboardPlatform;

use super::change::{Target, cursor_style_id, voice_engine_id};
use super::experiments::{Experiment, Platform, available_experiments};
use super::format::{num, plain};
use crate::commands::CommandId;
use crate::keymap::KeymapIndex;
use crate::panes_logic::agents::DEFAULT_CLAUDE_COMMAND;
use crate::shell::SettingsCategory;
use crate::state::types::{
    AgentId, CursorStyle, Settings, ShellInfo, UI_SCALE_MAX, UI_SCALE_MIN, VoiceEngine, Workspace,
};

/// A choice of a select or a segmented control.
///
/// The label of a segmented control's choice is always a plain entry of the
/// dictionary or a constant, which the translator hands out borrowed for
/// `'static`: the control wants text that outlives the frame it is drawn in
/// (the schema is rebuilt for every frame and cannot lend it), and this is the
/// text that does. A select owns its items, so its labels may be formatted.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub value: String,
    pub label: Cow<'static, str>,
}

impl Choice {
    fn new(value: impl Into<String>, label: impl Into<Cow<'static, str>>) -> Choice {
        Choice {
            value: value.into(),
            label: label.into(),
        }
    }

    /// The label for a control that borrows it. Empty for a label that was
    /// made on the fly, which a segmented control never has.
    pub fn static_label(&self) -> &'static str {
        match &self.label {
            Cow::Borrowed(label) => label,
            Cow::Owned(_) => "",
        }
    }
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// What a two-step button does once confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// `updateSettings(DEFAULT_SETTINGS)`.
    ResetSettings,
    /// `voice_unload`: the resident model leaves memory.
    UnloadVoice,
}

/// A block that does not fit a label and a control: the page draws it full
/// width. Each is drawn by its own module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// The path field and its Browse button.
    ProjectsFolder,
    /// The theme cards, `active` being the one the window shows.
    Themes {
        active: String,
    },
    TerminalPreview,
    Fonts,
    Mobile,
    /// P14's (`kits::settings_view`).
    Kits,
    /// P8's (`accounts::settings_page`).
    Accounts,
    VoiceModel,
    VoiceKey,
    DiscordCard,
    Shortcuts,
}

/// What a row is made of.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Select {
        value: String,
        choices: Vec<Choice>,
        target: Target,
    },
    Segmented {
        value: String,
        choices: Vec<Choice>,
        target: Target,
    },
    Toggle {
        value: bool,
        target: Target,
    },
    /// Free text: an identifier, a path, anything with no shortlist to pick
    /// from.
    Input {
        value: String,
        /// A constant or a plain dictionary entry, for the same reason as a
        /// segmented choice's label.
        placeholder: Cow<'static, str>,
        target: Target,
    },
    Slider {
        value: f64,
        min: f64,
        max: f64,
        step: f64,
        /// The value formatted, next to the track.
        display: String,
        target: Target,
    },
    /// A read-only line: a version, a keyboard shortcut.
    Text {
        value: String,
    },
    Action {
        label: String,
        /// Set to require a second click, for anything not worth undoing by
        /// hand.
        confirm: Option<String>,
        danger: bool,
        act: Act,
    },
    /// Drawn full width. `bare`: straight on the page rather than inside a
    /// grouped box, for a block that is a box of its own (a preview, the
    /// shortcut editor).
    Custom {
        bare: bool,
        block: Block,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: String,
    /// A custom block may have none.
    pub label: Option<String>,
    pub hint: Option<String>,
    /// Extra words the search box matches, beyond the label and the hint.
    pub keywords: String,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub id: &'static str,
    pub title: String,
    pub hint: Option<String>,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Category {
    pub id: SettingsCategory,
    pub label: String,
    pub icon: Icon,
    pub sections: Vec<Section>,
}

impl Category {
    pub fn item_count(&self) -> usize {
        self.sections
            .iter()
            .map(|section| section.items.len())
            .sum()
    }
}

/// What the schema is built from (`SettingsContext`).
#[derive(Clone, Copy)]
pub struct Context<'a> {
    pub t: Translator,
    pub settings: &'a Settings,
    pub shells: &'a [ShellInfo],
    /// The active workspace.
    pub workspace: Option<&'a Workspace>,
    /// Empty until known: the row hides itself meanwhile.
    pub version: &'a str,
    /// The experiments to offer. The app passes the shipped list; only tests
    /// pass their own.
    pub experiments: &'a [Experiment],
    pub platform: Option<Platform>,
    /// The resolved keymap (`Model.live.keymap.index`): the keys the hints
    /// name, and the platform they are spelled for.
    pub keymap: &'a KeymapIndex,
}

/// The delays the settings offer for hibernation, in minutes.
pub const HIBERNATE_DELAYS: [f64; 5] = [5.0, 10.0, 20.0, 30.0, 60.0];

/// The shipped terminal font, verbatim: a fresh install must land on a named
/// card rather than on "Custom".
pub const DEFAULT_FONT_STACK: &str =
    r#""Cascadia Code", "JetBrains Mono", Consolas, "Courier New", monospace"#;

/// The font cards, in order: a name and the stack it stands for.
pub const FONT_STACKS: [(&str, &str); 7] = [
    ("Cascadia Code", DEFAULT_FONT_STACK),
    (
        "JetBrains Mono",
        r#""JetBrains Mono", "Fira Code", Consolas, monospace"#,
    ),
    (
        "Fira Code",
        r#""Fira Code", "Cascadia Code", Consolas, monospace"#,
    ),
    ("Consolas", r#"Consolas, "Courier New", monospace"#),
    ("Hack", r#"Hack, "DejaVu Sans Mono", Consolas, monospace"#),
    ("IBM Plex Mono", r#""IBM Plex Mono", Consolas, monospace"#),
    (
        "System",
        "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
    ),
];

/// Groq's transcription models, cheapest first (`CLOUD_MODELS`): id, name,
/// price. Turbo is the default because at $0.04 an hour against $0.111 it is
/// the one that makes the cloud fallback cost nothing worth noticing, and
/// dictation is too short for the accuracy gap to show.
pub const CLOUD_MODELS: [(&str, &str, &str); 2] = [
    (
        "whisper-large-v3-turbo",
        "Whisper large-v3 turbo",
        "$0.04/h",
    ),
    ("whisper-large-v3", "Whisper large-v3", "$0.111/h"),
];

/// Mirrors `cleanup::DEFAULT_MODEL` of hangar-core (a private module there),
/// and is only ever shown as a placeholder.
pub const DEFAULT_CLEANUP_MODEL: &str = "llama-3.1-8b-instant";

/// Offered in the language picker. Not the ninety-nine Whisper handles, just
/// the ones worth a click, with "detect" covering the rest.
pub const VOICE_LANGUAGES: [&str; 11] = [
    "en", "fr", "de", "es", "it", "pt", "nl", "pl", "ru", "uk", "sv",
];

/// The categories in the order of the page.
pub const ORDER: [SettingsCategory; 13] = [
    SettingsCategory::General,
    SettingsCategory::Appearance,
    SettingsCategory::Terminal,
    SettingsCategory::Shell,
    SettingsCategory::Notifications,
    SettingsCategory::Mobile,
    SettingsCategory::Kits,
    SettingsCategory::Accounts,
    SettingsCategory::Voice,
    SettingsCategory::Discord,
    SettingsCategory::Shortcuts,
    SettingsCategory::Experimental,
    SettingsCategory::About,
];

fn item(id: &str, label: Option<String>, kind: Kind) -> Item {
    Item {
        id: id.to_string(),
        label,
        hint: None,
        keywords: String::new(),
        kind,
    }
}

fn toggle(id: &str, label: String, value: bool, target: Target) -> Item {
    item(id, Some(label), Kind::Toggle { value, target })
}

impl Item {
    fn hint(mut self, hint: impl Into<String>) -> Item {
        self.hint = Some(hint.into());
        self
    }

    fn keywords(mut self, keywords: impl Into<String>) -> Item {
        self.keywords = keywords.into();
        self
    }
}

fn section(id: &'static str, title: String, items: Vec<Item>) -> Section {
    Section {
        id,
        title,
        hint: None,
        items,
    }
}

impl Section {
    fn hint(mut self, hint: impl Into<String>) -> Section {
        self.hint = Some(hint.into());
        self
    }
}

fn slider(
    id: &str,
    label: String,
    value: f64,
    range: (f64, f64),
    step: f64,
    display: String,
    target: Target,
) -> Item {
    item(
        id,
        Some(label),
        Kind::Slider {
            value,
            min: range.0,
            max: range.1,
            step,
            display,
            target,
        },
    )
}

/// The full schema, in the order of the page (`buildCategories`).
pub fn build_categories(ctx: &Context) -> Vec<Category> {
    let Context {
        t,
        settings,
        shells,
        workspace,
        version,
        ..
    } = *ctx;
    let tx = |key: Key| t.t(key).into_owned();
    let locale = t.locale();

    // The page only exists while there is something to put on it, so a
    // version with no experiment shows no empty "Experimental" entry.
    let experiments = available_experiments(ctx.experiments, ctx.platform);

    // A workspace can pin its own theme; otherwise it follows the global one.
    let pinned_theme = workspace.and_then(|ws| ws.theme_id.as_deref());
    let effective_theme = pinned_theme.unwrap_or(&settings.theme_id);

    // The keymap as the person has it, so a hint that names a key stays true
    // after the command has been rebound, and says nothing once it is
    // unbound.
    let keymap = ctx.keymap;
    let with_key = |hint: String, id: CommandId| {
        let keys = keymap.label(id);
        if keys.is_empty() {
            hint
        } else {
            format!("{hint} · {keys}")
        }
    };

    let shell_choices = || shells.iter().map(|s| Choice::new(&s.id, s.label.clone()));

    vec![
        general(ctx, locale),
        Category {
            id: SettingsCategory::Appearance,
            label: tx(Key::SettingsCatAppearance),
            icon: Icon::Appearance,
            sections: vec![
                section(
                    "interface",
                    tx(Key::SettingsInterface),
                    vec![
                        slider(
                            "uiScale",
                            tx(Key::SettingsUiScale),
                            settings.ui_scale,
                            (UI_SCALE_MIN, UI_SCALE_MAX),
                            5.0,
                            format!("{} %", plain(settings.ui_scale)),
                            Target::UiScale,
                        )
                        .hint(tx(Key::SettingsUiScaleHint))
                        .keywords(
                            "zoom scale size taille texte text interface ui police font",
                        ),
                        toggle(
                            "navigatorOpen",
                            tx(Key::SettingsShowNavigator),
                            settings.navigator_open,
                            Target::NavigatorOpen,
                        )
                        .hint(with_key(
                            tx(Key::SettingsShowNavigatorHint),
                            CommandId::ViewNavigator,
                        ))
                        .keywords(
                            "navigator navigateur sidebar barre latérale workspaces panneau gauche left",
                        ),
                        toggle(
                            "inspectorOpen",
                            tx(Key::SettingsShowInspector),
                            settings.inspector_open,
                            Target::InspectorOpen,
                        )
                        .hint(with_key(
                            tx(Key::SettingsShowInspectorHint),
                            CommandId::ViewInspector,
                        ))
                        .keywords("inspector inspecteur panneau droite right details détails"),
                    ],
                ),
                section("theme", tx(Key::SettingsTheme), {
                    let mut items = Vec::new();
                    if let Some(ws) = workspace {
                        items.push(
                            toggle(
                                "themeScope",
                                tx(Key::SettingsThemePerWorkspace),
                                pinned_theme.is_some(),
                                Target::ThemeScope,
                            )
                            .hint(
                                t.t_with(
                                    Key::SettingsThemePerWorkspaceHint,
                                    &args!("name" => ws.name.as_str()),
                                ),
                            ),
                        );
                    }
                    let names: Vec<&str> =
                        hangar_theme::themes().iter().map(|theme| theme.name).collect();
                    items.push(
                        item(
                            "themePick",
                            None,
                            Kind::Custom {
                                bare: false,
                                block: Block::Themes {
                                    active: effective_theme.to_string(),
                                },
                            },
                        )
                        .keywords(format!("theme {}", names.join(" "))),
                    );
                    items
                })
                .hint(tx(Key::SettingsThemeHint)),
            ],
        },
        terminal(ctx, locale),
        Category {
            id: SettingsCategory::Shell,
            label: tx(Key::SettingsCatShell),
            icon: Icon::Prompt,
            sections: vec![
                section("shell", tx(Key::SettingsShell), {
                    let mut default_choices = vec![Choice::new(
                        "",
                        t.t_with(
                            Key::SettingsShellAuto,
                            &args!("name" => shells
                                .first()
                                .map_or_else(|| tx(Key::CreateShellNone), |s| s.label.clone())),
                        ),
                    )];
                    default_choices.extend(shell_choices());
                    let mut items = vec![item(
                        "shellDefault",
                        Some(tx(Key::SettingsShellDefault)),
                        Kind::Select {
                            value: settings.shell_id.clone().unwrap_or_default(),
                            choices: default_choices,
                            target: Target::ShellDefault,
                        },
                    )];
                    if let Some(ws) = workspace {
                        let mut choices = vec![Choice::new("", tx(Key::SettingsShellFollow))];
                        choices.extend(shell_choices());
                        items.push(
                            item(
                                "shellWorkspace",
                                Some(tx(Key::SettingsShellWorkspace)),
                                Kind::Select {
                                    value: ws.shell_id.clone().unwrap_or_default(),
                                    choices,
                                    target: Target::ShellWorkspace,
                                },
                            )
                            .hint(tx(Key::SettingsShellWorkspaceHint)),
                        );
                    }
                    items
                })
                .hint(t.t_with(Key::SettingsShellDetected, &args!("n" => shells.len()))),
            ],
        },
        Category {
            id: SettingsCategory::Notifications,
            label: tx(Key::SettingsCatNotifications),
            icon: Icon::Bell,
            sections: vec![section(
                "notifications",
                tx(Key::SettingsNotifications),
                vec![
                    toggle(
                        "notifyOnIdle",
                        tx(Key::SettingsNotifyOnIdle),
                        settings.notify_on_idle,
                        Target::NotifyOnIdle,
                    )
                    .hint(tx(Key::SettingsNotifyOnIdleHint)),
                    slider(
                        "notifyIdleMs",
                        tx(Key::SettingsNotifyIdle),
                        settings.notify_idle_ms as f64,
                        (1000.0, 15000.0),
                        500.0,
                        format!("{} s", num(locale, settings.notify_idle_ms as f64 / 1000.0, 1)),
                        Target::NotifyIdleMs,
                    ),
                    toggle(
                        "notifyAway",
                        tx(Key::SettingsNotifyAway),
                        settings.notify_only_when_away,
                        Target::NotifyOnlyWhenAway,
                    )
                    .hint(tx(Key::SettingsNotifyAwayHint)),
                ],
            )],
        },
        entry_point(
            SettingsCategory::Mobile,
            tx(Key::SettingsCatMobile),
            Icon::Broadcast,
            "mobile",
            Some(tx(Key::MobileSwitchHint)),
            tx(Key::MobileKeywords),
            Block::Mobile,
        ),
        entry_point(
            SettingsCategory::Kits,
            tx(Key::KitsManageTitle),
            Icon::Plug,
            "kits",
            Some(tx(Key::KitsManageIntro)),
            tx(Key::KitsManageKeywords),
            Block::Kits,
        ),
        entry_point(
            SettingsCategory::Accounts,
            tx(Key::AccountsSettingsTitle),
            Icon::Bolt,
            "accounts",
            Some(tx(Key::AccountsSettingsIntro)),
            "claude account accounts compte comptes abonnement subscription login".to_string(),
            Block::Accounts,
        ),
        voice(ctx, keymap),
        discord(ctx),
        Category {
            id: SettingsCategory::Shortcuts,
            label: tx(Key::SettingsCatShortcuts),
            icon: Icon::Keyboard,
            sections: vec![
                section(
                    "keymap",
                    tx(Key::SettingsShortcutsAll),
                    vec![
                        // One block rather than a row per command: the editor
                        // brings its own filter, and every command carries
                        // several keys plus their conflicts, which no
                        // label-and-control row could hold.
                        item(
                            "keymap",
                            None,
                            Kind::Custom {
                                bare: true,
                                block: Block::Shortcuts,
                            },
                        )
                        .keywords(
                            "shortcut shortcuts keyboard key keys binding keymap raccourci raccourcis clavier touche ctrl alt shift",
                        ),
                    ],
                )
                // The sequence it names is a real default, so it has to be
                // spelled the way this platform spells it.
                .hint(t.t_with(
                    Key::SettingsShortcutsHint,
                    &args!("mod" => if keymap.platform() == KeyboardPlatform::Mac { "Cmd" } else { "Ctrl" }),
                )),
            ],
        },
    ]
    .into_iter()
    .chain(experimental(ctx, &experiments))
    .chain([about(ctx, version)])
    .collect()
}

fn general(ctx: &Context, locale: Locale) -> Category {
    let Context { t, settings, .. } = *ctx;
    let tx = |key: Key| t.t(key).into_owned();

    let mut delays: Vec<f64> = HIBERNATE_DELAYS.to_vec();
    if !delays.contains(&settings.hibernate_after_min) {
        delays.push(settings.hibernate_after_min);
    }
    delays.sort_by(f64::total_cmp);

    let mut language_choices = vec![Choice::new("", t.t(Key::SettingsLocaleAuto))];
    language_choices.extend(
        hangar_i18n::LOCALES
            .iter()
            .map(|option| Choice::new(option.id.id(), option.label)),
    );

    Category {
        id: SettingsCategory::General,
        label: tx(Key::SettingsCatGeneral),
        icon: Icon::Sliders,
        sections: vec![
            section(
                "language",
                tx(Key::SettingsLanguage),
                vec![
                    item(
                        "locale",
                        Some(tx(Key::SettingsLanguageLabel)),
                        Kind::Segmented {
                            value: settings.locale.map_or_else(String::new, |l| l.id().into()),
                            choices: language_choices,
                            target: Target::Locale,
                        },
                    )
                    .keywords("langue language locale"),
                ],
            ),
            section(
                "projects",
                tx(Key::SettingsProjectsFolder),
                vec![
                    item(
                        "projectsFolder",
                        Some(tx(Key::SettingsProjectsFolder)),
                        Kind::Custom {
                            bare: false,
                            block: Block::ProjectsFolder,
                        },
                    )
                    .hint(tx(Key::SettingsProjectsFolderHint))
                    .keywords(
                        "git clone repository repo dépôt cloner dossier projets folder",
                    ),
                ],
            ),
            section(
                "sessions",
                tx(Key::SettingsSessions),
                vec![
                    toggle(
                        "autoResume",
                        tx(Key::SettingsAutoResume),
                        settings.auto_resume,
                        Target::AutoResume,
                    )
                    .hint(tx(Key::SettingsAutoResumeHint)),
                    toggle(
                        "claudeBridge",
                        tx(Key::SettingsClaudeBridge),
                        settings.claude_bridge,
                        Target::ClaudeBridge,
                    )
                    .hint(tx(Key::SettingsClaudeBridgeHint))
                    .keywords("mod plugin hangar-bridge claude code permission usage forfait"),
                    slider(
                        "launchDelay",
                        tx(Key::SettingsLaunchDelay),
                        settings.launch_delay_ms as f64,
                        (0.0, 4000.0),
                        100.0,
                        format!("{} ms", settings.launch_delay_ms),
                        Target::LaunchDelayMs,
                    )
                    .hint(tx(Key::SettingsLaunchDelayHint)),
                ],
            ),
            section(
                "memory",
                tx(Key::SettingsMemory),
                vec![
                    toggle(
                        "hibernateIdle",
                        tx(Key::SettingsHibernate),
                        settings.hibernate_idle,
                        Target::HibernateIdle,
                    )
                    .hint(tx(Key::SettingsHibernateHint))
                    .keywords(
                        "sleep veille hibernate hibernation memory mémoire ram idle inactif kill arrêter",
                    ),
                    item(
                        "hibernateAfter",
                        Some(tx(Key::SettingsHibernateDelay)),
                        Kind::Select {
                            // A value edited into state.json by hand still
                            // shows, rather than the list silently claiming
                            // another one.
                            value: plain(settings.hibernate_after_min),
                            choices: delays
                                .iter()
                                .map(|minutes| {
                                    Choice::new(
                                        plain(*minutes),
                                        format!("{} min", num(locale, *minutes, 0)),
                                    )
                                })
                                .collect(),
                            target: Target::HibernateAfterMin,
                        },
                    )
                    .hint(tx(Key::SettingsHibernateDelayHint))
                    .keywords("sleep veille delay délai minutes"),
                ],
            ),
            section(
                "agentControl",
                tx(Key::SettingsAgentControlSection),
                vec![
                    toggle(
                        "agentPaneControl",
                        tx(Key::SettingsAgentPaneControl),
                        settings.agent_pane_control,
                        Target::AgentPaneControl,
                    )
                    .hint(tx(Key::SettingsAgentPaneControlHint))
                    .keywords(
                        "mcp agents panes terminals orchestrate orchestrer contrôle control pilot",
                    ),
                ],
            ),
            section(
                "agentCommands",
                tx(Key::SettingsAgentCommands),
                vec![
                    item(
                        "agentCommand.claude",
                        Some(AgentId::Claude.info().label.to_string()),
                        Kind::Input {
                            value: settings.agent_commands.claude.clone().unwrap_or_default(),
                            // The default as the placeholder, so an empty
                            // field reads as "starts with this" rather than
                            // as nothing at all.
                            placeholder: DEFAULT_CLAUDE_COMMAND.into(),
                            target: Target::ClaudeCommand,
                        },
                    )
                    .keywords("claude command commande launch lancement alias"),
                ],
            )
            .hint(tx(Key::SettingsAgentCommandsHint)),
        ],
    }
}

fn terminal(ctx: &Context, locale: Locale) -> Category {
    let Context { t, settings, .. } = *ctx;
    let tx = |key: Key| t.t(key).into_owned();
    Category {
        id: SettingsCategory::Terminal,
        label: tx(Key::SettingsCatTerminal),
        icon: Icon::Terminal,
        sections: vec![
            section(
                "preview",
                tx(Key::SettingsPreview),
                vec![
                    item(
                        "termPreview",
                        None,
                        Kind::Custom {
                            bare: true,
                            block: Block::TerminalPreview,
                        },
                    )
                    .keywords("preview aperçu"),
                ],
            ),
            section(
                "typography",
                tx(Key::SettingsTypography),
                vec![
                    item(
                        "font",
                        Some(tx(Key::SettingsFont)),
                        Kind::Custom {
                            bare: false,
                            block: Block::Fonts,
                        },
                    )
                    .hint(tx(Key::SettingsFontHint))
                    .keywords(format!(
                        "font police {}",
                        FONT_STACKS
                            .iter()
                            .map(|(name, _)| *name)
                            .collect::<Vec<_>>()
                            .join(" ")
                    )),
                    slider(
                        "fontSize",
                        tx(Key::SettingsSize),
                        settings.font_size,
                        (9.0, 24.0),
                        1.0,
                        format!("{} px", plain(settings.font_size)),
                        Target::FontSize,
                    ),
                    slider(
                        "lineHeight",
                        tx(Key::SettingsLineHeight),
                        settings.line_height,
                        (1.0, 2.0),
                        0.05,
                        num(locale, settings.line_height, 2),
                        Target::LineHeight,
                    ),
                    slider(
                        "letterSpacing",
                        tx(Key::SettingsLetterSpacing),
                        settings.letter_spacing,
                        (-1.0, 3.0),
                        0.5,
                        format!("{} px", plain(settings.letter_spacing)),
                        Target::LetterSpacing,
                    ),
                    slider(
                        "padding",
                        tx(Key::SettingsPadding),
                        settings.padding,
                        (0.0, 28.0),
                        1.0,
                        format!("{} px", plain(settings.padding)),
                        Target::Padding,
                    ),
                ],
            ),
            section(
                "cursor",
                tx(Key::SettingsCursorSection),
                vec![
                    item(
                        "cursorStyle",
                        Some(tx(Key::SettingsCursor)),
                        Kind::Segmented {
                            value: cursor_style_id(settings.cursor_style).into(),
                            choices: vec![
                                Choice::new(
                                    cursor_style_id(CursorStyle::Bar),
                                    t.t(Key::SettingsCursorBar),
                                ),
                                Choice::new(
                                    cursor_style_id(CursorStyle::Block),
                                    t.t(Key::SettingsCursorBlock),
                                ),
                                Choice::new(
                                    cursor_style_id(CursorStyle::Underline),
                                    t.t(Key::SettingsCursorUnderline),
                                ),
                            ],
                            target: Target::CursorStyle,
                        },
                    ),
                    toggle(
                        "cursorBlink",
                        tx(Key::SettingsBlink),
                        settings.cursor_blink,
                        Target::CursorBlink,
                    ),
                ],
            ),
            section(
                "history",
                tx(Key::SettingsHistory),
                vec![
                    slider(
                        "scrollback",
                        tx(Key::SettingsScrollback),
                        settings.scrollback as f64,
                        (1000.0, 50000.0),
                        1000.0,
                        num(locale, settings.scrollback as f64, 0),
                        Target::Scrollback,
                    )
                    .hint(t.t_with(
                        Key::SettingsScrollbackLines,
                        &args!("n" => settings.scrollback),
                    ))
                    .keywords("scrollback historique history lignes lines buffer"),
                ],
            ),
        ],
    }
}

/// A category that is one block of another module's (mobile, kits, accounts).
fn entry_point(
    id: SettingsCategory,
    label: String,
    icon: Icon,
    section_id: &'static str,
    hint: Option<String>,
    keywords: String,
    block: Block,
) -> Category {
    let mut entry = item(section_id, None, Kind::Custom { bare: true, block }).keywords(keywords);
    entry.hint = hint;
    Category {
        id,
        label: label.clone(),
        icon,
        sections: vec![section(section_id, label, vec![entry])],
    }
}

fn voice(ctx: &Context, keymap: &KeymapIndex) -> Category {
    let Context { t, settings, .. } = *ctx;
    let tx = |key: Key| t.t(key).into_owned();
    let local = settings.voice_engine == VoiceEngine::Local;

    let mut main = vec![
        toggle(
            "voiceEnabled",
            tx(Key::SettingsVoiceEnable),
            settings.voice_enabled,
            Target::VoiceEnabled,
        )
        .hint(tx(Key::SettingsVoiceEnableHint))
        .keywords("voice dictation microphone dictée micro parole whisper parakeet"),
        // Read from the live keymap rather than the defaults: the whole point
        // of the row is to answer "which key was it again", and it has to
        // stay right after the command has been rebound.
        item(
            "voiceShortcut",
            Some(tx(Key::SettingsVoiceShortcut)),
            Kind::Text {
                value: keymap.label(CommandId::VoiceDictate).to_string(),
            },
        ),
        item(
            "voiceEngine",
            Some(tx(Key::SettingsVoiceEngine)),
            Kind::Segmented {
                value: voice_engine_id(settings.voice_engine).into(),
                choices: vec![
                    Choice::new(
                        voice_engine_id(VoiceEngine::Local),
                        t.t(Key::SettingsVoiceEngineLocal),
                    ),
                    Choice::new(
                        voice_engine_id(VoiceEngine::Groq),
                        t.t(Key::SettingsVoiceEngineCloud),
                    ),
                ],
                target: Target::VoiceEngine,
            },
        )
        .hint(tx(Key::SettingsVoiceEngineHint))
        .keywords("local cloud groq offline hors ligne"),
    ];
    if local {
        main.push(
            item(
                "voiceModel",
                Some(tx(Key::SettingsVoiceModel)),
                Kind::Custom {
                    bare: false,
                    block: Block::VoiceModel,
                },
            )
            .keywords("parakeet model modèle download télécharger"),
        );
    } else {
        main.push(
            item(
                "voiceCloudModel",
                Some(tx(Key::SettingsVoiceCloudModel)),
                Kind::Select {
                    value: settings.voice_cloud_model.clone(),
                    choices: CLOUD_MODELS
                        .iter()
                        .map(|(id, name, price)| Choice::new(*id, format!("{name} — {price}")))
                        .collect(),
                    target: Target::VoiceCloudModel,
                },
            )
            .hint(tx(Key::SettingsVoiceCloudModelHint))
            .keywords("whisper turbo groq model modèle"),
        );
    }
    main.push(
        item(
            "voiceLanguage",
            Some(tx(Key::SettingsVoiceLanguage)),
            Kind::Select {
                value: settings.voice_language.clone(),
                choices: std::iter::once(Choice::new("", tx(Key::SettingsVoiceLanguageAuto)))
                    .chain(VOICE_LANGUAGES.iter().map(|code| Choice::new(*code, *code)))
                    .collect(),
                target: Target::VoiceLanguage,
            },
        )
        .hint(tx(Key::SettingsVoiceLanguageHint))
        .keywords("language langue français english accent"),
    );
    main.push(
        toggle(
            "voiceSubmit",
            tx(Key::SettingsVoiceSubmit),
            settings.voice_submit,
            Target::VoiceSubmit,
        )
        .hint(tx(Key::SettingsVoiceSubmitHint))
        .keywords("enter entrée submit envoyer"),
    );
    if local {
        // The model stays in memory between dictations, which is what makes the
        // second one instant and the first one slow. Worth a way out for anyone
        // who dictated once this morning and would like the gigabyte back.
        main.push(
            item(
                "voiceUnload",
                Some(tx(Key::SettingsVoiceUnload)),
                Kind::Action {
                    label: tx(Key::SettingsVoiceUnloadAction),
                    confirm: None,
                    danger: false,
                    act: Act::UnloadVoice,
                },
            )
            .keywords("memory mémoire ram unload décharger"),
        );
    }

    Category {
        id: SettingsCategory::Voice,
        label: tx(Key::SettingsCatVoice),
        icon: Icon::Mic,
        sections: vec![
            section("voice", tx(Key::SettingsVoice), main).hint(tx(Key::SettingsVoiceHint)),
            section(
                "voiceKey",
                tx(Key::SettingsVoiceEngineCloud),
                vec![
                    item(
                        "voiceApiKey",
                        Some(tx(Key::SettingsVoiceKey)),
                        Kind::Custom {
                            bare: false,
                            block: Block::VoiceKey,
                        },
                    )
                    .keywords("groq api key clé token keychain trousseau"),
                ],
            ),
            section(
                "voiceCleanup",
                tx(Key::SettingsVoiceCleanupSection),
                vec![
                    toggle(
                        "voiceCleanup",
                        tx(Key::SettingsVoiceCleanup),
                        settings.voice_cleanup,
                        Target::VoiceCleanup,
                    )
                    .hint(tx(Key::SettingsVoiceCleanupHintRow))
                    .keywords("cleanup llm nettoyage relecture ponctuation"),
                    item(
                        "voiceCleanupModel",
                        Some(tx(Key::SettingsVoiceCleanupModel)),
                        Kind::Input {
                            value: settings.voice_cleanup_model.clone(),
                            placeholder: DEFAULT_CLEANUP_MODEL.into(),
                            target: Target::VoiceCleanupModel,
                        },
                    )
                    .hint(t.t_with(
                        Key::SettingsVoiceCleanupModelHint,
                        &args!("model" => DEFAULT_CLEANUP_MODEL),
                    )),
                    item(
                        "voiceCleanupHint",
                        Some(tx(Key::SettingsVoiceVocabulary)),
                        Kind::Input {
                            value: settings.voice_cleanup_hint.clone(),
                            placeholder: t.t(Key::SettingsVoiceVocabularyPlaceholder),
                            target: Target::VoiceCleanupHint,
                        },
                    )
                    .hint(tx(Key::SettingsVoiceVocabularyHint))
                    .keywords("vocabulary vocabulaire jargon noms spelling"),
                ],
            ),
        ],
    }
}

fn discord(ctx: &Context) -> Category {
    let Context { t, settings, .. } = *ctx;
    let tx = |key: Key| t.t(key).into_owned();
    Category {
        id: SettingsCategory::Discord,
        label: tx(Key::SettingsCatDiscord),
        icon: Icon::Gamepad,
        sections: vec![
            section(
                "discordPreview",
                tx(Key::SettingsPreview),
                vec![
                    item(
                        "discordCard",
                        None,
                        Kind::Custom {
                            bare: true,
                            block: Block::DiscordCard,
                        },
                    )
                    .keywords("discord rich presence statut status carte card"),
                ],
            ),
            section(
                "discord",
                tx(Key::SettingsDiscord),
                vec![
                    toggle(
                        "discordPresence",
                        tx(Key::SettingsDiscordEnable),
                        settings.discord_presence,
                        Target::DiscordPresence,
                    )
                    .hint(tx(Key::SettingsDiscordEnableHint))
                    .keywords("discord presence rpc"),
                    toggle(
                        "discordShowWorkspace",
                        tx(Key::SettingsDiscordWorkspace),
                        settings.discord_show_workspace,
                        Target::DiscordShowWorkspace,
                    )
                    .hint(tx(Key::SettingsDiscordWorkspaceHint)),
                    toggle(
                        "discordShowAgents",
                        tx(Key::SettingsDiscordAgents),
                        settings.discord_show_agents,
                        Target::DiscordShowAgents,
                    )
                    .hint(tx(Key::SettingsDiscordAgentsHint)),
                ],
            )
            .hint(tx(Key::SettingsDiscordHint)),
            section(
                "discordApp",
                tx(Key::SettingsDiscordApp),
                vec![
                    item(
                        "discordAppId",
                        Some(tx(Key::SettingsDiscordAppId)),
                        Kind::Input {
                            value: settings.discord_app_id.clone(),
                            placeholder: t.t(Key::SettingsDiscordAppIdPlaceholder),
                            target: Target::DiscordAppId,
                        },
                    )
                    .hint(tx(Key::SettingsDiscordAppIdHint))
                    .keywords("discord application id client"),
                ],
            ),
        ],
    }
}

/// The Experimental page, which only exists while there is a feature to put
/// on it; it sits just before About.
fn experimental(ctx: &Context, experiments: &[Experiment]) -> Option<Category> {
    let Context { t, settings, .. } = *ctx;
    let tx = |key: Key| t.t(key).into_owned();
    if experiments.is_empty() {
        return None;
    }
    Some(Category {
        id: SettingsCategory::Experimental,
        label: tx(Key::SettingsCatExperimental),
        icon: Icon::Bolt,
        sections: vec![
            section(
                "experiments",
                tx(Key::ExperimentsTitle),
                experiments
                    .iter()
                    .map(|experiment| {
                        let mut parts = vec![tx(experiment.stage.label_key())];
                        if let Some(platforms) = experiment.platforms {
                            let names: Vec<&str> =
                                platforms.iter().map(|platform| platform.name()).collect();
                            parts.push(
                                t.t_with(
                                    Key::ExperimentsOnlyOn,
                                    &args!("platforms" => names.join(", ")),
                                )
                                .into_owned(),
                            );
                        }
                        parts.push(tx(experiment.hint));
                        toggle(
                            &format!("experiment.{}", experiment.id),
                            tx(experiment.title),
                            settings.experiments.iter().any(|id| id == experiment.id),
                            Target::Experiment(experiment.id.to_string()),
                        )
                        .hint(parts.join(" · "))
                        .keywords(
                            "experimental experiment experiments beta labs expérimental expérience bêta",
                        )
                    })
                    .collect(),
            )
            .hint(tx(Key::ExperimentsWarning)),
        ],
    })
}

fn about(ctx: &Context, version: &str) -> Category {
    let Context { t, .. } = *ctx;
    let tx = |key: Key| t.t(key).into_owned();
    let mut identity = vec![item(
        "appName",
        Some("Hangar.AI".to_string()),
        Kind::Text {
            value: "hangar-ai".to_string(),
        },
    )];
    // Empty until the version is known: the row stays out meanwhile.
    if !version.is_empty() {
        identity.push(item(
            "version",
            Some(tx(Key::SettingsVersion)),
            Kind::Text {
                value: version.to_string(),
            },
        ));
    }
    Category {
        id: SettingsCategory::About,
        label: tx(Key::SettingsCatAbout),
        icon: Icon::Info,
        sections: vec![
            section("about", tx(Key::SettingsCatAbout), identity),
            section(
                "maintenance",
                tx(Key::SettingsMaintenance),
                vec![
                    item(
                        "reset",
                        Some(tx(Key::SettingsReset)),
                        Kind::Action {
                            label: tx(Key::SettingsReset),
                            confirm: Some(tx(Key::SettingsResetConfirm)),
                            danger: true,
                            act: Act::ResetSettings,
                        },
                    )
                    .hint(tx(Key::SettingsResetHint)),
                ],
            ),
        ],
    }
}

#[cfg(test)]
mod tests;
