//! Camera animation for smooth viewport transitions

use super::{ease_out_cubic, CAMERA_ANIMATION_DURATION_MS};
use crate::math::Camera;

/// Camera animation state
#[derive(Clone, Debug)]
pub struct CameraAnimation {
    /// Starting camera state
    from: Camera,
    /// Target camera state
    to: Camera,
    /// Start time (ms timestamp)
    start_ms: f64,
}

impl CameraAnimation {
    /// Create a new camera animation
    pub fn new(from: Camera, to: Camera, start_ms: f64) -> Self {
        Self { from, to, start_ms }
    }

    /// Get the progress (0.0 to 1.0)
    pub fn progress(&self, now_ms: f64) -> f32 {
        let elapsed = (now_ms - self.start_ms) as f32;
        let duration = CAMERA_ANIMATION_DURATION_MS as f32;
        (elapsed / duration).clamp(0.0, 1.0)
    }

    /// Check if animation is complete
    pub fn is_complete(&self, now_ms: f64) -> bool {
        self.progress(now_ms) >= 1.0
    }

    /// Get current camera state
    pub fn current(&self, now_ms: f64) -> Camera {
        let t = ease_out_cubic(self.progress(now_ms));
        Camera::lerp(&self.from, &self.to, t)
    }

    /// Get final camera state
    pub fn final_camera(&self) -> Camera {
        self.to
    }

    /// Get starting camera state
    pub fn start_camera(&self) -> Camera {
        self.from
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec2;

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
        assert!(anim.is_complete(CAMERA_ANIMATION_DURATION_MS as f64));
        let final_cam = anim.final_camera();
        assert!((final_cam.center.x - 100.0).abs() < 0.001);
        assert!((final_cam.center.y - 50.0).abs() < 0.001);
        assert!((final_cam.zoom - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_camera_animation_progress() {
        let from = Camera::new();
        let to = Camera::at(Vec2::new(100.0, 0.0), 1.0);
        let anim = CameraAnimation::new(from, to, 0.0);

        assert!((anim.progress(0.0) - 0.0).abs() < 0.001);
        assert!((anim.progress(CAMERA_ANIMATION_DURATION_MS as f64) - 1.0).abs() < 0.001);
    }
}
