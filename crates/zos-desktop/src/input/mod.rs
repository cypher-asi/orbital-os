//! Input routing module
//!
//! Provides input state machine for drag/resize operations.

mod drag;
mod result;
mod router;

pub use drag::DragState;
pub use result::InputResult;
pub use router::InputRouter;

use crate::math::{Size, Vec2};
use crate::window::WindowRegion;

/// Calculate new position and size after resize operation
pub fn calculate_resize(
    handle: WindowRegion,
    start_pos: Vec2,
    start_size: Size,
    delta: Vec2,
) -> (Vec2, Size) {
    let mut new_pos = start_pos;
    let mut new_size = start_size;

    match handle {
        WindowRegion::ResizeN => {
            new_pos.y = start_pos.y + delta.y;
            new_size.height = (start_size.height - delta.y).max(100.0);
        }
        WindowRegion::ResizeS => {
            new_size.height = (start_size.height + delta.y).max(100.0);
        }
        WindowRegion::ResizeE => {
            new_size.width = (start_size.width + delta.x).max(100.0);
        }
        WindowRegion::ResizeW => {
            new_pos.x = start_pos.x + delta.x;
            new_size.width = (start_size.width - delta.x).max(100.0);
        }
        WindowRegion::ResizeNE => {
            new_pos.y = start_pos.y + delta.y;
            new_size.width = (start_size.width + delta.x).max(100.0);
            new_size.height = (start_size.height - delta.y).max(100.0);
        }
        WindowRegion::ResizeNW => {
            new_pos.x = start_pos.x + delta.x;
            new_pos.y = start_pos.y + delta.y;
            new_size.width = (start_size.width - delta.x).max(100.0);
            new_size.height = (start_size.height - delta.y).max(100.0);
        }
        WindowRegion::ResizeSE => {
            new_size.width = (start_size.width + delta.x).max(100.0);
            new_size.height = (start_size.height + delta.y).max(100.0);
        }
        WindowRegion::ResizeSW => {
            new_pos.x = start_pos.x + delta.x;
            new_size.width = (start_size.width - delta.x).max(100.0);
            new_size.height = (start_size.height + delta.y).max(100.0);
        }
        _ => {}
    }

    (new_pos, new_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resize_south() {
        let (pos, size) = calculate_resize(
            WindowRegion::ResizeS,
            Vec2::new(100.0, 100.0),
            Size::new(400.0, 300.0),
            Vec2::new(0.0, 50.0),
        );
        assert!((pos.x - 100.0).abs() < 0.001);
        assert!((pos.y - 100.0).abs() < 0.001);
        assert!((size.width - 400.0).abs() < 0.001);
        assert!((size.height - 350.0).abs() < 0.001);
    }

    #[test]
    fn test_resize_north() {
        let (pos, size) = calculate_resize(
            WindowRegion::ResizeN,
            Vec2::new(100.0, 100.0),
            Size::new(400.0, 300.0),
            Vec2::new(0.0, -50.0),
        );
        assert!((pos.y - 50.0).abs() < 0.001);
        assert!((size.height - 350.0).abs() < 0.001);
    }
}
