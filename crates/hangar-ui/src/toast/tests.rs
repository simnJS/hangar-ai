use super::*;

type T = Toast<&'static str>;

fn at(seconds: u64) -> Instant {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *START.get_or_init(Instant::now) + Duration::from_secs(seconds)
}

mod lifetime {
    use super::*;

    #[test]
    fn grows_with_how_much_the_toast_matters() {
        let seconds = |toast: T| toast.lifetime.unwrap().as_secs();
        assert_eq!(seconds(T::info("a")), 4);
        assert_eq!(seconds(T::success("a")), 4);
        assert_eq!(seconds(T::warning("a")), 7);
        assert_eq!(seconds(T::error("a")), 10);
    }

    #[test]
    fn a_toast_with_an_action_stays_long_enough_to_reach_it() {
        let toast = T::info("Saved").action("Undo", "undo");
        assert_eq!(toast.lifetime, Some(Duration::from_secs(10)));
        // But not shorter than an error's, if it was longer.
        let long = T::info("Saved")
            .lifetime(Duration::from_secs(30))
            .action("Undo", "undo");
        assert_eq!(long.lifetime, Some(Duration::from_secs(30)));
    }

    #[test]
    fn a_sticky_toast_never_expires() {
        let mut toasts = Toasts::new();
        toasts.push(T::error("Failed").sticky(), at(0));
        toasts.tick(at(100_000));
        assert_eq!(toasts.len(), 1);
        assert_eq!(toasts.next_deadline(), None);
    }
}

mod showing {
    use super::*;

    #[test]
    fn expires_when_its_time_has_come() {
        let mut toasts = Toasts::new();
        toasts.push(T::info("a"), at(0));
        toasts.tick(at(3));
        assert_eq!(toasts.len(), 1);
        toasts.tick(at(4));
        assert!(toasts.is_empty());
    }

    #[test]
    fn counts_each_toasts_time_from_its_own_start() {
        let mut toasts = Toasts::new();
        toasts.push(T::info("early"), at(0));
        toasts.push(T::info("late"), at(3));
        toasts.tick(at(5));
        let left: Vec<_> = toasts.iter().map(|s| s.toast.message.as_str()).collect();
        assert_eq!(left, ["late"]);
        assert_eq!(toasts.next_deadline(), Some(at(7)));
    }

    #[test]
    fn dismissing_one_leaves_the_others() {
        let mut toasts = Toasts::new();
        let first = toasts.push(T::info("a"), at(0));
        let second = toasts.push(T::info("b"), at(0));
        toasts.dismiss(first);
        let ids: Vec<_> = toasts.iter().map(|shown| shown.id).collect();
        assert_eq!(ids, [second]);
    }

    #[test]
    fn identities_are_never_reused() {
        let mut toasts = Toasts::new();
        let first = toasts.push(T::info("a"), at(0));
        toasts.dismiss(first);
        assert_ne!(toasts.push(T::info("b"), at(0)), first);
    }

    #[test]
    fn a_sixth_pushes_the_oldest_out() {
        let mut toasts = Toasts::new();
        for n in 0..=MAX_VISIBLE {
            toasts.push(T::info(format!("{n}")), at(0));
        }
        assert_eq!(toasts.len(), MAX_VISIBLE);
        assert_eq!(toasts.iter().next().unwrap().toast.message, "1");
    }

    #[test]
    fn the_clock_runs_only_while_something_will_go() {
        // `Subscription::none()` has no recipes; a running clock has one.
        use iced::advanced::subscription::into_recipes;
        let mut toasts = Toasts::new();
        assert!(into_recipes(toasts.subscription().map(|_| ())).is_empty());
        toasts.push(T::info("a"), at(0));
        assert_eq!(into_recipes(toasts.subscription().map(|_| ())).len(), 1);
    }
}
