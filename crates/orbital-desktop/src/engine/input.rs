//! Input handling for pointer events and drag operations

use crate::input::{DragState, InputResult};
use crate::math::Vec2;
use crate::window::{WindowId, WindowRegion};
use super::DesktopEngine;

impl DesktopEngine {
    /// Start move drag
    pub fn start_move_drag(&mut self, id: WindowId, screen_x: f32, screen_y: f32) {
        self.camera_animation = None;

        let window_position = match self.windows.get(id) {
            Some(window) => window.position,
            None => return,
        };

        let canvas_pos = self.viewport.screen_to_canvas(Vec2::new(screen_x, screen_y));
        let offset = canvas_pos - window_position;
        self.windows.focus(id);
        self.input.start_window_move(id, offset);
    }

    /// Start resize drag
    pub fn start_resize_drag(&mut self, id: WindowId, direction: &str, screen_x: f32, screen_y: f32) {
        self.camera_animation = None;

        let handle = match direction {
            "n" => WindowRegion::ResizeN,
            "s" => WindowRegion::ResizeS,
            "e" => WindowRegion::ResizeE,
            "w" => WindowRegion::ResizeW,
            "ne" => WindowRegion::ResizeNE,
            "nw" => WindowRegion::ResizeNW,
            "se" => WindowRegion::ResizeSE,
            "sw" => WindowRegion::ResizeSW,
            _ => return,
        };

        if let Some(window) = self.windows.get(id) {
            let canvas_pos = self.viewport.screen_to_canvas(Vec2::new(screen_x, screen_y));
            self.input.start_window_resize(id, handle, window.position, window.size, canvas_pos);
        }
    }

    /// Handle pointer down
    pub fn handle_pointer_down(&mut self, x: f32, y: f32, button: u8, ctrl: bool, shift: bool) -> InputResult {
        let screen_pos = Vec2::new(x, y);
        let canvas_pos = self.viewport.screen_to_canvas(screen_pos);

        // Middle mouse or ctrl/shift + left = pan
        if button == 1 || (button == 0 && (ctrl || shift)) {
            self.camera_animation = None;
            self.input.start_pan(screen_pos, self.viewport.center);
            return InputResult::Handled;
        }

        // Left button - check windows
        if button == 0 {
            return self.handle_left_click(canvas_pos);
        }

        InputResult::Unhandled
    }

    /// Handle left click on windows
    fn handle_left_click(&mut self, canvas_pos: Vec2) -> InputResult {
        let active_windows = &self.desktops.active_desktop().windows;
        let zoom = self.viewport.zoom;

        let (window_id, region) = match self.windows.region_at_filtered(canvas_pos, Some(active_windows), zoom) {
            Some(hit) => hit,
            None => return InputResult::Unhandled,
        };

        match region {
            WindowRegion::CloseButton => {
                self.close_window(window_id);
                InputResult::Handled
            }
            WindowRegion::MinimizeButton => {
                self.minimize_window(window_id);
                InputResult::Handled
            }
            WindowRegion::MaximizeButton => {
                self.maximize_window(window_id);
                InputResult::Handled
            }
            WindowRegion::TitleBar => {
                self.handle_title_bar_click(window_id, canvas_pos)
            }
            WindowRegion::Content => {
                self.handle_content_click(window_id, canvas_pos)
            }
            handle if handle.is_resize() => {
                self.handle_resize_click(window_id, handle, canvas_pos)
            }
            _ => InputResult::Unhandled,
        }
    }

    /// Handle click on title bar - starts window move
    fn handle_title_bar_click(&mut self, window_id: WindowId, canvas_pos: Vec2) -> InputResult {
        self.camera_animation = None;
        self.focus_window(window_id);
        if let Some(window) = self.windows.get(window_id) {
            self.input.start_window_move(window_id, canvas_pos - window.position);
        }
        InputResult::Handled
    }

    /// Handle click on content area
    fn handle_content_click(&mut self, window_id: WindowId, canvas_pos: Vec2) -> InputResult {
        self.focus_window(window_id);
        
        let window = match self.windows.get(window_id) {
            Some(w) => w,
            None => return InputResult::Unhandled,
        };

        // If content_interactive is false, clicking/dragging content moves the window
        // If content_interactive is true, forward events to the app
        if !window.content_interactive {
            self.camera_animation = None;
            self.input.start_window_move(window_id, canvas_pos - window.position);
            InputResult::Handled
        } else {
            let local = canvas_pos - window.position;
            InputResult::Forward {
                window_id,
                local_x: local.x,
                local_y: local.y,
            }
        }
    }

    /// Handle click on resize handle
    fn handle_resize_click(&mut self, window_id: WindowId, handle: WindowRegion, canvas_pos: Vec2) -> InputResult {
        self.camera_animation = None;
        self.focus_window(window_id);
        if let Some(window) = self.windows.get(window_id) {
            self.input.start_window_resize(window_id, handle, window.position, window.size, canvas_pos);
        }
        InputResult::Handled
    }

    /// Handle pointer move
    pub fn handle_pointer_move(&mut self, x: f32, y: f32) -> InputResult {
        let screen_pos = Vec2::new(x, y);
        let canvas_pos = self.viewport.screen_to_canvas(screen_pos);

        let drag_state = match self.input.drag_state() {
            Some(state) => state,
            None => return InputResult::Unhandled,
        };

        match drag_state {
            DragState::PanCanvas { start, start_center } => {
                let delta = screen_pos - *start;
                self.viewport.center = *start_center - delta / self.viewport.zoom;
                InputResult::Handled
            }
            DragState::MoveWindow { window_id, offset } => {
                let new_pos = canvas_pos - *offset;
                let wid = *window_id;
                self.move_window(wid, new_pos.x, new_pos.y);
                InputResult::Handled
            }
            DragState::ResizeWindow { window_id, handle, start_pos, start_size, start_mouse } => {
                let delta = canvas_pos - *start_mouse;
                let (new_pos, new_size) = crate::input::calculate_resize(*handle, *start_pos, *start_size, delta);
                let wid = *window_id;
                self.move_window(wid, new_pos.x, new_pos.y);
                self.resize_window(wid, new_size.width, new_size.height);
                InputResult::Handled
            }
        }
    }

    /// Handle pointer up
    pub fn handle_pointer_up(&mut self) -> InputResult {
        if self.input.is_dragging() {
            let was_pan = matches!(self.input.drag_state(), Some(DragState::PanCanvas { .. }));
            self.input.end_drag();

            if was_pan {
                self.commit_viewport_to_desktop();
            }

            return InputResult::Handled;
        }
        InputResult::Unhandled
    }

    /// Handle wheel event
    pub fn handle_wheel(&mut self, _dx: f32, dy: f32, x: f32, y: f32, ctrl: bool) -> InputResult {
        if ctrl {
            let factor = if dy < 0.0 { 1.1 } else { 0.9 };
            self.zoom_at(factor, x, y);
            InputResult::Handled
        } else {
            InputResult::Unhandled
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Size;
    use crate::window::WindowConfig;

    fn create_test_engine() -> DesktopEngine {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);
        engine
    }

    fn create_test_window(engine: &mut DesktopEngine, x: f32, y: f32) -> WindowId {
        engine.create_window(WindowConfig {
            title: "Test Window".to_string(),
            position: Some(Vec2::new(x, y)),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        })
    }

    #[test]
    fn test_pointer_down_middle_button_starts_pan() {
        let mut engine = create_test_engine();

        let result = engine.handle_pointer_down(500.0, 500.0, 1, false, false);
        
        assert!(matches!(result, InputResult::Handled));
        assert!(engine.input.is_dragging());
    }

    #[test]
    fn test_pointer_down_ctrl_click_starts_pan() {
        let mut engine = create_test_engine();

        let result = engine.handle_pointer_down(500.0, 500.0, 0, true, false);
        
        assert!(matches!(result, InputResult::Handled));
        assert!(engine.input.is_dragging());
    }

    #[test]
    fn test_pointer_down_shift_click_starts_pan() {
        let mut engine = create_test_engine();

        let result = engine.handle_pointer_down(500.0, 500.0, 0, false, true);
        
        assert!(matches!(result, InputResult::Handled));
        assert!(engine.input.is_dragging());
    }

    #[test]
    fn test_pointer_down_on_empty_area_unhandled() {
        let mut engine = create_test_engine();

        // Left click on empty area
        let result = engine.handle_pointer_down(100.0, 100.0, 0, false, false);
        
        assert!(matches!(result, InputResult::Unhandled));
    }

    #[test]
    fn test_pointer_move_during_pan_updates_viewport() {
        let mut engine = create_test_engine();

        let initial_center = engine.viewport.center;

        // Start pan
        engine.handle_pointer_down(500.0, 500.0, 1, false, false);
        
        // Move pointer
        engine.handle_pointer_move(600.0, 600.0);

        // Center should have moved
        assert!((engine.viewport.center.x - initial_center.x).abs() > 0.001 ||
                (engine.viewport.center.y - initial_center.y).abs() > 0.001);
    }

    #[test]
    fn test_pointer_up_ends_drag() {
        let mut engine = create_test_engine();

        // Start pan
        engine.handle_pointer_down(500.0, 500.0, 1, false, false);
        assert!(engine.input.is_dragging());

        // End drag
        let result = engine.handle_pointer_up();
        
        assert!(matches!(result, InputResult::Handled));
        assert!(!engine.input.is_dragging());
    }

    #[test]
    fn test_wheel_with_ctrl_zooms() {
        let mut engine = create_test_engine();

        let initial_zoom = engine.viewport.zoom;

        // Zoom in
        let result = engine.handle_wheel(0.0, -100.0, 960.0, 540.0, true);
        
        assert!(matches!(result, InputResult::Handled));
        assert!(engine.viewport.zoom > initial_zoom);
    }

    #[test]
    fn test_wheel_without_ctrl_unhandled() {
        let mut engine = create_test_engine();

        let result = engine.handle_wheel(0.0, -100.0, 960.0, 540.0, false);
        
        assert!(matches!(result, InputResult::Unhandled));
    }

    #[test]
    fn test_start_move_drag() {
        let mut engine = create_test_engine();
        let id = create_test_window(&mut engine, 100.0, 100.0);

        engine.start_move_drag(id, 150.0, 130.0);

        assert!(engine.input.is_dragging());
        assert!(engine.windows.focused() == Some(id));
    }

    #[test]
    fn test_start_resize_drag() {
        let mut engine = create_test_engine();
        let id = create_test_window(&mut engine, 100.0, 100.0);

        engine.start_resize_drag(id, "se", 900.0, 700.0);

        assert!(engine.input.is_dragging());
    }

    #[test]
    fn test_resize_drag_directions() {
        let directions = ["n", "s", "e", "w", "ne", "nw", "se", "sw"];
        
        for dir in directions {
            let mut engine = create_test_engine();
            let id = create_test_window(&mut engine, 100.0, 100.0);

            engine.start_resize_drag(id, dir, 500.0, 500.0);
            assert!(engine.input.is_dragging(), "Failed to start resize for direction: {}", dir);
            engine.input.end_drag();
        }
    }

    #[test]
    fn test_invalid_resize_direction_ignored() {
        let mut engine = create_test_engine();
        let id = create_test_window(&mut engine, 100.0, 100.0);

        engine.start_resize_drag(id, "invalid", 500.0, 500.0);

        assert!(!engine.input.is_dragging());
    }

    #[test]
    fn test_camera_animation_cancelled_on_pan() {
        let mut engine = create_test_engine();
        let id = create_test_window(&mut engine, 5000.0, 5000.0);

        // Start camera animation
        engine.pan_to_window(id, 0.0);
        assert!(engine.camera_animation.is_some());

        // Start pan - should cancel animation
        engine.handle_pointer_down(500.0, 500.0, 1, false, false);
        
        assert!(engine.camera_animation.is_none());
    }
}
