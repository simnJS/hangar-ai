//! `src/lib/kitTabs.test.ts`, case for case. `kitFileUrl`'s two cases are the
//! kit relay's (hangar-webview), where the URL is built; `frameTookKeyboard`'s
//! went with the frame (a page is a window of its own here).

use hangar_core::kits::KitInfo;
use hangar_i18n::Locale;
use hangar_kit_host::Palette;
use hangar_theme::{Appearance, Rgb, Theme, contrast_ratio};
use serde_json::{Value, json};

use super::*;
use crate::kit_tabs::testing::*;
use crate::kits::model::effective_kits;
use crate::state::types::{Settings, Workspace, WorkspaceKit};

fn on(kits: Vec<WorkspaceKit>) -> Vec<WorkspaceKit> {
    kits
}

fn choice(kit_id: &str, kit_name: &str, tab: Tab, permissions: &[&str]) -> KitTabChoice {
    KitTabChoice {
        kit_id: kit_id.into(),
        kit_name: kit_name.into(),
        tab,
        permissions: strings(permissions),
        commands_accepted: None,
    }
}

mod effective_permissions_ {
    use super::*;

    #[test]
    fn keeps_what_the_tab_declares_and_the_person_granted_both() {
        assert_eq!(
            effective_permissions(&["panes.read", "notify"], &["notify", "files.read"]),
            strings(&["notify"])
        );
        assert!(effective_permissions(&["panes.read"], &[] as &[&str]).is_empty());
        assert!(effective_permissions(&[] as &[&str], &["panes.read"]).is_empty());
        assert_eq!(
            effective_permissions(&["files.delete", "notify"], &["files.delete", "notify"]),
            strings(&["notify"])
        );
    }

    #[test]
    fn keeps_a_local_servers_port_as_declared_and_granted_that_very_port() {
        assert_eq!(
            effective_permissions(
                &["loopback:8765", "data.read"],
                &["loopback:8765", "data.read"]
            ),
            strings(&["data.read", "loopback:8765"])
        );
        assert!(effective_permissions(&["loopback:8765"], &["loopback:9000"]).is_empty());
        assert_eq!(
            effective_permissions(
                &["loopback:9000", "loopback:8765"],
                &["loopback:8765", "loopback:9000"]
            ),
            strings(&["loopback:8765", "loopback:9000"])
        );
        assert!(effective_permissions(&["loopback:80"], &["loopback:80"]).is_empty());
    }

    #[test]
    fn never_hands_a_tab_the_pseudo_permission_of_accepted_hooks() {
        let hooks = "hooks:0123456789abcdef";
        assert_eq!(
            effective_permissions(&[hooks, "notify"], &[hooks, "notify"]),
            strings(&["notify"])
        );
    }
}

mod workspace_kit_tabs_ {
    use super::*;

    #[test]
    fn lists_the_tabs_of_the_kits_enabled_on_the_workspace_in_their_order() {
        let installed = installed();
        let kits = on(vec![
            entry("roblox", &["files.read", "panes.send", "notify"]),
            entry("hello-kit", &["panes.read"]),
        ]);
        assert_eq!(
            workspace_kit_tabs(&kits, Some(&installed)),
            vec![
                choice(
                    "roblox",
                    "roblox name",
                    explorer(),
                    &["panes.send", "files.read"]
                ),
                choice("hello-kit", "hello-kit name", status(), &["panes.read"]),
            ]
        );
    }

    const GRANT: &str = "commands:0123456789abcdef";
    const CHANGED: &str = "commands:fedcba9876543210";

    fn ci() -> Tab {
        tab("ci", "CI", "tabs/ci.html", &["command:view", "notify"])
    }

    fn github_kit(commands_permission: &str) -> KitInfo {
        KitInfo {
            source: github(),
            commands_permission: Some(commands_permission.into()),
            ..kit("ci-kit", vec![ci()])
        }
    }

    fn held_and(more: &[&str]) -> Vec<&'static str> {
        let mut held = vec!["command:view", "notify"];
        for grant in more {
            held.push(if *grant == GRANT { GRANT } else { CHANGED });
        }
        held
    }

    #[test]
    fn holds_what_the_tab_declares_once_the_workspace_accepted_these_very_commands() {
        let kits = vec![entry("ci-kit", &held_and(&[GRANT]))];
        let [choice] = &workspace_kit_tabs(&kits, Some(&[github_kit(GRANT)]))[..] else {
            panic!("one tab")
        };
        assert_eq!(choice.permissions, strings(&["notify", "command:view"]));
        assert_eq!(choice.commands_accepted.as_deref(), Some(GRANT));
    }

    #[test]
    fn holds_none_of_them_when_they_changed_and_keeps_the_rest_of_what_it_was_granted() {
        let kits = vec![entry("ci-kit", &held_and(&[GRANT]))];
        let [choice] = &workspace_kit_tabs(&kits, Some(&[github_kit(CHANGED)]))[..] else {
            panic!("one tab")
        };
        assert_eq!(choice.permissions, strings(&["notify"]));
        assert_eq!(choice.commands_accepted, None);
    }

    #[test]
    fn holds_none_of_them_when_they_were_never_accepted() {
        let kits = vec![entry("ci-kit", &held_and(&[]))];
        let [choice] = &workspace_kit_tabs(&kits, Some(&[github_kit(GRANT)]))[..] else {
            panic!("one tab")
        };
        assert_eq!(choice.permissions, strings(&["notify"]));
    }

    #[test]
    fn still_needs_the_tabs_own_permission_once_they_are_accepted() {
        let kits = vec![entry("ci-kit", &["notify", GRANT])];
        let [choice] = &workspace_kit_tabs(&kits, Some(&[github_kit(GRANT)]))[..] else {
            panic!("one tab")
        };
        assert_eq!(choice.permissions, strings(&["notify"]));
        assert_eq!(choice.commands_accepted.as_deref(), Some(GRANT));
    }

    #[test]
    fn asks_a_linked_folder_nothing() {
        let folder_kit = KitInfo {
            commands_permission: Some(CHANGED.into()),
            ..kit("ci-kit", vec![ci()])
        };
        let kits = vec![entry("ci-kit", &held_and(&[]))];
        let [choice] = &workspace_kit_tabs(&kits, Some(&[folder_kit]))[..] else {
            panic!("one tab")
        };
        assert_eq!(choice.permissions, strings(&["notify", "command:view"]));
        assert_eq!(choice.commands_accepted, None);
    }

    fn everywhere(granted: &[&str]) -> Settings {
        Settings {
            kits_everywhere: Some(vec![entry("ci-kit", granted)]),
            ..Settings::default()
        }
    }

    #[test]
    fn reads_what_a_kit_enabled_on_all_workspaces_accepted_and_a_workspaces_own_entry_first() {
        let settings = everywhere(&held_and(&[GRANT]));
        let bare = Workspace::default();
        let kits = effective_kits(Some(&bare), Some(&settings));
        let all = workspace_kit_tabs(&kits, Some(&[github_kit(GRANT)]));
        assert_eq!(all[0].permissions, strings(&["notify", "command:view"]));
        assert_eq!(all[0].commands_accepted.as_deref(), Some(GRANT));

        let changed = workspace_kit_tabs(&kits, Some(&[github_kit(CHANGED)]));
        assert_eq!(changed[0].permissions, strings(&["notify"]));

        let own = Workspace {
            kits: Some(vec![entry("ci-kit", &["notify"])]),
            ..Workspace::default()
        };
        let kits = effective_kits(Some(&own), Some(&settings));
        let own = workspace_kit_tabs(&kits, Some(&[github_kit(GRANT)]));
        assert_eq!(own[0].permissions, strings(&["notify"]));
        assert_eq!(own[0].commands_accepted, None);
    }

    #[test]
    fn is_ready_to_open_on_any_workspace_once_it_is_enabled_on_all_of_them() {
        let settings = everywhere(&held_and(&[GRANT]));
        let installed = [github_kit(GRANT)];
        assert_eq!(
            kit_tab_state(&[], Some(&installed), "ci-kit", "ci"),
            KitTabState::Disabled
        );
        let kits = effective_kits(Some(&Workspace::default()), Some(&settings));
        match kit_tab_state(&kits, Some(&installed), "ci-kit", "ci") {
            KitTabState::Ready { choice, .. } => {
                assert_eq!(choice.permissions, strings(&["notify", "command:view"]))
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn leaves_out_kits_that_are_not_enabled_not_installed_or_not_usable() {
        let installed = installed();
        let kits = vec![
            entry("missing-kit", &[]),
            entry("broken-kit", &[]),
            entry("future-kit", &["notify"]),
        ];
        assert!(workspace_kit_tabs(&kits, Some(&installed)).is_empty());
        assert!(workspace_kit_tabs(&[], Some(&installed)).is_empty());
        assert!(workspace_kit_tabs(&[entry("hello-kit", &[])], None).is_empty());
    }

    #[test]
    fn refuses_a_kit_whose_manifest_names_another_id_than_the_one_enabled() {
        let renamed = KitInfo {
            manifest: Some(manifest("other", Vec::new())),
            ..kit("hello-kit", vec![status()])
        };
        assert!(workspace_kit_tabs(&[entry("hello-kit", &[])], Some(&[renamed])).is_empty());
    }
}

mod read_kit_tab_params_ {
    use super::*;

    fn run() -> Tab {
        let mut tab = tab("run", "Run", "run.html", &[]);
        tab.params.insert("run".into(), param(Some("[0-9]+")));
        tab.params.insert("letter".into(), param(Some(r"\p{L}+")));
        tab.params.insert("note".into(), param(None));
        tab.params.insert("either".into(), param(Some("a|b")));
        tab
    }

    fn read(tab: &Tab, raw: Value) -> Result<KitTabParams, String> {
        read_kit_tab_params(tab, Some(&raw))
    }

    fn params(pairs: &[(&str, &str)]) -> KitTabParams {
        pairs
            .iter()
            .map(|(name, value)| ((*name).into(), (*value).into()))
            .collect()
    }

    #[test]
    fn takes_nothing_as_nothing() {
        let tab = run();
        assert_eq!(read_kit_tab_params(&tab, None), Ok(KitTabParams::new()));
        assert_eq!(read(&tab, Value::Null), Ok(KitTabParams::new()));
        assert_eq!(read(&tab, json!({})), Ok(KitTabParams::new()));
    }

    #[test]
    fn anchors_a_rule_whatever_its_author_wrote() {
        let tab = run();
        assert_eq!(
            read(&tab, json!({ "run": "12" })),
            Ok(params(&[("run", "12")]))
        );
        assert!(
            read(&tab, json!({ "run": "12a" }))
                .unwrap_err()
                .contains("must match its rule")
        );
        assert_eq!(
            read(&tab, json!({ "either": "a" })),
            Ok(params(&[("either", "a")]))
        );
        // Wrapped, or `a|b` would only be anchored on one side of each.
        for value in ["ax", "xb"] {
            assert!(
                read(&tab, json!({ "either": value }))
                    .unwrap_err()
                    .contains("must match its rule")
            );
        }
    }

    #[test]
    fn reads_unicode_classes_and_takes_any_string_where_there_is_no_rule() {
        let tab = run();
        assert_eq!(
            read(&tab, json!({ "letter": "éa" })),
            Ok(params(&[("letter", "éa")]))
        );
        assert_eq!(
            read(&tab, json!({ "note": "; rm -rf /" })),
            Ok(params(&[("note", "; rm -rf /")]))
        );
        assert_eq!(
            read(&tab, json!({ "note": "" })),
            Ok(params(&[("note", "")]))
        );
    }

    #[test]
    fn does_not_let_a_rule_climb_out_of_its_wrapping() {
        let mut sly = run();
        sly.params.insert("run".into(), param(Some("a)|(?:b")));
        assert!(
            read(&sly, json!({ "run": "ab" }))
                .unwrap_err()
                .contains("cannot read the kit's rule")
        );
    }

    #[test]
    fn holds_a_value_to_its_length_before_anything_else_runs() {
        let tab = run();
        assert!(read(&tab, json!({ "note": "x".repeat(KIT_TAB_PARAM_MAX_CHARS) })).is_ok());
        assert!(
            read(
                &tab,
                json!({ "note": "x".repeat(KIT_TAB_PARAM_MAX_CHARS + 1) })
            )
            .unwrap_err()
            .contains("at most")
        );
    }

    #[test]
    fn keeps_a_name_like_proto_a_name_refused_unless_declared() {
        let tab = run();
        let raw = json!({ "__proto__": "1" });
        assert!(
            read(&tab, raw.clone())
                .unwrap_err()
                .contains("declares no param")
        );
        let mut declared = run();
        declared.params.clear();
        declared.params.insert("__proto__".into(), param(None));
        let read = read(&declared, raw).unwrap();
        assert_eq!(read.keys().collect::<Vec<_>>(), vec!["__proto__"]);
    }

    #[test]
    fn says_which_names_it_takes_and_refuses_what_is_not_an_object_of_strings() {
        let tab = run();
        assert_eq!(
            read(&tab, json!({ "other": "1" })).unwrap_err(),
            "The tab \"run\" declares no param \"other\". It takes: either, letter, note, run."
        );
        assert_eq!(
            read(&tab, json!({ "run": 12 })).unwrap_err(),
            "Param \"run\" must be a string."
        );
        assert!(read(&tab, json!(["run"])).is_err());
        assert!(read(&tab, json!("run=1")).is_err());
    }
}

mod kit_tab_state_ {
    use super::*;

    fn kits() -> Vec<WorkspaceKit> {
        vec![
            entry("hello-kit", &["panes.read", "notify"]),
            entry("broken-kit", &[]),
        ]
    }

    #[test]
    fn is_ready_with_the_pages_url_and_what_it_may_call() {
        let installed = installed();
        assert_eq!(
            kit_tab_state(&kits(), Some(&installed), "hello-kit", "status"),
            KitTabState::Ready {
                url: Some(kit_file_url("hello-kit", "tabs/status/index.html")),
                choice: Box::new(choice(
                    "hello-kit",
                    "hello-kit name",
                    status(),
                    &["panes.read", "notify"]
                )),
            }
        );
    }

    #[test]
    fn a_native_tab_has_no_page_to_serve() {
        let native = tab("n", "N", "tabs/n.wasm", &[]);
        let installed = [kit("hello-kit", vec![native])];
        match kit_tab_state(&kits(), Some(&installed), "hello-kit", "n") {
            KitTabState::Ready { url, .. } => assert_eq!(url, None),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn says_why_a_tab_cannot_run() {
        let installed = installed();
        let kits = kits();
        assert_eq!(
            kit_tab_state(&kits, Some(&installed), "roblox", "explorer"),
            KitTabState::Disabled
        );
        assert_eq!(
            kit_tab_state(&kits, None, "hello-kit", "status"),
            KitTabState::Loading
        );
        assert_eq!(
            kit_tab_state(&kits, Some(&[]), "hello-kit", "status"),
            KitTabState::Missing
        );
        assert_eq!(
            kit_tab_state(&kits, Some(&installed), "hello-kit", "gone"),
            KitTabState::Missing
        );
        assert_eq!(
            kit_tab_state(&kits, Some(&installed), "broken-kit", "status"),
            KitTabState::Broken("its manifest is invalid".into())
        );
        assert_eq!(
            kit_tab_state(&[], Some(&installed), "hello-kit", "status"),
            KitTabState::Disabled
        );
    }
}

mod the_theme_a_tab_is_told {
    use super::*;

    const MONO: &str = "\"Cascadia Mono\", monospace";

    fn event(theme: &Theme, scale: f64) -> Value {
        kit_theme_event(theme, scale, MONO, Locale::Fr)
    }

    /// Every opaque surface text is written on.
    fn surfaces(p: &Palette) -> Vec<iced::Color> {
        vec![p.bg, p.surface, p.raised, p.float, p.field, p.well]
    }

    fn text(p: &Palette) -> Vec<(&'static str, iced::Color)> {
        vec![
            ("text", p.text),
            ("text-dim", p.text_dim),
            ("text-muted", p.text_muted),
            ("text-faint", p.text_faint),
            ("accent-text", p.accent_text),
            ("ok", p.ok),
            ("warn", p.warn),
            ("danger", p.danger),
            ("neutral", p.neutral),
        ]
    }

    fn shapes(p: &Palette) -> Vec<(&'static str, iced::Color)> {
        vec![
            ("ok-fill", p.ok_fill),
            ("warn-fill", p.warn_fill),
            ("neutral-fill", p.neutral_fill),
            ("danger", p.danger),
            ("focus", p.focus),
        ]
    }

    /// What is below the ratio, by the pair: an empty list reads best in a
    /// failure.
    fn below(names: &[(&'static str, iced::Color)], on: &[iced::Color], ratio: f64) -> Vec<String> {
        names
            .iter()
            .flat_map(|(name, colour)| {
                on.iter()
                    .filter(|surface| {
                        contrast_ratio(Rgb::from(*colour), Rgb::from(**surface)) < ratio - 0.005
                    })
                    .map(move |surface| format!("{name} {colour:?} on {surface:?}"))
            })
            .collect()
    }

    #[test]
    fn on_every_theme_it_is_the_protocols_event_in_the_themes_own_scheme_and_hangars_language() {
        for theme in hangar_theme::themes() {
            let sent = event(theme, 1.0);
            assert_eq!(sent["hangar"], 1);
            assert_eq!(sent["event"], "theme");
            assert_eq!(sent["data"]["scheme"], theme.appearance.as_str());
            assert_eq!(sent["data"]["locale"], "fr");
        }
    }

    #[test]
    fn on_every_theme_it_sends_every_token_of_the_map_from_the_chrome_token_it_names() {
        for theme in hangar_theme::themes() {
            let sent = event(theme, 1.0);
            let tokens = sent["data"]["tokens"].as_object().unwrap();
            let map = hangar_theme::kit_tokens(theme);
            for (token, value) in &map {
                assert_eq!(tokens[token], json!(value), "{token} on {}", theme.id);
                assert!(!value.is_empty());
            }
            let mut names: Vec<&str> = tokens.keys().map(String::as_str).collect();
            let mut expected: Vec<&str> = map.iter().map(|(name, _)| name.as_str()).collect();
            expected.extend(["--kit-scale", "--kit-mono"]);
            names.sort_unstable();
            expected.sort_unstable();
            assert_eq!(names, expected);
        }
    }

    #[test]
    fn on_every_theme_it_keeps_text_and_shapes_and_the_primary_label_to_their_contrast() {
        use hangar_theme::{FILL_CONTRAST, TEXT_CONTRAST};
        for theme in hangar_theme::themes() {
            let p = Palette::of(theme);
            assert_eq!(
                below(&text(&p), &surfaces(&p), TEXT_CONTRAST),
                Vec::<String>::new(),
                "{}",
                theme.id
            );
            assert_eq!(
                below(&shapes(&p), &surfaces(&p), FILL_CONTRAST),
                Vec::<String>::new(),
                "{}",
                theme.id
            );
            assert!(
                contrast_ratio(Rgb::from(p.on_accent), Rgb::from(p.accent))
                    >= TEXT_CONTRAST - 0.005,
                "{}",
                theme.id
            );
        }
    }

    #[test]
    fn takes_any_theme_and_what_its_palette_gets_wrong_is_corrected_not_sent_as_it_is() {
        use hangar_theme::{FILL_CONTRAST, TEXT_CONTRAST, derive_colors};
        let base = hangar_theme::by_id_or_default("graphite");
        let custom = |appearance: Appearance, background: u32, foreground: u32| {
            let rgb = |packed: u32| {
                iced::Color::from_rgb8((packed >> 16) as u8, (packed >> 8) as u8, packed as u8)
            };
            let mut terminal = base.terminal;
            terminal.background = rgb(background);
            terminal.foreground = rgb(foreground);
            for slot in [1, 2, 3] {
                terminal.ansi[slot] = rgb(background);
            }
            let accent = rgb(0xff00aa);
            Theme {
                id: "custom",
                name: "Not bundled",
                appearance,
                terminal,
                colors: derive_colors(appearance, accent, &terminal),
            }
        };
        for theme in [
            custom(Appearance::Dark, 0x101014, 0x1c1c20),
            custom(Appearance::Light, 0xfafafa, 0xeeeeee),
        ] {
            let p = Palette::of(&theme);
            assert_ne!(p.text, theme.terminal.foreground);
            assert_ne!(p.danger, theme.terminal.ansi[1]);
            assert!(below(&text(&p), &surfaces(&p), TEXT_CONTRAST).is_empty());
            assert!(below(&shapes(&p), &surfaces(&p), FILL_CONTRAST).is_empty());
        }
    }

    #[test]
    fn carries_the_interface_size_and_the_terminals_font() {
        let sent = event(&hangar_theme::themes()[0], 1.1);
        assert_eq!(sent["data"]["tokens"]["--kit-scale"], "1.1");
        assert_eq!(sent["data"]["tokens"]["--kit-mono"], MONO);
        let sent = event(&hangar_theme::themes()[0], 1.0);
        assert_eq!(sent["data"]["tokens"]["--kit-scale"], "1");
    }

    #[test]
    fn gives_the_interface_size_as_the_window_applies_it() {
        assert_eq!(kit_ui_scale(100.0), 1.0);
        assert_eq!(kit_ui_scale(110.0), 1.1);
        assert_eq!(kit_ui_scale(10.0), 0.9);
        assert_eq!(kit_ui_scale(900.0), 1.3);
        assert_eq!(kit_ui_scale(0.0), 1.0);
    }

    #[test]
    fn is_sent_at_the_load_of_the_page_then_at_each_change_and_never_before_the_load() {
        let themes = hangar_theme::themes();
        let dark_theme = themes.iter().find(|t| t.is_dark()).unwrap();
        let light_theme = themes.iter().find(|t| !t.is_dark()).unwrap();
        let dark = event(dark_theme, 1.0);
        let light = event(light_theme, 1.0);
        let other = kit_theme_event(dark_theme, 1.0, MONO, Locale::En);
        let mut sender = ThemeSender::default();
        let mut posted = Vec::new();

        posted.extend(sender.set(dark));
        posted.extend(sender.set(light.clone()));
        assert!(posted.is_empty());

        posted.extend(sender.load());
        assert_eq!(posted, vec![light.clone()]);

        posted.extend(sender.set(other.clone()));
        assert_eq!(posted, vec![light, other]);
    }

    #[test]
    fn asks_nothing_when_nothing_was_set() {
        let mut sender = ThemeSender::default();
        assert_eq!(sender.load(), None);
    }
}

mod kit_tab_loads_ {
    use super::*;

    #[test]
    fn keeps_a_restored_tab_unloaded_while_it_is_hidden_and_loads_it_once_shown() {
        assert!(!kit_tab_loads(true, false));
        assert!(kit_tab_loads(true, true));
    }

    #[test]
    fn loads_a_tab_no_restart_brought_back_at_once_shown_or_not() {
        assert!(kit_tab_loads(false, false));
        assert!(kit_tab_loads(false, true));
    }
}

mod for_agents {
    use super::*;

    fn ws(kits: Vec<WorkspaceKit>) -> Workspace {
        Workspace {
            name: "Game".into(),
            kits: Some(kits),
            ..Workspace::default()
        }
    }

    #[test]
    fn says_why_a_tab_may_not_be_opened_in_words_to_act_on() {
        let installed = installed();
        let settings = Settings::default();
        let refusal = |ws: &Workspace, kit: &str, tab: &str| {
            kit_tab_to_open(ws, &settings, &installed, kit, tab).unwrap_err()
        };
        assert_eq!(
            refusal(&ws(Vec::new()), "hello-kit", "status"),
            "The kit \"hello-kit\" is not enabled on \"Game\". No kit is enabled there. Only the person can enable one."
        );
        let some = ws(vec![entry("roblox", &[])]);
        assert_eq!(
            refusal(&some, "hello-kit", "status"),
            "The kit \"hello-kit\" is not enabled on \"Game\". Enabled there: roblox. Only the person can enable one."
        );
        let gone = ws(vec![entry("gone-kit", &[])]);
        assert_eq!(
            refusal(&gone, "gone-kit", "x"),
            "The kit \"gone-kit\" is enabled on \"Game\", and no longer installed."
        );
        let broken = ws(vec![entry("broken-kit", &[])]);
        assert_eq!(
            refusal(&broken, "broken-kit", "x"),
            "The kit \"broken-kit\" cannot be used right now: its manifest is invalid."
        );
        let review = ws(vec![entry("hello-kit", &["panes.read"])]);
        assert!(refusal(&review, "hello-kit", "status").contains("has changes to review"));
        let ready = ws(vec![entry("hello-kit", &["panes.read", "notify"])]);
        assert_eq!(
            refusal(&ready, "hello-kit", "gone"),
            "The kit \"hello-kit\" has no tab \"gone\". Its tabs: status."
        );
        let (kit, tab) = kit_tab_to_open(&ready, &settings, &installed, "hello-kit", "status")
            .expect("it opens");
        assert_eq!((kit.id.as_str(), tab.id.as_str()), ("hello-kit", "status"));
    }

    #[test]
    fn lists_the_tabs_with_what_they_take_and_what_waits_for_review() {
        let installed = installed();
        let rows = kit_tab_listing(
            &ws(vec![
                entry("hello-kit", &["panes.read", "notify"]),
                entry("roblox", &["files.read"]),
            ]),
            &Settings::default(),
            &installed,
        );
        assert_eq!(
            rows[0],
            json!({
                "kit": "hello-kit", "kitName": "hello-kit name", "tab": "status",
                "title": "Status", "description": "", "singleton": false, "params": {},
            })
        );
        assert_eq!(rows[1]["kit"], "roblox");
        assert_eq!(rows[1]["toReview"], true);
    }
}
