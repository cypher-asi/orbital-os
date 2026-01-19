//! Viewport Animation and Transition Management
//!
//! This module provides the crossfade transition system for smooth desktop switching.
//!
//! ## Crossfade Transitions
//!
//! The crossfade model uses simple opacity transitions between two layers:
//! - **Desktop layer**: The current desktop with its windows
//! - **Void layer**: The meta-layer showing all desktops as tiles
//!
//! Transitions animate opacity (0.0 to 1.0) for each layer:
//! - Enter void: Desktop fades out, void fades in
//! - Exit void: Void fades out, desktop fades in
//! - Switch desktop: Quick opacity dip and restore
//!
//! Both layers render simultaneously during transitions for smooth visual effect.

use super::types::Camera;

#[cfg(test)]
use super::types::Vec2;

// =============================================================================
// Crossfade Transition System
// =============================================================================

/// Duration for crossfade transitions in milliseconds
pub const CROSSFADE_DURATION_MS: f32 = 250.0;

/// Duration for camera animations within a layer
pub const CAMERA_ANIMATION_DURATION_MS: f32 = 300.0;

/// Type of crossfade transition
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossfadeDirection {
    /// Transitioning from desktop view to void view
    ToVoid,
    /// Transitioning from void view to desktop view
    ToDesktop,
    /// Transitioning directly between two desktops (simple crossfade)
    SwitchDesktop,
}

/// Simple crossfade transition between desktop and void layers
///
/// During a transition, both layers render simultaneously:
/// - `desktop_opacity`: Opacity of the desktop layer (0.0 = hidden, 1.0 = fully visible)
/// - `void_opacity`: Opacity of the void layer (0.0 = hidden, 1.0 = fully visible)
///
/// These opacities are complementary during transition (sum ~= 1.0 at midpoint).
#[derive(Clone, Debug)]
pub struct Crossfade {
    /// Direction of the transition
    pub direction: CrossfadeDirection,
    /// Start time (ms since epoch)
    pub start_time: f64,
    /// Duration in ms
    pub duration_ms: f32,
    /// Target desktop index (for ToDesktop transitions)
    pub target_desktop: Option<usize>,
    /// Source desktop index (for ToVoid transitions)
    pub source_desktop: Option<usize>,
}

impl Crossfade {
    /// Create a new transition to void
    pub fn to_void(now: f64, from_desktop: usize) -> Self {
        Self {
            direction: CrossfadeDirection::ToVoid,
            start_time: now,
            duration_ms: CROSSFADE_DURATION_MS,
            target_desktop: None,
            source_desktop: Some(from_desktop),
        }
    }

    /// Create a new transition to desktop
    pub fn to_desktop(now: f64, to_desktop: usize) -> Self {
        Self {
            direction: CrossfadeDirection::ToDesktop,
            start_time: now,
            duration_ms: CROSSFADE_DURATION_MS,
            target_desktop: Some(to_desktop),
            source_desktop: None,
        }
    }

    /// Create a new transition between desktops (direct crossfade)
    pub fn switch_desktop(now: f64, from_desktop: usize, to_desktop: usize) -> Self {
        Self {
            direction: CrossfadeDirection::SwitchDesktop,
            start_time: now,
            duration_ms: CROSSFADE_DURATION_MS,
            target_desktop: Some(to_desktop),
            source_desktop: Some(from_desktop),
        }
    }

    /// Get progress (0.0 to 1.0)
    pub fn progress(&self, now: f64) -> f32 {
        let elapsed = (now - self.start_time) as f32;
        (elapsed / self.duration_ms).clamp(0.0, 1.0)
    }

    /// Check if transition is complete
    pub fn is_complete(&self, now: f64) -> bool {
        self.progress(now) >= 1.0
    }

    /// Get layer opacities at current time
    ///
    /// Returns (desktop_opacity, void_opacity)
    pub fn opacities(&self, now: f64) -> (f32, f32) {
        let raw_t = self.progress(now);

        match self.direction {
            CrossfadeDirection::ToVoid => {
                // Desktop fades out, void fades in (smooth easing)
                let t = ease_out_cubic(raw_t);
                (1.0 - t, t)
            }
            CrossfadeDirection::ToDesktop => {
                // Void fades out, desktop fades in (smooth easing)
                let t = ease_out_cubic(raw_t);
                (t, 1.0 - t)
            }
            CrossfadeDirection::SwitchDesktop => {
                // Desktop-to-desktop: quick fade out and fade in
                // Use raw progress for symmetric fade-out/fade-in
                // At t=0.5, opacity is at its lowest (0.0), then returns to 1.0
                let fade = if raw_t < 0.5 {
                    // Fade out: 1.0 -> 0.0
                    1.0 - (raw_t * 2.0)
                } else {
                    // Fade in: 0.0 -> 1.0
                    (raw_t - 0.5) * 2.0
                };
                (fade, 0.0) // Void not involved in desktop switch
            }
        }
    }
}

/// Camera animation for smooth pan/zoom within a layer
#[derive(Clone, Debug)]
pub struct CameraAnimation {
    /// Starting camera state
    pub from: Camera,
    /// Target camera state
    pub to: Camera,
    /// Start time (ms)
    pub start_time: f64,
    /// Duration (ms)
    pub duration_ms: f32,
}

impl CameraAnimation {
    /// Create a new camera animation
    pub fn new(from: Camera, to: Camera, now: f64) -> Self {
        Self {
            from,
            to,
            start_time: now,
            duration_ms: CAMERA_ANIMATION_DURATION_MS,
        }
    }

    /// Create with custom duration
    pub fn with_duration(from: Camera, to: Camera, now: f64, duration_ms: f32) -> Self {
        Self {
            from,
            to,
            start_time: now,
            duration_ms,
        }
    }

    /// Get progress (0.0 to 1.0)
    pub fn progress(&self, now: f64) -> f32 {
        let elapsed = (now - self.start_time) as f32;
        (elapsed / self.duration_ms).clamp(0.0, 1.0)
    }

    /// Check if animation is complete
    pub fn is_complete(&self, now: f64) -> bool {
        self.progress(now) >= 1.0
    }

    /// Get interpolated camera at current time
    pub fn current(&self, now: f64) -> Camera {
        let t = ease_out_cubic(self.progress(now));
        Camera::lerp(&self.from, &self.to, t)
    }

    /// Get final camera state
    pub fn final_camera(&self) -> Camera {
        self.to
    }
}

/// Ease-out cubic - smooth deceleration
fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crossfade_switch_desktop() {
        let crossfade = Crossfade::switch_desktop(0.0, 0, 1);

        // At start (t=0), desktop should be visible
        let (desktop_opacity, void_opacity) = crossfade.opacities(0.0);
        assert!(desktop_opacity > 0.9, "Desktop should be visible at start");
        assert!(void_opacity < 0.1, "Void should not be visible for desktop switch");

        // At midpoint, opacity should be at minimum
        let midpoint = CROSSFADE_DURATION_MS as f64 / 2.0;
        let (mid_opacity, _) = crossfade.opacities(midpoint);
        assert!(mid_opacity < 0.1, "Opacity should be near zero at midpoint");

        // At end (t=duration), desktop should be visible again
        let (end_opacity, _) = crossfade.opacities(CROSSFADE_DURATION_MS as f64);
        assert!(end_opacity > 0.9, "Desktop should be visible at end");
    }

    #[test]
    fn test_crossfade_to_void() {
        let crossfade = Crossfade::to_void(0.0, 0);

        // At start, desktop visible, void hidden
        let (desktop_opacity, void_opacity) = crossfade.opacities(0.0);
        assert!(desktop_opacity > 0.9, "Desktop should be visible at start");
        assert!(void_opacity < 0.1, "Void should be hidden at start");

        // At end, desktop hidden, void visible
        let (desktop_opacity, void_opacity) = crossfade.opacities(CROSSFADE_DURATION_MS as f64);
        assert!(desktop_opacity < 0.1, "Desktop should be hidden at end");
        assert!(void_opacity > 0.9, "Void should be visible at end");
    }

    #[test]
    fn test_crossfade_to_desktop() {
        let crossfade = Crossfade::to_desktop(0.0, 1);

        // At start, void visible, desktop hidden
        let (desktop_opacity, void_opacity) = crossfade.opacities(0.0);
        assert!(desktop_opacity < 0.1, "Desktop should be hidden at start");
        assert!(void_opacity > 0.9, "Void should be visible at start");

        // At end, desktop visible, void hidden
        let (desktop_opacity, void_opacity) = crossfade.opacities(CROSSFADE_DURATION_MS as f64);
        assert!(desktop_opacity > 0.9, "Desktop should be visible at end");
        assert!(void_opacity < 0.1, "Void should be hidden at end");
    }

    #[test]
    fn test_camera_animation() {
        let from = Camera::at(Vec2::new(0.0, 0.0), 1.0);
        let to = Camera::at(Vec2::new(100.0, 50.0), 2.0);
        let anim = CameraAnimation::new(from, to, 0.0);

        // At start
        let current = anim.current(0.0);
        assert!((current.center.x - 0.0).abs() < 0.001);
        assert!((current.zoom - 1.0).abs() < 0.001);

        // At end
        let final_cam = anim.final_camera();
        assert!((final_cam.center.x - 100.0).abs() < 0.001);
        assert!((final_cam.center.y - 50.0).abs() < 0.001);
        assert!((final_cam.zoom - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_animation_completes() {
        let from = Camera::at(Vec2::new(0.0, 0.0), 1.0);
        let to = Camera::at(Vec2::new(100.0, 0.0), 1.0);
        let anim = CameraAnimation::new(from, to, 0.0);

        // Before completion
        assert!(!anim.is_complete(CAMERA_ANIMATION_DURATION_MS as f64 - 1.0));

        // At/after completion
        assert!(anim.is_complete(CAMERA_ANIMATION_DURATION_MS as f64));
        assert!(anim.is_complete(CAMERA_ANIMATION_DURATION_MS as f64 + 100.0));
    }
}
