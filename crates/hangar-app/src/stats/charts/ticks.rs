//! Round steps for an axis.

/// Round steps for an axis: 1, 2, 2.5 or 5 times a power of ten, so the ticks
/// read as numbers someone would have chosen. The last tick is at or above
/// `max`; with nothing to draw the axis is just its zero.
pub fn nice_ticks(max: f64, count: usize) -> Vec<f64> {
    if max.is_nan() || max <= 0.0 {
        return vec![0.0];
    }
    let rough = max / count.max(1) as f64;
    let magnitude = 10f64.powf(rough.log10().floor());
    let residual = rough / magnitude;
    let nice = if residual <= 1.0 {
        1.0
    } else if residual <= 2.0 {
        2.0
    } else if residual <= 2.5 {
        2.5
    } else if residual <= 5.0 {
        5.0
    } else {
        10.0
    };
    let step = nice * magnitude;
    let top = (max / step).ceil() * step;
    // Counted rather than accumulated: adding 0.1 ten times is not 1.
    let steps = (top / step).round() as usize;
    (0..=steps).map(|at| at as f64 * step).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_to_draw_is_a_zero_axis() {
        assert_eq!(nice_ticks(0.0, 4), [0.0]);
        assert_eq!(nice_ticks(-3.0, 4), [0.0]);
        assert_eq!(nice_ticks(f64::NAN, 4), [0.0]);
    }

    #[test]
    fn steps_are_one_two_two_and_a_half_or_five_times_a_power_of_ten() {
        assert_eq!(nice_ticks(100.0, 4), [0.0, 25.0, 50.0, 75.0, 100.0]);
        assert_eq!(nice_ticks(1000.0, 4), [0.0, 250.0, 500.0, 750.0, 1000.0]);
        assert_eq!(nice_ticks(40.0, 4), [0.0, 10.0, 20.0, 30.0, 40.0]);
        assert_eq!(nice_ticks(8.0, 4), [0.0, 2.0, 4.0, 6.0, 8.0]);
        assert_eq!(nice_ticks(3.0, 4), [0.0, 1.0, 2.0, 3.0]);
        assert_eq!(nice_ticks(9.0, 4), [0.0, 2.5, 5.0, 7.5, 10.0]);
        assert_eq!(nice_ticks(18.0, 4), [0.0, 5.0, 10.0, 15.0, 20.0]);
    }

    #[test]
    fn the_top_tick_reaches_the_maximum() {
        for max in [0.3, 1.0, 7.7, 99.0, 1234.0, 5.5e9] {
            let ticks = nice_ticks(max, 4);
            assert!(*ticks.last().unwrap() >= max, "{max}: {ticks:?}");
            assert!(ticks.len() <= 7, "{max}: {ticks:?}");
            assert_eq!(ticks[0], 0.0);
        }
    }

    #[test]
    fn fractions_stay_on_a_step() {
        let ticks = nice_ticks(0.4, 4);
        assert_eq!(ticks.len(), 5);
        for (at, tick) in ticks.iter().enumerate() {
            assert!((tick - at as f64 * 0.1).abs() < 1e-12, "{ticks:?}");
        }
    }
}
