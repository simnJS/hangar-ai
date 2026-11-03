use super::*;

mod gauge_level {
    use super::*;

    #[test]
    fn is_amber_from_70_and_red_from_90() {
        assert_eq!(gauge_level(0), GaugeLevel::Ok);
        assert_eq!(gauge_level(69), GaugeLevel::Ok);
        assert_eq!(gauge_level(70), GaugeLevel::Warn);
        assert_eq!(gauge_level(89), GaugeLevel::Warn);
        assert_eq!(gauge_level(90), GaugeLevel::Hot);
        assert_eq!(gauge_level(100), GaugeLevel::Hot);
    }
}

mod pulse {
    #[test]
    fn starts_small_and_visible() {
        let (scale, opacity) = super::super::pulse(0.0);
        assert!((scale - 0.6).abs() < 1e-6);
        assert!((opacity - 0.7).abs() < 1e-6);
    }

    #[test]
    fn grows_and_fades_until_seven_tenths_of_the_turn() {
        let (scale, opacity) = super::super::pulse(0.35);
        assert!((scale - 0.975).abs() < 1e-4);
        assert!((opacity - 0.35).abs() < 1e-4);
        let (scale, opacity) = super::super::pulse(0.6999);
        assert!(scale > 1.34 && opacity < 0.001);
    }

    #[test]
    fn waits_invisible_for_the_rest_of_the_turn() {
        assert_eq!(super::super::pulse(0.7), (1.35, 0.0));
        assert_eq!(super::super::pulse(0.99), (1.35, 0.0));
    }
}

mod animation {
    use super::*;

    #[test]
    fn only_a_working_or_starting_glyph_moves() {
        for state in [
            GlyphState::Waiting,
            GlyphState::Yours,
            GlyphState::Idle,
            GlyphState::Exited,
        ] {
            assert!(!state.animates(), "{state:?}");
        }
        assert!(GlyphState::Working.animates());
        assert!(GlyphState::Starting.animates());
    }

    #[test]
    fn a_starting_ring_is_lit_for_half_of_its_blink() {
        assert!(blink_lit(0.0));
        assert!(blink_lit(0.49));
        assert!(!blink_lit(0.5));
        assert!(!blink_lit(0.99));
    }

    #[test]
    fn the_app_states_map_one_for_one() {
        assert_eq!(GlyphState::from(AgentState::Working), GlyphState::Working);
        assert_eq!(GlyphState::from(AgentState::Waiting), GlyphState::Waiting);
        assert_eq!(GlyphState::from(AgentState::Yours), GlyphState::Yours);
        assert_eq!(GlyphState::from(AgentState::Idle), GlyphState::Idle);
    }
}

#[test]
fn a_glyph_takes_its_fill_from_the_themes_state_colours() {
    let t = hangar_theme::default_theme();
    assert_eq!(
        Tint::of(t, GlyphState::Working).fill,
        t.colors.state_working_fill
    );
    assert_eq!(
        Tint::of(t, GlyphState::Waiting).fill,
        t.colors.state_waiting_fill
    );
    assert_eq!(
        Tint::of(t, GlyphState::Exited).fill,
        t.colors.state_idle_fill
    );
}
