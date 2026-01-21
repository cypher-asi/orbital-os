//! Void state - camera for the meta-layer showing all desktops

use crate::math::{Camera, Rect, Size, Vec2};

/// State for the Void layer where all desktops appear as tiles
///
/// The void is a separate coordinate space where desktops are arranged
/// horizontally. It has its own camera state independent of any desktop.
#[derive(Clone, Debug)]
pub struct VoidState {
    /// Camera for the void view (center, zoom)
    pub camera: Camera,
    /// Screen size for constraint calculations
    screen_size: Size,
}

impl Default for VoidState {
    fn default() -> Self {
        Self::new(Size::new(1920.0, 1080.0))
    }
}

impl VoidState {
    /// Create a new void state with the given screen size
    pub fn new(screen_size: Size) -> Self {
        Self {
            camera: Camera::new(),
            screen_size,
        }
    }

    /// Update screen size
    pub fn set_screen_size(&mut self, size: Size) {
        self.screen_size = size;
    }

    /// Get the camera
    #[inline]
    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    /// Get mutable camera reference
    #[inline]
    pub fn camera_mut(&mut self) -> &mut Camera {
        &mut self.camera
    }

    /// Set the camera state
    #[inline]
    pub fn set_camera(&mut self, camera: Camera) {
        self.camera = camera;
    }

    /// Center the void camera on a specific position
    #[inline]
    pub fn center_on(&mut self, position: Vec2) {
        self.camera.center = position;
    }

    /// Zoom the void camera with constraints (min: 0.1, max: 1.0)
    pub fn zoom_at(&mut self, factor: f32, anchor: Vec2) -> bool {
        let old_zoom = self.camera.zoom;
        self.camera
            .zoom_at_clamped(factor, anchor, self.screen_size, 0.1, 1.0);
        (self.camera.zoom - old_zoom).abs() > 0.0001
    }

    /// Pan the void camera
    #[inline]
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.camera.pan(dx, dy);
    }

    /// Constrain the void camera to keep desktops visible
    pub fn constrain_to_desktops(&mut self, desktop_bounds: &[Rect], padding: f32) {
        if desktop_bounds.is_empty() {
            return;
        }

        let min_x = desktop_bounds.iter().map(|b| b.x).fold(f32::INFINITY, f32::min);
        let max_x = desktop_bounds.iter().map(|b| b.right()).fold(f32::NEG_INFINITY, f32::max);
        let min_y = desktop_bounds.iter().map(|b| b.y).fold(f32::INFINITY, f32::min);
        let max_y = desktop_bounds.iter().map(|b| b.bottom()).fold(f32::NEG_INFINITY, f32::max);

        let bounds = Rect::new(
            min_x - padding,
            min_y - padding,
            (max_x - min_x) + padding * 2.0,
            (max_y - min_y) + padding * 2.0,
        );

        let half_visible_w = self.screen_size.width / self.camera.zoom / 2.0;
        let half_visible_h = self.screen_size.height / self.camera.zoom / 2.0;

        let center_min_x = bounds.x + half_visible_w;
        let center_max_x = bounds.x + bounds.width - half_visible_w;
        let center_min_y = bounds.y + half_visible_h;
        let center_max_y = bounds.y + bounds.height - half_visible_h;

        if center_min_x <= center_max_x {
            self.camera.center.x = self.camera.center.x.clamp(center_min_x, center_max_x);
        } else {
            self.camera.center.x = bounds.x + bounds.width / 2.0;
        }

        if center_min_y <= center_max_y {
            self.camera.center.y = self.camera.center.y.clamp(center_min_y, center_max_y);
        } else {
            self.camera.center.y = bounds.y + bounds.height / 2.0;
        }
    }

    /// Calculate the center of all desktops
    pub fn calculate_void_center(desktop_bounds: &[Rect]) -> Vec2 {
        if desktop_bounds.is_empty() {
            return Vec2::ZERO;
        }

        let min_x = desktop_bounds.iter().map(|b| b.x).fold(f32::INFINITY, f32::min);
        let max_x = desktop_bounds.iter().map(|b| b.right()).fold(f32::NEG_INFINITY, f32::max);
        let min_y = desktop_bounds.iter().map(|b| b.y).fold(f32::INFINITY, f32::min);
        let max_y = desktop_bounds.iter().map(|b| b.bottom()).fold(f32::NEG_INFINITY, f32::max);

        Vec2::new((min_x + max_x) / 2.0, (min_y + max_y) / 2.0)
    }

    /// Calculate zoom level to fit all desktops in view
    pub fn calculate_fit_zoom(desktop_bounds: &[Rect], screen_size: Size) -> f32 {
        if desktop_bounds.is_empty() {
            return 0.4;
        }

        let min_x = desktop_bounds.iter().map(|b| b.x).fold(f32::INFINITY, f32::min);
        let max_x = desktop_bounds.iter().map(|b| b.right()).fold(f32::NEG_INFINITY, f32::max);

        let total_width = max_x - min_x;
        if total_width <= 0.0 {
            return 0.4;
        }

        let fit_zoom = screen_size.width / (total_width * 1.2);
        fit_zoom.clamp(0.15, 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_void_state_default() {
        let void = VoidState::default();
        assert!((void.camera.center.x - 0.0).abs() < 0.001);
        assert!((void.camera.zoom - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_void_center_calculation() {
        let bounds = vec![
            Rect::new(-960.0, -540.0, 1920.0, 1080.0),
            Rect::new(1060.0, -540.0, 1920.0, 1080.0),
        ];
        let center = VoidState::calculate_void_center(&bounds);
        assert!((center.x - 1010.0).abs() < 0.001);
        assert!((center.y - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_void_fit_zoom() {
        let bounds = vec![
            Rect::new(-960.0, -540.0, 1920.0, 1080.0),
        ];
        let screen_size = Size::new(1920.0, 1080.0);
        let zoom = VoidState::calculate_fit_zoom(&bounds, screen_size);
        assert!((0.15..=0.5).contains(&zoom));
    }
}
