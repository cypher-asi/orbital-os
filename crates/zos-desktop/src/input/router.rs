//! Input router state machine

use super::DragState;
use crate::math::{Size, Vec2};
use crate::window::{WindowId, WindowRegion};

/// Input router managing drag state
pub struct InputRouter {
    /// Current drag state
    drag: Option<DragState>,
}

impl Default for InputRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl InputRouter {
    /// Create a new input router
    pub fn new() -> Self {
        Self { drag: None }
    }

    /// Get current drag state
    #[inline]
    pub fn drag_state(&self) -> Option<&DragState> {
        self.drag.as_ref()
    }

    /// Check if currently dragging
    #[inline]
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Start canvas pan operation
    pub fn start_pan(&mut self, start: Vec2, start_center: Vec2) {
        self.drag = Some(DragState::PanCanvas {
            start,
            start_center,
        });
    }

    /// Start window move operation
    pub fn start_window_move(&mut self, window_id: WindowId, offset: Vec2) {
        self.drag = Some(DragState::MoveWindow { window_id, offset });
    }

    /// Start window resize operation
    pub fn start_window_resize(
        &mut self,
        window_id: WindowId,
        handle: WindowRegion,
        start_pos: Vec2,
        start_size: Size,
        start_mouse: Vec2,
    ) {
        self.drag = Some(DragState::ResizeWindow {
            window_id,
            handle,
            start_pos,
            start_size,
            start_mouse,
        });
    }

    /// End current drag operation
    pub fn end_drag(&mut self) {
        self.drag = None;
    }

    /// Cancel current drag operation (alias for end_drag)
    #[inline]
    pub fn cancel(&mut self) {
        self.end_drag();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_router_pan() {
        let mut router = InputRouter::new();
        assert!(!router.is_dragging());

        router.start_pan(Vec2::new(100.0, 100.0), Vec2::new(0.0, 0.0));
        assert!(router.is_dragging());
        assert!(matches!(
            router.drag_state(),
            Some(DragState::PanCanvas { .. })
        ));

        router.end_drag();
        assert!(!router.is_dragging());
    }

    #[test]
    fn test_input_router_move() {
        let mut router = InputRouter::new();

        router.start_window_move(1, Vec2::new(10.0, 10.0));
        assert!(router.is_dragging());

        if let Some(DragState::MoveWindow { window_id, .. }) = router.drag_state() {
            assert_eq!(*window_id, 1);
        } else {
            panic!("Expected MoveWindow state");
        }
    }

    #[test]
    fn test_input_router_resize() {
        let mut router = InputRouter::new();

        router.start_window_resize(
            1,
            WindowRegion::ResizeSE,
            Vec2::new(100.0, 100.0),
            Size::new(400.0, 300.0),
            Vec2::new(500.0, 400.0),
        );

        assert!(router.is_dragging());
        assert!(matches!(
            router.drag_state(),
            Some(DragState::ResizeWindow { .. })
        ));
    }
}
