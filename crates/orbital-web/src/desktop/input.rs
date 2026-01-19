//! Input Router for the desktop environment
//!
//! Routes input events to appropriate handlers:
//! - Canvas pan/zoom (middle mouse, ctrl+scroll)
//! - Window drag (title bar)
//! - Window resize (edges/corners)
//! - Event forwarding to window content

use serde::{Deserialize, Serialize};

use super::types::{Size, Vec2};
use super::windows::{WindowId, WindowRegion};

/// Result of input handling
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum InputResult {
    /// Event was handled by the engine
    Handled,
    /// Event was not handled
    Unhandled,
    /// Event should be forwarded to window content
    Forward {
        #[serde(rename = "windowId")]
        window_id: WindowId,
        #[serde(rename = "localX")]
        local_x: f32,
        #[serde(rename = "localY")]
        local_y: f32,
    },
}

/// Current drag state
#[derive(Clone, Debug)]
pub enum DragState {
    /// Panning the canvas
    PanCanvas {
        /// Start position in screen coordinates
        start: Vec2,
        /// Start viewport center
        start_center: Vec2,
    },
    /// Moving a window
    MoveWindow {
        window_id: WindowId,
        /// Offset from window position to mouse position
        offset: Vec2,
    },
    /// Resizing a window
    ResizeWindow {
        window_id: WindowId,
        /// Which edge/corner is being dragged
        handle: WindowRegion,
        /// Start window position
        start_pos: Vec2,
        /// Start window size
        start_size: Size,
        /// Start mouse position in canvas coords
        start_mouse: Vec2,
    },
}

/// Input router for desktop interactions
pub struct InputRouter {
    /// Current drag state
    drag_state: Option<DragState>,
    /// Last known mouse position (screen coords)
    last_mouse_pos: Vec2,
}

impl Default for InputRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl InputRouter {
    /// Create a new input router
    pub fn new() -> Self {
        Self {
            drag_state: None,
            last_mouse_pos: Vec2::ZERO,
        }
    }

    /// Start panning the canvas
    pub fn start_pan(&mut self, start: Vec2, start_center: Vec2) {
        self.drag_state = Some(DragState::PanCanvas {
            start,
            start_center,
        });
    }

    /// Start moving a window
    pub fn start_window_move(&mut self, window_id: WindowId, offset: Vec2) {
        self.drag_state = Some(DragState::MoveWindow { window_id, offset });
    }

    /// Start resizing a window
    pub fn start_window_resize(
        &mut self,
        window_id: WindowId,
        handle: WindowRegion,
        start_pos: Vec2,
        start_size: Size,
        start_mouse: Vec2,
    ) {
        self.drag_state = Some(DragState::ResizeWindow {
            window_id,
            handle,
            start_pos,
            start_size,
            start_mouse,
        });
    }

    /// End any active drag operation
    pub fn end_drag(&mut self) {
        self.drag_state = None;
    }

    /// Check if currently dragging
    pub fn is_dragging(&self) -> bool {
        self.drag_state.is_some()
    }

    /// Get current drag state
    pub fn drag_state(&self) -> Option<&DragState> {
        self.drag_state.as_ref()
    }

    /// Update last mouse position
    pub fn set_last_mouse_pos(&mut self, pos: Vec2) {
        self.last_mouse_pos = pos;
    }
}

/// Calculate new position and size for resize operation
pub fn calculate_resize(
    handle: WindowRegion,
    start_pos: Vec2,
    start_size: Size,
    delta: Vec2,
) -> (Vec2, Size) {
    let mut new_pos = start_pos;
    let mut new_size = start_size;

    match handle {
        WindowRegion::ResizeE => {
            new_size.width = (start_size.width + delta.x).max(100.0);
        }
        WindowRegion::ResizeW => {
            let width_delta = delta.x.min(start_size.width - 100.0);
            new_pos.x = start_pos.x + width_delta;
            new_size.width = start_size.width - width_delta;
        }
        WindowRegion::ResizeS => {
            new_size.height = (start_size.height + delta.y).max(100.0);
        }
        WindowRegion::ResizeN => {
            let height_delta = delta.y.min(start_size.height - 100.0);
            new_pos.y = start_pos.y + height_delta;
            new_size.height = start_size.height - height_delta;
        }
        WindowRegion::ResizeSE => {
            new_size.width = (start_size.width + delta.x).max(100.0);
            new_size.height = (start_size.height + delta.y).max(100.0);
        }
        WindowRegion::ResizeNE => {
            new_size.width = (start_size.width + delta.x).max(100.0);
            let height_delta = delta.y.min(start_size.height - 100.0);
            new_pos.y = start_pos.y + height_delta;
            new_size.height = start_size.height - height_delta;
        }
        WindowRegion::ResizeSW => {
            let width_delta = delta.x.min(start_size.width - 100.0);
            new_pos.x = start_pos.x + width_delta;
            new_size.width = start_size.width - width_delta;
            new_size.height = (start_size.height + delta.y).max(100.0);
        }
        WindowRegion::ResizeNW => {
            let width_delta = delta.x.min(start_size.width - 100.0);
            new_pos.x = start_pos.x + width_delta;
            new_size.width = start_size.width - width_delta;
            let height_delta = delta.y.min(start_size.height - 100.0);
            new_pos.y = start_pos.y + height_delta;
            new_size.height = start_size.height - height_delta;
        }
        _ => {}
    }

    (new_pos, new_size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::{DesktopEngine, WindowConfig};

    #[test]
    fn test_input_router_pan() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        // Start pan (middle mouse button)
        let result = engine.handle_pointer_down(500.0, 500.0, 1, false, false);
        assert!(matches!(result, InputResult::Handled));
        assert!(engine.input.is_dragging());

        // Move mouse
        engine.handle_pointer_move(600.0, 600.0);

        // End pan
        engine.handle_pointer_up();
        assert!(!engine.input.is_dragging());
    }

    #[test]
    fn test_input_router_window_drag() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        // Create a window centered on canvas (so it's visible)
        // Window at canvas (-400, -300) to (400, 300), centered at origin
        let _window_id = engine.create_window(WindowConfig {
            title: "Test".to_string(),
            position: Some(Vec2::new(-400.0, -300.0)),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        // Click on title bar center - window title bar is at canvas (-400, -300)
        // Title bar center at canvas (0, -289) approximately
        // canvas_to_screen: offset = (0, -289), half_screen = (960, 540)
        // screen = (0, -289) * 1.0 + (960, 540) = (960, 251)
        let result = engine.handle_pointer_down(960.0, 251.0, 0, false, false);
        assert!(matches!(result, InputResult::Handled));
    }

    #[test]
    fn test_input_router_window_close() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        // Create a window centered on canvas
        // Window at canvas (-400, -300) to (400, 300)
        let window_id = engine.create_window(WindowConfig {
            title: "Test".to_string(),
            position: Some(Vec2::new(-400.0, -300.0)),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        assert!(engine.windows.get(window_id).is_some());

        // Close button at canvas: x = -400 + 800 - 10 - 22 = 368, y = -300 + 11 = -289
        // canvas_to_screen(368 + 11, -289) = (379, -289) + (960, 540) = (1339, 251)
        // Close button center is at canvas (368 + 11, -300 + 11) = (379, -289)
        let result = engine.handle_pointer_down(1339.0, 251.0, 0, false, false);

        assert!(matches!(result, InputResult::Handled));
        assert!(engine.windows.get(window_id).is_none());
    }

    #[test]
    fn test_calculate_resize() {
        let start_pos = Vec2::new(100.0, 100.0);
        let start_size = Size::new(400.0, 300.0);

        // Resize SE corner (expand)
        let delta = Vec2::new(50.0, 30.0);
        let (new_pos, new_size) =
            calculate_resize(WindowRegion::ResizeSE, start_pos, start_size, delta);
        assert!((new_pos.x - 100.0).abs() < 0.001);
        assert!((new_pos.y - 100.0).abs() < 0.001);
        assert!((new_size.width - 450.0).abs() < 0.001);
        assert!((new_size.height - 330.0).abs() < 0.001);

        // Resize NW corner (should move position)
        let (new_pos, new_size) =
            calculate_resize(WindowRegion::ResizeNW, start_pos, start_size, delta);
        assert!((new_pos.x - 150.0).abs() < 0.001);
        assert!((new_pos.y - 130.0).abs() < 0.001);
        assert!((new_size.width - 350.0).abs() < 0.001);
        assert!((new_size.height - 270.0).abs() < 0.001);
    }
}
