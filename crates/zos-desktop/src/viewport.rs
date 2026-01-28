//! Viewport for canvas navigation

use crate::math::{Camera, Rect, Size, Vec2};

/// Viewport for infinite canvas navigation
///
/// Simple state holder for the current viewport position and zoom.
/// Animation is handled by CameraAnimation which updates viewport state each frame.
#[derive(Clone, Debug)]
pub struct Viewport {
    /// Center position on infinite canvas
    pub center: Vec2,
    /// Zoom level (1.0 = 100%, 0.5 = zoomed out, 2.0 = zoomed in)
    pub zoom: f32,
    /// Screen size in pixels
    pub screen_size: Size,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            center: Vec2::ZERO,
            zoom: 1.0,
            screen_size: Size::new(1920.0, 1080.0),
        }
    }
}

impl Viewport {
    /// Create a new viewport with the given screen size
    pub fn new(screen_width: f32, screen_height: f32) -> Self {
        Self {
            center: Vec2::ZERO,
            zoom: 1.0,
            screen_size: Size::new(screen_width, screen_height),
        }
    }

    /// Set the zoom level directly
    #[inline]
    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom;
    }

    /// Set the zoom level with clamping
    #[inline]
    pub fn set_zoom_clamped(&mut self, zoom: f32, min: f32, max: f32) {
        self.zoom = zoom.clamp(min, max);
    }

    /// Convert screen coordinates to canvas coordinates
    pub fn screen_to_canvas(&self, screen: Vec2) -> Vec2 {
        let half_screen = self.screen_size.as_vec2() * 0.5;
        let offset = screen - half_screen;
        self.center + offset / self.zoom
    }

    /// Convert canvas coordinates to screen coordinates
    pub fn canvas_to_screen(&self, canvas: Vec2) -> Vec2 {
        let offset = canvas - self.center;
        let half_screen = self.screen_size.as_vec2() * 0.5;
        offset * self.zoom + half_screen
    }

    /// Pan the viewport by the given delta (in screen pixels)
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.center.x -= dx / self.zoom;
        self.center.y -= dy / self.zoom;
    }

    /// Zoom the viewport around an anchor point (in screen coordinates)
    pub fn zoom_at(&mut self, factor: f32, anchor_x: f32, anchor_y: f32) {
        let anchor_screen = Vec2::new(anchor_x, anchor_y);
        let anchor_canvas = self.screen_to_canvas(anchor_screen);

        self.zoom *= factor;

        let half_screen = self.screen_size.as_vec2() * 0.5;
        let anchor_offset = anchor_screen - half_screen;
        self.center = anchor_canvas - anchor_offset / self.zoom;
    }

    /// Zoom the viewport with clamping
    pub fn zoom_at_clamped(
        &mut self,
        factor: f32,
        anchor_x: f32,
        anchor_y: f32,
        min_zoom: f32,
        max_zoom: f32,
    ) {
        let anchor_screen = Vec2::new(anchor_x, anchor_y);
        let anchor_canvas = self.screen_to_canvas(anchor_screen);

        self.zoom = (self.zoom * factor).clamp(min_zoom, max_zoom);

        let half_screen = self.screen_size.as_vec2() * 0.5;
        let anchor_offset = anchor_screen - half_screen;
        self.center = anchor_canvas - anchor_offset / self.zoom;
    }

    /// Get the visible rectangle on the canvas
    pub fn visible_rect(&self) -> Rect {
        let half_size = self.screen_size.as_vec2() / self.zoom * 0.5;
        Rect::new(
            self.center.x - half_size.x,
            self.center.y - half_size.y,
            self.screen_size.width / self.zoom,
            self.screen_size.height / self.zoom,
        )
    }

    /// Apply a camera state
    #[inline]
    pub fn apply_camera(&mut self, camera: Camera) {
        self.center = camera.center;
        self.zoom = camera.zoom;
    }

    /// Get the current state as a Camera
    #[inline]
    pub fn to_camera(&self) -> Camera {
        Camera::at(self.center, self.zoom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewport_screen_to_canvas() {
        let viewport = Viewport::new(1920.0, 1080.0);

        // Center of screen should map to viewport center
        let center = viewport.screen_to_canvas(Vec2::new(960.0, 540.0));
        assert!((center.x - 0.0).abs() < 0.001);
        assert!((center.y - 0.0).abs() < 0.001);

        // Top-left of screen
        let top_left = viewport.screen_to_canvas(Vec2::new(0.0, 0.0));
        assert!((top_left.x - (-960.0)).abs() < 0.001);
        assert!((top_left.y - (-540.0)).abs() < 0.001);
    }

    #[test]
    fn test_viewport_canvas_to_screen() {
        let viewport = Viewport::new(1920.0, 1080.0);

        // Canvas origin should map to screen center
        let screen = viewport.canvas_to_screen(Vec2::ZERO);
        assert!((screen.x - 960.0).abs() < 0.001);
        assert!((screen.y - 540.0).abs() < 0.001);
    }

    #[test]
    fn test_viewport_zoom() {
        let mut viewport = Viewport::new(1920.0, 1080.0);

        // Zoom in at center
        viewport.zoom_at(2.0, 960.0, 540.0);
        assert!((viewport.zoom - 2.0).abs() < 0.001);
        // Center should not move when zooming at center
        assert!((viewport.center.x - 0.0).abs() < 0.001);
        assert!((viewport.center.y - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_viewport_pan() {
        let mut viewport = Viewport::new(1920.0, 1080.0);

        // Pan right 100 screen pixels
        viewport.pan(-100.0, 0.0);

        // At zoom 1.0, this should move center by 100
        assert!((viewport.center.x - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_viewport_visible_rect() {
        let viewport = Viewport::new(1920.0, 1080.0);
        let rect = viewport.visible_rect();

        assert!((rect.x - (-960.0)).abs() < 0.001);
        assert!((rect.y - (-540.0)).abs() < 0.001);
        assert!((rect.width - 1920.0).abs() < 0.001);
        assert!((rect.height - 1080.0).abs() < 0.001);
    }

    #[test]
    fn test_viewport_apply_camera() {
        let mut viewport = Viewport::new(1920.0, 1080.0);
        let camera = Camera::at(Vec2::new(100.0, 200.0), 0.5);

        viewport.apply_camera(camera);

        assert!((viewport.center.x - 100.0).abs() < 0.001);
        assert!((viewport.center.y - 200.0).abs() < 0.001);
        assert!((viewport.zoom - 0.5).abs() < 0.001);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Round-trip property: screen -> canvas -> screen should return original point
        #[test]
        fn screen_canvas_roundtrip(
            screen_x in 0.0f32..1920.0,
            screen_y in 0.0f32..1080.0,
            center_x in -10000.0f32..10000.0,
            center_y in -10000.0f32..10000.0,
            zoom in 0.1f32..10.0,
        ) {
            let mut viewport = Viewport::new(1920.0, 1080.0);
            viewport.center = Vec2::new(center_x, center_y);
            viewport.zoom = zoom;

            let screen = Vec2::new(screen_x, screen_y);
            let canvas = viewport.screen_to_canvas(screen);
            let result = viewport.canvas_to_screen(canvas);

            prop_assert!((result.x - screen.x).abs() < 0.01, "x mismatch: {} vs {}", result.x, screen.x);
            prop_assert!((result.y - screen.y).abs() < 0.01, "y mismatch: {} vs {}", result.y, screen.y);
        }

        /// Zoom at anchor point should preserve that point's canvas position
        #[test]
        fn zoom_preserves_anchor(
            anchor_x in 0.0f32..1920.0,
            anchor_y in 0.0f32..1080.0,
            initial_zoom in 0.5f32..2.0,
            zoom_factor in 0.5f32..2.0,
        ) {
            let mut viewport = Viewport::new(1920.0, 1080.0);
            viewport.zoom = initial_zoom;

            let anchor_screen = Vec2::new(anchor_x, anchor_y);
            let anchor_canvas_before = viewport.screen_to_canvas(anchor_screen);

            viewport.zoom_at(zoom_factor, anchor_x, anchor_y);

            let anchor_canvas_after = viewport.screen_to_canvas(anchor_screen);

            prop_assert!(
                (anchor_canvas_before.x - anchor_canvas_after.x).abs() < 0.1,
                "anchor x shifted: {} vs {}",
                anchor_canvas_before.x,
                anchor_canvas_after.x
            );
            prop_assert!(
                (anchor_canvas_before.y - anchor_canvas_after.y).abs() < 0.1,
                "anchor y shifted: {} vs {}",
                anchor_canvas_before.y,
                anchor_canvas_after.y
            );
        }

        /// Pan should move center inversely to screen direction
        #[test]
        fn pan_moves_center_inversely(
            dx in -500.0f32..500.0,
            dy in -500.0f32..500.0,
            zoom in 0.5f32..2.0,
        ) {
            let mut viewport = Viewport::new(1920.0, 1080.0);
            viewport.zoom = zoom;
            let initial_center = viewport.center;

            viewport.pan(dx, dy);

            // Pan moves center in opposite direction, scaled by zoom
            let expected_x = initial_center.x - dx / zoom;
            let expected_y = initial_center.y - dy / zoom;

            prop_assert!((viewport.center.x - expected_x).abs() < 0.001);
            prop_assert!((viewport.center.y - expected_y).abs() < 0.001);
        }
    }
}
