//! Desktop struct - an isolated infinite canvas

use super::DesktopId;
use crate::math::{Camera, Rect, Vec2};
use crate::window::WindowId;
use serde::{Deserialize, Serialize};

/// A desktop - an isolated infinite canvas
///
/// Each desktop is a self-contained environment with:
/// - Its own set of windows (in desktop-local coordinates)
/// - Its own camera state (center and zoom)
///
/// The `bounds` field defines where this desktop appears in the void view,
/// not a limit on the desktop's internal size (which is infinite).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Desktop {
    /// Unique identifier
    pub id: DesktopId,
    /// Human-readable name
    pub name: String,
    /// Position in void view (where this desktop appears when zoomed out)
    pub bounds: Rect,
    /// Windows in this desktop (stored by ID)
    #[serde(skip)]
    pub windows: Vec<WindowId>,
    /// Camera state (position and zoom within this desktop)
    #[serde(default)]
    pub camera: Camera,
    /// Background type (grain, mist, etc.)
    #[serde(default = "default_background")]
    pub background: String,
}

fn default_background() -> String {
    "grain".to_string()
}

impl Desktop {
    /// Create a new desktop at the given bounds
    pub fn new(id: DesktopId, name: String, bounds: Rect) -> Self {
        Self {
            id,
            name,
            bounds,
            windows: Vec::new(),
            camera: Camera::new(),
            background: default_background(),
        }
    }

    /// Get the camera state for this desktop
    #[inline]
    pub fn camera(&self) -> Camera {
        self.camera
    }

    /// Set the camera state for this desktop
    #[inline]
    pub fn set_camera(&mut self, camera: Camera) {
        self.camera = camera;
    }

    /// Save camera state (called when leaving this desktop)
    #[inline]
    pub fn save_camera(&mut self, center: Vec2, zoom: f32) {
        self.camera = Camera::at(center, zoom);
    }

    /// Reset camera to default (centered on desktop origin, zoom 1.0)
    #[inline]
    pub fn reset_camera(&mut self) {
        self.camera = Camera::new();
    }

    /// Add a window to this desktop
    pub fn add_window(&mut self, window_id: WindowId) {
        if !self.windows.contains(&window_id) {
            self.windows.push(window_id);
        }
    }

    /// Remove a window from this desktop
    pub fn remove_window(&mut self, window_id: WindowId) {
        self.windows.retain(|&id| id != window_id);
    }

    /// Check if desktop contains a window
    #[inline]
    pub fn contains_window(&self, window_id: WindowId) -> bool {
        self.windows.contains(&window_id)
    }

    /// Get the center position of this desktop in void space
    #[inline]
    pub fn void_center(&self) -> Vec2 {
        self.bounds.center()
    }

    /// Set the background for this desktop
    pub fn set_background(&mut self, background: &str) {
        self.background = background.to_string();
    }

    /// Get the background for this desktop
    #[inline]
    pub fn background(&self) -> &str {
        &self.background
    }
}

/// Persisted desktop data (for storage)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersistedDesktop {
    pub id: DesktopId,
    pub name: String,
    #[serde(default)]
    pub camera: Camera,
    #[serde(default = "default_background")]
    pub background: String,
}

impl From<&Desktop> for PersistedDesktop {
    fn from(desktop: &Desktop) -> Self {
        Self {
            id: desktop.id,
            name: desktop.name.clone(),
            camera: desktop.camera,
            background: desktop.background.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desktop_creation() {
        let desktop = Desktop::new(1, "Test".to_string(), Rect::new(0.0, 0.0, 1920.0, 1080.0));
        assert_eq!(desktop.id, 1);
        assert_eq!(desktop.name, "Test");
        assert!(desktop.windows.is_empty());
    }

    #[test]
    fn test_desktop_windows() {
        let mut desktop = Desktop::new(1, "Test".to_string(), Rect::new(0.0, 0.0, 1920.0, 1080.0));

        desktop.add_window(100);
        desktop.add_window(101);
        assert_eq!(desktop.windows.len(), 2);
        assert!(desktop.contains_window(100));

        desktop.remove_window(100);
        assert_eq!(desktop.windows.len(), 1);
        assert!(!desktop.contains_window(100));
    }

    #[test]
    fn test_desktop_camera() {
        let mut desktop = Desktop::new(1, "Test".to_string(), Rect::new(0.0, 0.0, 1920.0, 1080.0));

        desktop.save_camera(Vec2::new(100.0, 200.0), 2.0);

        let camera = desktop.camera();
        assert!((camera.center.x - 100.0).abs() < 0.001);
        assert!((camera.center.y - 200.0).abs() < 0.001);
        assert!((camera.zoom - 2.0).abs() < 0.001);
    }
}
