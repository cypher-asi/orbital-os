//! Camera animation

use super::DesktopEngine;
use crate::math::Camera;
use crate::transition::CameraAnimation;
use crate::window::WindowId;

impl DesktopEngine {
    /// Pan the camera to center on a window
    pub fn pan_to_window(&mut self, id: WindowId, now_ms: f64) {
        let window = match self.windows.get(id) {
            Some(w) => w,
            None => return,
        };

        // Check if we have a saved camera position for this window
        let target_camera = if let Some(saved_camera) = self.window_cameras.get(&id) {
            // Restore the saved camera position for this window
            *saved_camera
        } else {
            // First time viewing this window - center on it
            let target_center = window.position + window.size.as_vec2() * 0.5;
            Camera::at(target_center, self.viewport.zoom)
        };

        self.camera_animation = Some(CameraAnimation::new(
            self.viewport.to_camera(),
            target_camera,
            now_ms,
        ));
        self.last_activity_ms = now_ms;
    }
}
