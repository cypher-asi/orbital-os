//! Axis-aligned rectangle type

use super::{Size, Vec2};
use serde::{Deserialize, Serialize};

/// Axis-aligned rectangle
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    /// Zero rectangle at origin
    pub const ZERO: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    /// Create a new rectangle
    #[inline]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Create from position and size
    #[inline]
    pub fn from_pos_size(pos: Vec2, size: Size) -> Self {
        Self {
            x: pos.x,
            y: pos.y,
            width: size.width,
            height: size.height,
        }
    }

    /// Create from center point and size
    #[inline]
    pub fn from_center_size(center: Vec2, size: Size) -> Self {
        Self {
            x: center.x - size.width / 2.0,
            y: center.y - size.height / 2.0,
            width: size.width,
            height: size.height,
        }
    }

    /// Get the center point
    #[inline]
    pub fn center(&self) -> Vec2 {
        Vec2::new(self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    /// Get position (top-left corner)
    #[inline]
    pub fn position(&self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    /// Get size
    #[inline]
    pub fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }

    /// Get the right edge
    #[inline]
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    /// Get the bottom edge
    #[inline]
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// Check if a point is inside the rectangle
    #[inline]
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.x && p.x < self.x + self.width && p.y >= self.y && p.y < self.y + self.height
    }

    /// Check if two rectangles intersect
    #[inline]
    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }

    /// Get intersection of two rectangles (if any)
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        if !self.intersects(other) {
            return None;
        }

        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());

        Some(Rect::new(x, y, right - x, bottom - y))
    }

    /// Expand rectangle by amount on all sides
    #[inline]
    pub fn expand(&self, amount: f32) -> Rect {
        Rect::new(
            self.x - amount,
            self.y - amount,
            self.width + amount * 2.0,
            self.height + amount * 2.0,
        )
    }

    /// Shrink rectangle by amount on all sides
    #[inline]
    pub fn shrink(&self, amount: f32) -> Rect {
        self.expand(-amount)
    }

    /// Translate rectangle by offset
    #[inline]
    pub fn translate(&self, offset: Vec2) -> Rect {
        Rect::new(
            self.x + offset.x,
            self.y + offset.y,
            self.width,
            self.height,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rect_center() {
        let r = Rect::new(100.0, 200.0, 50.0, 30.0);
        let c = r.center();
        assert!((c.x - 125.0).abs() < 0.001);
        assert!((c.y - 215.0).abs() < 0.001);
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

    #[test]
    fn test_rect_intersection() {
        let a = Rect::new(0.0, 0.0, 100.0, 100.0);
        let b = Rect::new(50.0, 50.0, 100.0, 100.0);

        let i = a.intersection(&b).unwrap();
        assert!((i.x - 50.0).abs() < 0.001);
        assert!((i.y - 50.0).abs() < 0.001);
        assert!((i.width - 50.0).abs() < 0.001);
        assert!((i.height - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_rect_expand() {
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        let expanded = r.expand(5.0);
        assert!((expanded.x - 5.0).abs() < 0.001);
        assert!((expanded.y - 15.0).abs() < 0.001);
        assert!((expanded.width - 110.0).abs() < 0.001);
        assert!((expanded.height - 60.0).abs() < 0.001);
    }

    #[test]
    fn test_rect_from_center_size() {
        let r = Rect::from_center_size(Vec2::new(100.0, 100.0), Size::new(50.0, 30.0));
        assert!((r.x - 75.0).abs() < 0.001);
        assert!((r.y - 85.0).abs() < 0.001);
        assert!((r.center().x - 100.0).abs() < 0.001);
    }
}
