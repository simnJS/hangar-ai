use super::*;

fn field() -> NumberField {
    NumberField::new(10.0, 1.0, 20.0, 1.0)
}

fn typed(field: &mut NumberField, text: &str) -> Option<f64> {
    field.update(NumberMsg::Input(text.to_string()))
}

mod number_field {
    use super::*;

    #[test]
    fn starts_on_its_value_written_out() {
        let field = NumberField::new(10.0, 1.0, 20.0, 1.0);
        assert_eq!(field.value(), 10.0);
        assert_eq!(field.text(), "10");
        assert!(field.is_valid());
    }

    #[test]
    fn clamps_a_value_it_is_given_outside_its_bounds() {
        assert_eq!(NumberField::new(99.0, 1.0, 20.0, 1.0).value(), 20.0);
        assert_eq!(NumberField::new(-5.0, 1.0, 20.0, 1.0).value(), 1.0);
    }

    #[test]
    fn follows_what_is_typed_while_it_reads_as_a_number_in_bounds() {
        let mut field = field();
        assert_eq!(typed(&mut field, "15"), Some(15.0));
        assert_eq!(field.value(), 15.0);
        assert_eq!(field.text(), "15");
    }

    #[test]
    fn keeps_the_last_good_value_while_the_text_is_on_its_way() {
        let mut field = field();
        // Emptied to type something else: no number, so nothing to report.
        assert_eq!(typed(&mut field, ""), None);
        assert_eq!(field.value(), 10.0);
        assert!(!field.is_valid());
        assert_eq!(typed(&mut field, "7"), Some(7.0));
        assert!(field.is_valid());
    }

    #[test]
    fn flags_a_number_outside_the_bounds_without_taking_it() {
        let mut field = field();
        assert_eq!(typed(&mut field, "50"), None);
        assert_eq!(field.value(), 10.0);
        assert_eq!(field.text(), "50");
        assert!(!field.is_valid());
    }

    #[test]
    fn refuses_characters_that_could_never_make_a_number() {
        let mut field = field();
        assert_eq!(typed(&mut field, "1x"), None);
        assert_eq!(field.text(), "10");
        // A whole-number field takes no decimal point; a minus needs a floor
        // below zero.
        typed(&mut field, "1.");
        assert_eq!(field.text(), "10");
        typed(&mut field, "-1");
        assert_eq!(field.text(), "10");
    }

    #[test]
    fn takes_fractions_and_a_sign_when_the_bounds_allow_them() {
        let mut field = NumberField::new(0.5, -1.0, 1.0, 0.1).decimals(2);
        assert_eq!(field.text(), "0.50");
        typed(&mut field, "-0.");
        assert_eq!(field.text(), "-0.");
        assert_eq!(typed(&mut field, "-0.25"), Some(-0.25));
        // Two points is no number.
        typed(&mut field, "0.2.");
        assert_eq!(field.text(), "-0.25");
    }

    #[test]
    fn steps_inside_its_bounds() {
        let mut field = NumberField::new(19.0, 1.0, 20.0, 1.0);
        assert_eq!(field.update(NumberMsg::Step(1)), Some(20.0));
        assert_eq!(field.update(NumberMsg::Step(1)), None);
        assert_eq!(field.text(), "20");
        assert_eq!(field.update(NumberMsg::Step(-1)), Some(19.0));
    }

    #[test]
    fn settles_the_text_on_the_value_when_committed() {
        let mut field = field();
        typed(&mut field, "99");
        assert!(!field.is_valid());
        assert_eq!(field.update(NumberMsg::Commit), None);
        assert_eq!(field.text(), "10");
        assert!(field.is_valid());
    }

    #[test]
    fn can_be_set_from_outside() {
        let mut field = field();
        field.set(3.0);
        assert_eq!((field.value(), field.text()), (3.0, "3"));
        field.set(300.0);
        assert_eq!(field.value(), 20.0);
    }
}
