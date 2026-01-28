//! Easing functions for animations

/// Ease-in-out cubic function
#[inline]
pub fn ease_in_out(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// Ease-out cubic function
#[inline]
pub fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// Ease-in cubic function
#[inline]
#[allow(dead_code)]
pub fn ease_in_cubic(t: f32) -> f32 {
    t * t * t
}

/// Linear interpolation (no easing)
#[inline]
#[allow(dead_code)]
pub fn linear(t: f32) -> f32 {
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ease_in_out() {
        // Start at 0
        assert!((ease_in_out(0.0) - 0.0).abs() < 0.001);
        // End at 1
        assert!((ease_in_out(1.0) - 1.0).abs() < 0.001);
        // Midpoint at 0.5
        assert!((ease_in_out(0.5) - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_ease_out_cubic() {
        assert!((ease_out_cubic(0.0) - 0.0).abs() < 0.001);
        assert!((ease_out_cubic(1.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_linear() {
        assert!((linear(0.0) - 0.0).abs() < 0.001);
        assert!((linear(0.5) - 0.5).abs() < 0.001);
        assert!((linear(1.0) - 1.0).abs() < 0.001);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// All easing functions should map [0,1] inputs to [0,1] outputs
        #[test]
        fn easing_bounded_output(t in 0.0f32..=1.0) {
            let result = ease_in_out(t);
            prop_assert!(result >= 0.0, "ease_in_out({}) = {} < 0", t, result);
            prop_assert!(result <= 1.0, "ease_in_out({}) = {} > 1", t, result);

            let result = ease_out_cubic(t);
            prop_assert!(result >= 0.0, "ease_out_cubic({}) = {} < 0", t, result);
            prop_assert!(result <= 1.0, "ease_out_cubic({}) = {} > 1", t, result);

            let result = ease_in_cubic(t);
            prop_assert!(result >= 0.0, "ease_in_cubic({}) = {} < 0", t, result);
            prop_assert!(result <= 1.0, "ease_in_cubic({}) = {} > 1", t, result);

            let result = linear(t);
            prop_assert!(result >= 0.0, "linear({}) = {} < 0", t, result);
            prop_assert!(result <= 1.0, "linear({}) = {} > 1", t, result);
        }

        /// All easing functions should be monotonically increasing
        #[test]
        fn easing_monotonic(t1 in 0.0f32..=1.0, t2 in 0.0f32..=1.0) {
            let (t_lo, t_hi) = if t1 <= t2 { (t1, t2) } else { (t2, t1) };

            prop_assert!(
                ease_in_out(t_lo) <= ease_in_out(t_hi) + 0.001,
                "ease_in_out not monotonic: f({}) > f({})",
                t_lo, t_hi
            );
            prop_assert!(
                ease_out_cubic(t_lo) <= ease_out_cubic(t_hi) + 0.001,
                "ease_out_cubic not monotonic: f({}) > f({})",
                t_lo, t_hi
            );
            prop_assert!(
                ease_in_cubic(t_lo) <= ease_in_cubic(t_hi) + 0.001,
                "ease_in_cubic not monotonic: f({}) > f({})",
                t_lo, t_hi
            );
            prop_assert!(
                linear(t_lo) <= linear(t_hi) + 0.001,
                "linear not monotonic: f({}) > f({})",
                t_lo, t_hi
            );
        }

        /// Endpoints should be fixed: f(0) = 0, f(1) = 1
        #[test]
        fn easing_endpoints_fixed(_seed in any::<u64>()) {
            // All easing functions should start at 0 and end at 1
            prop_assert!((ease_in_out(0.0) - 0.0).abs() < 0.001);
            prop_assert!((ease_in_out(1.0) - 1.0).abs() < 0.001);

            prop_assert!((ease_out_cubic(0.0) - 0.0).abs() < 0.001);
            prop_assert!((ease_out_cubic(1.0) - 1.0).abs() < 0.001);

            prop_assert!((ease_in_cubic(0.0) - 0.0).abs() < 0.001);
            prop_assert!((ease_in_cubic(1.0) - 1.0).abs() < 0.001);

            prop_assert!((linear(0.0) - 0.0).abs() < 0.001);
            prop_assert!((linear(1.0) - 1.0).abs() < 0.001);
        }
    }
}
