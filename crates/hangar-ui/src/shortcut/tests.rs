//! The tests of `src/lib/keys.test.ts`, and the recorder's.

use super::*;

fn chord(key: &str) -> Chord {
    Chord::new(key)
}

/// What `Mod` stands for on the platform the tests run on.
fn modifier() -> Chord {
    match Platform::current() {
        Platform::Mac => chord("K").meta(),
        Platform::Other => chord("K").ctrl(),
    }
}

/// A key press as iced reports it: the key without modifiers, the physical
/// key, the modifiers held and the text produced.
fn press<'a>(
    key: &'a Key,
    code: Code,
    modifiers: Modifiers,
    text: Option<&'a str>,
) -> KeyPress<'a> {
    KeyPress {
        key,
        physical: Physical::Code(code),
        modifiers,
        text,
    }
}

fn character(c: &str) -> Key {
    Key::Character(c.into())
}

mod canonical_key {
    use super::*;

    #[test]
    fn folds_every_spelling_onto_one_name() {
        assert_eq!(canonical_key("esc").as_deref(), Some("Escape"));
        assert_eq!(canonical_key("Return").as_deref(), Some("Enter"));
        assert_eq!(canonical_key("ArrowUp").as_deref(), Some("Up"));
        assert_eq!(canonical_key("pgdn").as_deref(), Some("PageDown"));
        assert_eq!(canonical_key("comma").as_deref(), Some(","));
    }

    #[test]
    fn upper_cases_letters_and_function_keys() {
        assert_eq!(canonical_key("a").as_deref(), Some("A"));
        assert_eq!(canonical_key("f5").as_deref(), Some("F5"));
        assert_eq!(canonical_key("F12").as_deref(), Some("F12"));
    }

    #[test]
    fn is_none_for_nothing_at_all() {
        assert_eq!(canonical_key("   "), None);
    }
}

mod parse_chord {
    use super::*;

    #[test]
    fn reads_modifiers_in_any_order_and_case() {
        let expected = chord("Enter").ctrl().shift();
        assert_eq!(parse_chord("Ctrl+Shift+Enter"), Some(expected.clone()));
        assert_eq!(parse_chord("shift+ctrl+enter"), Some(expected));
    }

    #[test]
    fn accepts_the_platform_names_of_the_modifiers() {
        assert_eq!(
            parse_chord("Cmd+Option+X"),
            Some(chord("X").meta().alt())
        );
        assert_eq!(
            parse_chord("Control+Win+Q"),
            Some(chord("Q").ctrl().meta())
        );
    }

    #[test]
    fn reads_mod_as_the_platform_modifier() {
        assert_eq!(parse_chord("Mod+K"), Some(modifier()));
        assert_eq!(
            parse_chord_on(Platform::Mac, "Mod+K"),
            Some(chord("K").meta())
        );
        assert_eq!(
            parse_chord_on(Platform::Other, "Mod+K"),
            Some(chord("K").ctrl())
        );
    }

    #[test]
    fn takes_a_plus_sign_as_the_key_itself() {
        assert_eq!(parse_chord("Ctrl++"), Some(chord("+").ctrl()));
        assert_eq!(parse_chord("Ctrl+="), Some(chord("=").ctrl()));
    }

    #[test]
    fn refuses_a_lone_modifier() {
        assert_eq!(parse_chord("Ctrl"), None);
        assert_eq!(parse_chord("Ctrl+Shift"), None);
        assert_eq!(parse_chord("Mod"), None);
    }

    #[test]
    fn refuses_an_unknown_word_where_a_modifier_belongs() {
        assert_eq!(parse_chord("Hyper+A"), None);
    }

    #[test]
    fn refuses_an_empty_string() {
        assert_eq!(parse_chord(""), None);
    }
}

mod parse_binding {
    use super::*;

    #[test]
    fn reads_a_two_step_sequence() {
        assert_eq!(
            parse_binding("Ctrl+K Ctrl+S"),
            Some(vec![chord("K").ctrl(), chord("S").ctrl()])
        );
    }

    #[test]
    fn is_none_as_soon_as_one_step_does_not_parse() {
        assert_eq!(parse_binding("Ctrl+K Hyper+S"), None);
        assert_eq!(parse_binding("  "), None);
    }
}

mod serialize_binding {
    use super::*;

    #[test]
    fn gives_two_spellings_of_the_same_keys_the_same_form() {
        assert_eq!(
            serialize_binding("Shift+Ctrl+a").as_deref(),
            Some("ctrl+shift+A")
        );
        assert_eq!(
            serialize_binding("ctrl+shift+A").as_deref(),
            Some("ctrl+shift+A")
        );
        assert_eq!(
            serialize_binding("Meta+Shift+Alt+Ctrl+Up").as_deref(),
            Some("ctrl+alt+shift+meta+Up")
        );
    }

    #[test]
    fn keeps_the_steps_of_a_sequence_apart() {
        assert_eq!(
            serialize_binding("Ctrl+K  Ctrl+S").as_deref(),
            Some("ctrl+K ctrl+S")
        );
    }

    #[test]
    fn is_none_for_what_does_not_parse() {
        assert_eq!(serialize_binding("Hyper+A"), None);
    }
}

mod writing_a_binding_back {
    use super::*;

    #[test]
    fn round_trips_through_the_parser() {
        let chords = vec![chord("K").ctrl().alt(), chord("Enter").shift()];
        assert_eq!(write_binding(&chords), "Ctrl+Alt+K Shift+Enter");
        assert_eq!(parse_binding(&write_binding(&chords)), Some(chords));
    }

    #[test]
    fn formats_for_the_eye_leaving_what_does_not_parse_as_it_is() {
        assert_eq!(format_binding("Hyper+A"), "Hyper+A");
        assert_eq!(
            format_binding_on(Platform::Other, "ctrl+k ctrl+s"),
            "Ctrl+K Ctrl+S"
        );
        assert_eq!(format_binding_on(Platform::Other, "Alt+Up"), "Alt+↑");
    }

    #[test]
    fn writes_a_mac_the_way_a_mac_reads() {
        assert_eq!(
            format_binding_on(Platform::Mac, "Ctrl+Alt+Shift+Meta+K"),
            "⌃⌥⇧⌘K"
        );
        assert_eq!(
            chord_segments_on(Platform::Mac, &chord("Up").shift().meta()),
            ["⇧", "⌘", "↑"]
        );
    }

    #[test]
    fn writes_the_others_with_plus_signs() {
        assert_eq!(
            chord_segments_on(Platform::Other, &chord("K").ctrl().alt().shift().meta()),
            ["Ctrl", "Alt", "Shift", "Win", "K"]
        );
    }
}

mod key_from_press {
    use super::*;

    #[test]
    fn upper_cases_letters() {
        let key = character("a");
        assert_eq!(
            key_from_press(&press(&key, Code::KeyA, Modifiers::empty(), None)).as_deref(),
            Some("A")
        );
    }

    #[test]
    fn keeps_the_printed_letter_on_a_layout_that_moves_it() {
        // AZERTY: the key printed "A" sits where QWERTY has Q.
        let key = character("a");
        assert_eq!(
            key_from_press(&press(&key, Code::KeyQ, Modifiers::empty(), None)).as_deref(),
            Some("A")
        );
    }

    #[test]
    fn falls_back_to_the_digit_when_the_digit_row_needs_shift() {
        // AZERTY: the "1" key types "&" without Shift.
        let key = character("&");
        assert_eq!(
            key_from_press(&press(&key, Code::Digit1, Modifiers::empty(), None)).as_deref(),
            Some("1")
        );
    }

    #[test]
    fn falls_back_to_the_code_on_a_non_latin_layout() {
        let key = character("ф");
        assert_eq!(
            key_from_press(&press(&key, Code::KeyA, Modifiers::empty(), None)).as_deref(),
            Some("A")
        );
    }

    #[test]
    fn names_the_numpad_symbols_and_the_arrows() {
        let plus = character("+");
        assert_eq!(
            key_from_press(&press(&plus, Code::NumpadAdd, Modifiers::empty(), None)).as_deref(),
            Some("+")
        );
        let enter = Key::Named(Named::Enter);
        assert_eq!(
            key_from_press(&press(&enter, Code::NumpadEnter, Modifiers::empty(), None)).as_deref(),
            Some("Enter")
        );
        let left = Key::Named(Named::ArrowLeft);
        assert_eq!(
            key_from_press(&press(&left, Code::ArrowLeft, Modifiers::empty(), None)).as_deref(),
            Some("Left")
        );
        let space = character(" ");
        assert_eq!(
            key_from_press(&press(&space, Code::Space, Modifiers::empty(), None)).as_deref(),
            Some("Space")
        );
        let named_space = Key::Named(Named::Space);
        assert_eq!(
            key_from_press(&press(&named_space, Code::Space, Modifiers::empty(), None)).as_deref(),
            Some("Space")
        );
    }

    #[test]
    fn names_the_function_keys_and_the_page_keys() {
        let f6 = Key::Named(Named::F6);
        assert_eq!(
            key_from_press(&press(&f6, Code::F6, Modifiers::empty(), None)).as_deref(),
            Some("F6")
        );
        let page_down = Key::Named(Named::PageDown);
        assert_eq!(
            key_from_press(&press(&page_down, Code::PageDown, Modifiers::empty(), None))
                .as_deref(),
            Some("PageDown")
        );
    }

    #[test]
    fn is_none_for_a_modifier_on_its_own() {
        for named in [Named::Shift, Named::Control, Named::Alt, Named::Super] {
            let key = Key::Named(named);
            assert_eq!(
                key_from_press(&press(&key, Code::ShiftLeft, Modifiers::empty(), None)),
                None,
                "{named:?}"
            );
        }
    }
}

mod chord_from_press {
    use super::*;

    #[test]
    fn carries_the_modifiers_held() {
        let key = character("k");
        let modifiers = Modifiers::CTRL | Modifiers::SHIFT;
        assert_eq!(
            chord_from_press(&press(&key, Code::KeyK, modifiers, None)),
            Some(chord("K").ctrl().shift())
        );
    }

    #[test]
    fn ignores_a_character_composed_with_altgr() {
        // Windows reports AltGr as Ctrl+Alt; on AZERTY that is how "@" is typed.
        let modifiers = Modifiers::CTRL | Modifiers::ALT;
        let at = character("@");
        assert_eq!(
            chord_from_press(&press(&at, Code::Digit0, modifiers, Some("@"))),
            None
        );
        let zero = character("0");
        assert_eq!(
            chord_from_press(&press(&zero, Code::Digit0, modifiers, None)),
            Some(chord("0").ctrl().alt())
        );
    }
}

mod chord_classes {
    use super::*;

    #[test]
    fn calls_a_chord_bare_when_nothing_keeps_it_out_of_typing() {
        assert!(is_bare_chord(&chord("A").shift()));
        assert!(!is_bare_chord(&chord("A").alt()));
    }

    #[test]
    fn knows_the_function_keys_which_type_nothing_in_a_field() {
        assert!(is_function_key("F6"));
        assert!(is_function_key("F12"));
        assert!(is_function_key("F24"));
        assert!(!is_function_key("F25"));
        assert!(!is_function_key("F"));
        assert!(!is_function_key("F06"));
        assert!(!is_function_key("Home"));
    }

    #[test]
    fn recognises_the_editing_chords_a_text_field_owns() {
        let owned = |key: &str| {
            let mut c = chord(key);
            match Platform::current() {
                Platform::Mac => c.meta = true,
                Platform::Other => c.ctrl = true,
            }
            c
        };
        assert!(is_text_editing_chord(&owned("C")));
        assert!(is_text_editing_chord(&owned("V")));
        assert!(!is_text_editing_chord(&owned("K")));
        assert!(!is_text_editing_chord(&owned("C").alt()));
        assert!(!is_text_editing_chord(&chord("C")));
    }
}

mod recorder {
    use super::*;

    fn press_of(chord: Chord) -> Input {
        let held = Held {
            ctrl: chord.ctrl,
            shift: chord.shift,
            alt: chord.alt,
            meta: chord.meta,
        };
        Input::Press {
            chord: Some(chord),
            held,
        }
    }

    #[test]
    fn records_a_chord_and_confirms_it_with_enter() {
        let mut recorder = Recorder::new();
        assert_eq!(
            recorder.input(press_of(chord("K").ctrl())),
            Outcome::Pending
        );
        assert_eq!(recorder.binding().as_deref(), Some("Ctrl+K"));
        assert_eq!(
            recorder.input(press_of(chord("Enter"))),
            Outcome::Done(vec![chord("K").ctrl()])
        );
    }

    #[test]
    fn cancels_on_escape_on_its_own() {
        let mut recorder = Recorder::new();
        recorder.input(press_of(chord("K").ctrl()));
        assert_eq!(recorder.input(press_of(chord("Escape"))), Outcome::Cancelled);
    }

    #[test]
    fn enter_with_nothing_recorded_is_not_a_confirmation() {
        let mut recorder = Recorder::new();
        assert_eq!(recorder.input(press_of(chord("Enter"))), Outcome::Pending);
        assert_eq!(recorder.binding(), None);
        assert_eq!(recorder.confirm(), None);
    }

    #[test]
    fn enter_and_escape_with_a_modifier_are_ordinary_keys() {
        let mut recorder = Recorder::new();
        recorder.input(press_of(chord("Enter").ctrl().shift()));
        recorder.input(press_of(chord("Escape").alt()));
        assert_eq!(recorder.binding().as_deref(), Some("Ctrl+Shift+Enter Alt+Escape"));
    }

    #[test]
    fn shift_enter_is_recorded_as_a_key_too() {
        // Only the bare keys steer the recorder; Shift is a modifier that does
        // not keep Enter out of typing, but it does make it a new chord.
        let mut recorder = Recorder::new();
        recorder.input(press_of(chord("Enter").shift()));
        assert_eq!(recorder.binding().as_deref(), Some("Shift+Enter"));
    }

    #[test]
    fn builds_a_sequence_and_starts_over_at_a_third_chord() {
        let mut recorder = Recorder::new();
        recorder.input(press_of(chord("K").ctrl()));
        recorder.input(press_of(chord("S").ctrl()));
        assert_eq!(recorder.binding().as_deref(), Some("Ctrl+K Ctrl+S"));
        recorder.input(press_of(chord("P").ctrl()));
        assert_eq!(recorder.binding().as_deref(), Some("Ctrl+P"));
    }

    #[test]
    fn shows_the_modifiers_while_they_are_held_and_drops_them_after() {
        let mut recorder = Recorder::new();
        let held = Held {
            ctrl: true,
            alt: true,
            ..Held::default()
        };
        recorder.input(Input::Press { chord: None, held });
        assert_eq!(recorder.held(), Some(held));
        assert_eq!(held.segments_on(Platform::Other), ["Ctrl", "Alt"]);
        recorder.input(Input::Release);
        assert_eq!(recorder.held(), None);
        recorder.input(Input::Modifiers(held));
        recorder.input(press_of(chord("K").ctrl().alt()));
        assert_eq!(recorder.held(), None);
        recorder.input(Input::Modifiers(Held::default()));
        assert_eq!(recorder.held(), None);
    }

    #[test]
    fn reads_iced_events() {
        let event = keyboard::Event::KeyPressed {
            key: Key::Character("k".into()),
            modified_key: Key::Character("k".into()),
            physical_key: Physical::Code(Code::KeyK),
            location: keyboard::Location::Standard,
            modifiers: Modifiers::CTRL,
            text: None,
            repeat: false,
        };
        let mut recorder = Recorder::new();
        let input = input_from_event(&event).unwrap();
        assert_eq!(recorder.input(input), Outcome::Pending);
        assert_eq!(recorder.binding().as_deref(), Some("Ctrl+K"));

        let modifier_only = keyboard::Event::KeyPressed {
            key: Key::Named(Named::Shift),
            modified_key: Key::Named(Named::Shift),
            physical_key: Physical::Code(Code::ShiftLeft),
            location: keyboard::Location::Left,
            modifiers: Modifiers::SHIFT,
            text: None,
            repeat: false,
        };
        let input = input_from_event(&modifier_only).unwrap();
        assert!(matches!(input, Input::Press { chord: None, .. }));
    }
}
