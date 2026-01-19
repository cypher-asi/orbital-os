//! Core types for the desktop environment
//!
//! These types mirror the TypeScript types in `www/desktop/types.ts`
//! for interop between Rust and React.

use serde::{Deserialize, Serialize};

// =============================================================================
// Math Types
// =============================================================================

/// 2D vector for positions and offsets
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    /// Zero vector
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    /// Create a new vector
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Distance to another point
    pub fn distance(self, other: Vec2) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }
}

impl std::ops::Add for Vec2 {
    type Output = Vec2;
    fn add(self, other: Vec2) -> Vec2 {
        Vec2::new(self.x + other.x, self.y + other.y)
    }
}

impl std::ops::Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, other: Vec2) -> Vec2 {
        Vec2::new(self.x - other.x, self.y - other.y)
    }
}

impl std::ops::Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f32) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}

impl std::ops::Div<f32> for Vec2 {
    type Output = Vec2;
    fn div(self, s: f32) -> Vec2 {
        Vec2::new(self.x / s, self.y / s)
    }
}

/// 2D size
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    /// Create a new size
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    /// Convert to Vec2
    pub fn as_vec2(self) -> Vec2 {
        Vec2::new(self.width, self.height)
    }

    /// Area
    pub fn area(self) -> f32 {
        self.width * self.height
    }
}

/// Axis-aligned rectangle
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    /// Create a new rectangle
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Create from position and size
    pub fn from_pos_size(pos: Vec2, size: Size) -> Self {
        Self {
            x: pos.x,
            y: pos.y,
            width: size.width,
            height: size.height,
        }
    }

    /// Get the center point
    pub fn center(&self) -> Vec2 {
        Vec2::new(self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    /// Get position (top-left)
    pub fn position(&self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    /// Get size
    pub fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }

    /// Check if a point is inside the rectangle
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.x && p.x < self.x + self.width && p.y >= self.y && p.y < self.y + self.height
    }

    /// Check if two rectangles intersect
    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }

    /// Get the right edge
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    /// Get the bottom edge
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// Expand rectangle by amount on all sides
    pub fn expand(&self, amount: f32) -> Rect {
        Rect::new(
            self.x - amount,
            self.y - amount,
            self.width + amount * 2.0,
            self.height + amount * 2.0,
        )
    }

    /// Shrink rectangle by amount on all sides
    pub fn shrink(&self, amount: f32) -> Rect {
        self.expand(-amount)
    }
}

// =============================================================================
// Camera - Viewport state for a layer (desktop or void)
// =============================================================================

/// Camera state representing a viewport position and zoom level.
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
    pub fn new() -> Self {
        Self {
            center: Vec2::ZERO,
            zoom: 1.0,
        }
    }

    /// Create a camera at a specific position and zoom
    pub fn at(center: Vec2, zoom: f32) -> Self {
        Self { center, zoom }
    }

    /// Convert screen coordinates to layer coordinates
    pub fn screen_to_layer(&self, screen: Vec2, screen_size: Size) -> Vec2 {
        let half_screen = screen_size.as_vec2() * 0.5;
        let offset = screen - half_screen;
        self.center + offset / self.zoom
    }

    /// Convert layer coordinates to screen coordinates
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
    pub fn lerp(from: &Camera, to: &Camera, t: f32) -> Camera {
        Camera {
            center: Vec2::new(
                from.center.x + (to.center.x - from.center.x) * t,
                from.center.y + (to.center.y - from.center.y) * t,
            ),
            zoom: from.zoom + (to.zoom - from.zoom) * t,
        }
    }
}

// =============================================================================
// Style Constants
// =============================================================================

/// Frame style constants matching TypeScript FRAME_STYLE
pub struct FrameStyle {
    pub title_bar_height: f32,
    pub border_radius: f32,
    pub border_width: f32,
    pub shadow_blur: f32,
    pub shadow_offset_y: f32,
    pub resize_handle_size: f32,
    pub button_size: f32,
    pub button_spacing: f32,
    pub button_margin: f32,
}

/// Default frame style
pub const FRAME_STYLE: FrameStyle = FrameStyle {
    title_bar_height: 22.0,
    border_radius: 0.0,
    border_width: 1.0,
    shadow_blur: 20.0,
    shadow_offset_y: 4.0,
    resize_handle_size: 8.0,
    button_size: 22.0,
    button_spacing: 8.0,
    button_margin: 10.0,
};

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
    fn test_vec2_operations() {
        let a = Vec2::new(1.0, 2.0);
        let b = Vec2::new(3.0, 4.0);

        // Test Add trait
        let sum = a + b;
        assert!((sum.x - 4.0).abs() < 0.001);
        assert!((sum.y - 6.0).abs() < 0.001);

        // Test Sub trait
        let diff = b - a;
        assert!((diff.x - 2.0).abs() < 0.001);
        assert!((diff.y - 2.0).abs() < 0.001);

        // Test Mul<f32> trait
        let scaled = a * 2.0;
        assert!((scaled.x - 2.0).abs() < 0.001);
        assert!((scaled.y - 4.0).abs() < 0.001);

        // Test Div<f32> trait
        let divided = b / 2.0;
        assert!((divided.x - 1.5).abs() < 0.001);
        assert!((divided.y - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_rect_contains() {
        let rect = Rect::new(10.0, 20.0, 100.0, 50.0);

        assert!(rect.contains(Vec2::new(50.0, 40.0)));
        assert!(!rect.contains(Vec2::new(5.0, 40.0)));
        assert!(!rect.contains(Vec2::new(50.0, 100.0)));
    }

    #[test]
    fn test_rect_intersects() {
        let a = Rect::new(0.0, 0.0, 100.0, 100.0);
        let b = Rect::new(50.0, 50.0, 100.0, 100.0);
        let c = Rect::new(200.0, 200.0, 50.0, 50.0);

        assert!(a.intersects(&b));
        assert!(!a.intersects(&c));
    }
}
