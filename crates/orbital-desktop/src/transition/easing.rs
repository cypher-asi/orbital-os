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
