//! Crossfade transition between desktop layers
//!
//! ## State Machine
//!
//! Crossfade represents an animated transition between view states:
//!
//! ```text
//! +----------+     ToVoid      +------+
//! | Desktop  | --------------> | Void |
//! +----------+ <-------------- +------+
//!      |         ToDesktop
//!      |
//!      | SwitchDesktop
//!      v
//! +----------+
//! | Desktop' |
//! +----------+
//! ```
//!
//! ## Invariants
//!
//! - `start_ms` is always a valid timestamp
//! - Progress is always clamped to [0.0, 1.0]
//! - Desktop switch transitions never show the void layer
//! - `source_desktop` is set for ToVoid and SwitchDesktop directions
//! - `target_desktop` is set for ToDesktop and SwitchDesktop directions
//!
//! ## Timing
//!
//! - ToVoid/ToDesktop: 750ms with ease-in-out
//! - SwitchDesktop: 400ms with blackout period (35% fade out, 20% black, 45% fade in)

use super::{ease_in_out, CROSSFADE_DURATION_MS, DESKTOP_SWITCH_DURATION_MS};

/// Direction of crossfade transition
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossfadeDirection {
    /// Transitioning to void (desktop fades out, void fades in)
    ToVoid,
    /// Transitioning to desktop (void fades out, desktop fades in)
    ToDesktop,
    /// Switching between desktops (quick fade out/in)
    SwitchDesktop,
}

/// Crossfade transition state
#[derive(Clone, Debug)]
pub struct Crossfade {
    /// Start time (ms timestamp)
    pub start_ms: f64,
    /// Direction of transition
    pub direction: CrossfadeDirection,
    /// Source desktop index (for SwitchDesktop)
    pub source_desktop: Option<usize>,
    /// Target desktop index
    pub target_desktop: Option<usize>,
}

impl Crossfade {
    /// Create a transition to void
    pub fn to_void(start_ms: f64, from_desktop: usize) -> Self {
        Self {
            start_ms,
            direction: CrossfadeDirection::ToVoid,
            source_desktop: Some(from_desktop),
            target_desktop: None,
        }
    }

    /// Create a transition to desktop
    pub fn to_desktop(start_ms: f64, to_desktop: usize) -> Self {
        Self {
            start_ms,
            direction: CrossfadeDirection::ToDesktop,
            source_desktop: None,
            target_desktop: Some(to_desktop),
        }
    }

    /// Create a desktop switch transition
    pub fn switch_desktop(start_ms: f64, from: usize, to: usize) -> Self {
        Self {
            start_ms,
            direction: CrossfadeDirection::SwitchDesktop,
            source_desktop: Some(from),
            target_desktop: Some(to),
        }
    }

    /// Get the progress (0.0 to 1.0)
    pub fn progress(&self, now_ms: f64) -> f32 {
        let elapsed = (now_ms - self.start_ms) as f32;
        let duration = match self.direction {
            CrossfadeDirection::SwitchDesktop => DESKTOP_SWITCH_DURATION_MS as f32,
            _ => CROSSFADE_DURATION_MS as f32,
        };
        (elapsed / duration).clamp(0.0, 1.0)
    }

    /// Check if transition is complete
    pub fn is_complete(&self, now_ms: f64) -> bool {
        self.progress(now_ms) >= 1.0
    }

    /// Get the eased progress
    pub fn eased_progress(&self, now_ms: f64) -> f32 {
        ease_in_out(self.progress(now_ms))
    }

    /// Get layer opacities (desktop_opacity, void_opacity)
    pub fn opacities(&self, now_ms: f64) -> (f32, f32) {
        let t = self.eased_progress(now_ms);

        match self.direction {
            CrossfadeDirection::ToVoid => {
                // Desktop fades out, void fades in
                (1.0 - t, t)
            }
            CrossfadeDirection::ToDesktop => {
                // Void fades out, desktop fades in
                (t, 1.0 - t)
            }
            CrossfadeDirection::SwitchDesktop => {
                // Desktop switch with extended blackout period for smoother background transitions
                // First 35%: fade out quickly (1.0 → 0.0)
                // Middle 20%: stay at 0.0 (black) - perfect time for background switch
                // Last 45%: fade in more gradually (0.0 → 1.0)
                let opacity = if t < 0.35 {
                    // First 35%: fade out (1.0 → 0.0)
                    1.0 - (t / 0.35)
                } else if t < 0.55 {
                    // Middle 20%: stay fully transparent
                    0.0
                } else {
                    // Last 45%: fade in gradually (0.0 → 1.0)
                    (t - 0.55) / 0.45
                };
                (opacity, 0.0) // Void not visible during desktop switch
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crossfade_to_void() {
        let crossfade = Crossfade::to_void(0.0, 0);

        // At start
        let (desktop, void) = crossfade.opacities(0.0);
        assert!(desktop > 0.9);
        assert!(void < 0.1);

        // At end
        let (desktop, void) = crossfade.opacities(CROSSFADE_DURATION_MS as f64);
        assert!(desktop < 0.1);
        assert!(void > 0.9);
    }

    #[test]
    fn test_crossfade_to_desktop() {
        let crossfade = Crossfade::to_desktop(0.0, 1);

        // At start
        let (desktop, void) = crossfade.opacities(0.0);
        assert!(desktop < 0.1);
        assert!(void > 0.9);

        // At end
        let (desktop, void) = crossfade.opacities(CROSSFADE_DURATION_MS as f64);
        assert!(desktop > 0.9);
        assert!(void < 0.1);
    }

    #[test]
    fn test_crossfade_switch_desktop() {
        let crossfade = Crossfade::switch_desktop(0.0, 0, 1);

        // At start, desktop fully visible
        let (desktop, void) = crossfade.opacities(0.0);
        assert!(desktop > 0.9);
        assert!(void < 0.1);

        // At end, desktop fully visible again
        let (desktop, void) = crossfade.opacities(DESKTOP_SWITCH_DURATION_MS as f64);
        assert!(desktop > 0.9);
        assert!(void < 0.1);

        // During blackout period (40-60%), opacity should be 0
        let (desktop, _) = crossfade.opacities((DESKTOP_SWITCH_DURATION_MS * 50 / 100) as f64);
        assert!(
            desktop < 0.01,
            "Desktop opacity during blackout should be 0, was {}",
            desktop
        );
    }

    #[test]
    fn test_crossfade_progress() {
        let crossfade = Crossfade::to_void(0.0, 0);

        assert!((crossfade.progress(0.0) - 0.0).abs() < 0.001);
        assert!(crossfade.progress(CROSSFADE_DURATION_MS as f64) >= 1.0);
        assert!(crossfade.is_complete(CROSSFADE_DURATION_MS as f64));
    }
}
