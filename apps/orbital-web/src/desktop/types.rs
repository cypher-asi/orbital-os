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
        Self { x, y, width, height }
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
    title_bar_height: 20.0,
    border_radius: 0.0,
    border_width: 1.0,
    shadow_blur: 20.0,
    shadow_offset_y: 4.0,
    resize_handle_size: 8.0,
    button_size: 16.0,
    button_spacing: 8.0,
    button_margin: 10.0,
};

#[cfg(test)]
mod tests {
    use super::*;

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
