//! Camera type for viewport transformations

use super::{Rect, Size, Vec2};
use serde::{Deserialize, Serialize};

/// Camera state representing a viewport position and zoom level
///
/// Used for both desktop-internal cameras (each desktop remembers its view state)
/// and the void camera (the meta-layer showing all desktops).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    /// Center position in the layer's coordinate space
    pub center: Vec2,
    /// Zoom level (1.0 = normal, >1.0 = zoomed in, <1.0 = zoomed out)
    pub zoom: f32,
}

impl Camera {
    /// Create a camera at default position (origin, zoom 1.0)
    #[inline]
    pub fn new() -> Self {
        Self {
            center: Vec2::ZERO,
            zoom: 1.0,
        }
    }

    /// Create a camera at a specific position and zoom
    #[inline]
    pub fn at(center: Vec2, zoom: f32) -> Self {
        Self { center, zoom }
    }

    /// Convert screen coordinates to layer coordinates
    #[inline]
    pub fn screen_to_layer(&self, screen: Vec2, screen_size: Size) -> Vec2 {
        let half_screen = screen_size.as_vec2() * 0.5;
        let offset = screen - half_screen;
        self.center + offset / self.zoom
    }

    /// Convert layer coordinates to screen coordinates
    #[inline]
    pub fn layer_to_screen(&self, layer: Vec2, screen_size: Size) -> Vec2 {
        let offset = layer - self.center;
        let half_screen = screen_size.as_vec2() * 0.5;
        offset * self.zoom + half_screen
    }

    /// Get the visible rectangle in layer coordinates
    pub fn visible_rect(&self, screen_size: Size) -> Rect {
        let half_size = screen_size.as_vec2() / self.zoom * 0.5;
        Rect::new(
            self.center.x - half_size.x,
            self.center.y - half_size.y,
            screen_size.width / self.zoom,
            screen_size.height / self.zoom,
        )
    }

    /// Pan the camera by a screen-space delta
    #[inline]
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.center.x -= dx / self.zoom;
        self.center.y -= dy / self.zoom;
    }

    /// Zoom at an anchor point (in screen coordinates)
    pub fn zoom_at(&mut self, factor: f32, anchor: Vec2, screen_size: Size) {
        let anchor_layer = self.screen_to_layer(anchor, screen_size);
        self.zoom *= factor;
        let half_screen = screen_size.as_vec2() * 0.5;
        let anchor_offset = anchor - half_screen;
        self.center = anchor_layer - anchor_offset / self.zoom;
    }

    /// Zoom with clamping
    pub fn zoom_at_clamped(
        &mut self,
        factor: f32,
        anchor: Vec2,
        screen_size: Size,
        min_zoom: f32,
        max_zoom: f32,
    ) {
        let anchor_layer = self.screen_to_layer(anchor, screen_size);
        self.zoom = (self.zoom * factor).clamp(min_zoom, max_zoom);
        let half_screen = screen_size.as_vec2() * 0.5;
        let anchor_offset = anchor - half_screen;
        self.center = anchor_layer - anchor_offset / self.zoom;
    }

    /// Linear interpolation between two cameras
    #[inline]
    pub fn lerp(from: &Camera, to: &Camera, t: f32) -> Camera {
        Camera {
            center: Vec2::lerp(from.center, to.center, t),
            zoom: from.zoom + (to.zoom - from.zoom) * t,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camera_defaults() {
        let camera = Camera::new();
        assert!((camera.center.x - 0.0).abs() < 0.001);
        assert!((camera.center.y - 0.0).abs() < 0.001);
        assert!((camera.zoom - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_camera_screen_to_layer() {
        let camera = Camera::new();
        let screen_size = Size::new(1920.0, 1080.0);

        // Center of screen should map to camera center
        let center = camera.screen_to_layer(Vec2::new(960.0, 540.0), screen_size);
        assert!((center.x - 0.0).abs() < 0.001);
        assert!((center.y - 0.0).abs() < 0.001);

        // Top-left of screen
        let top_left = camera.screen_to_layer(Vec2::new(0.0, 0.0), screen_size);
        assert!((top_left.x - (-960.0)).abs() < 0.001);
        assert!((top_left.y - (-540.0)).abs() < 0.001);
    }

    #[test]
    fn test_camera_layer_to_screen() {
        let camera = Camera::new();
        let screen_size = Size::new(1920.0, 1080.0);

        // Camera center should map to screen center
        let screen = camera.layer_to_screen(Vec2::ZERO, screen_size);
        assert!((screen.x - 960.0).abs() < 0.001);
        assert!((screen.y - 540.0).abs() < 0.001);
    }

    #[test]
    fn test_camera_pan() {
        let mut camera = Camera::new();
        camera.pan(-100.0, 0.0);
        assert!((camera.center.x - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_camera_lerp() {
        let from = Camera::at(Vec2::new(0.0, 0.0), 1.0);
        let to = Camera::at(Vec2::new(100.0, 200.0), 0.5);

        let mid = Camera::lerp(&from, &to, 0.5);
        assert!((mid.center.x - 50.0).abs() < 0.001);
        assert!((mid.center.y - 100.0).abs() < 0.001);
        assert!((mid.zoom - 0.75).abs() < 0.001);
    }

    #[test]
    fn test_camera_visible_rect() {
        let camera = Camera::new();
        let screen_size = Size::new(1920.0, 1080.0);

        let rect = camera.visible_rect(screen_size);
        assert!((rect.x - (-960.0)).abs() < 0.001);
        assert!((rect.y - (-540.0)).abs() < 0.001);
        assert!((rect.width - 1920.0).abs() < 0.001);
        assert!((rect.height - 1080.0).abs() < 0.001);
    }
}
