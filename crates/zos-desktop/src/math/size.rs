//! 2D size type for dimensions

use super::Vec2;
use serde::{Deserialize, Serialize};

/// 2D size for width and height
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    /// Zero size
    pub const ZERO: Size = Size {
        width: 0.0,
        height: 0.0,
    };

    /// Create a new size
    #[inline]
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    /// Convert to Vec2
    #[inline]
    pub fn as_vec2(self) -> Vec2 {
        Vec2::new(self.width, self.height)
    }

    /// Calculate area
    #[inline]
    pub fn area(self) -> f32 {
        self.width * self.height
    }

    /// Check if size is zero or negative
    #[inline]
    pub fn is_empty(self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    /// Get aspect ratio (width / height)
    #[inline]
    pub fn aspect_ratio(self) -> f32 {
        if self.height > 0.0 {
            self.width / self.height
        } else {
            1.0
        }
    }

    /// Scale both dimensions
    #[inline]
    pub fn scale(self, factor: f32) -> Self {
        Self::new(self.width * factor, self.height * factor)
    }

    /// Clamp size to minimum and maximum
    #[inline]
    pub fn clamp(self, min: Size, max: Size) -> Self {
        Self::new(
            self.width.clamp(min.width, max.width),
            self.height.clamp(min.height, max.height),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_area() {
        let s = Size::new(10.0, 5.0);
        assert!((s.area() - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_size_as_vec2() {
        let s = Size::new(100.0, 200.0);
        let v = s.as_vec2();
        assert!((v.x - 100.0).abs() < 0.001);
        assert!((v.y - 200.0).abs() < 0.001);
    }

    #[test]
    fn test_size_aspect_ratio() {
        let s = Size::new(1920.0, 1080.0);
        let ratio = s.aspect_ratio();
        assert!((ratio - 16.0 / 9.0).abs() < 0.001);
    }

    #[test]
    fn test_size_scale() {
        let s = Size::new(100.0, 50.0);
        let scaled = s.scale(2.0);
        assert!((scaled.width - 200.0).abs() < 0.001);
        assert!((scaled.height - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_size_clamp() {
        let s = Size::new(50.0, 500.0);
        let min = Size::new(100.0, 100.0);
        let max = Size::new(400.0, 300.0);
        let clamped = s.clamp(min, max);
        assert!((clamped.width - 100.0).abs() < 0.001);
        assert!((clamped.height - 300.0).abs() < 0.001);
    }
}
