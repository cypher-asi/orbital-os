//! Desktop Environment for Orbital OS
//!
//! This module implements the desktop environment with:
//! - Infinite canvas viewport with pan/zoom
//! - Window management with z-order and focus
//! - Workspace regions on the infinite canvas
//! - Input routing for window interactions
//!
//! ## Architecture
//!
//! The desktop engine runs in Rust/WASM and manages all window state.
//! React handles only window content rendering as positioned overlays.
//! Window state is ephemeral (not logged to Axiom).
//!
//! ## Key Components
//!
//! - [`DesktopEngine`]: Main engine coordinating viewport, windows, workspaces
//! - [`WindowManager`]: CRUD operations for windows, z-order, focus stack
//! - [`WorkspaceManager`]: Infinite canvas regions, workspace switching
//! - [`InputRouter`]: Pan/zoom, window drag/resize, event forwarding

mod input;
mod types;
mod windows;
mod workspaces;

use std::cell::Cell;

pub use input::{DragState, InputResult, InputRouter};
pub use types::{Rect, Size, Vec2, FRAME_STYLE};
pub use windows::{Window, WindowConfig, WindowId, WindowManager, WindowRegion, WindowState};
pub use workspaces::{Workspace, WorkspaceId, WorkspaceManager};

/// Desktop engine coordinating all desktop components
///
/// This is the main entry point for desktop operations, managing:
/// - Viewport (pan/zoom state for infinite canvas)
/// - Window manager (window CRUD, focus, z-order)
/// - Workspace manager (canvas regions)
/// - Input router (drag/resize state machine)
pub struct DesktopEngine {
    /// Viewport for infinite canvas
    pub viewport: Viewport,
    /// Window manager
    pub windows: WindowManager,
    /// Workspace manager
    pub workspaces: WorkspaceManager,
    /// Input router
    pub input: InputRouter,
}

/// Viewport for infinite canvas navigation
///
/// Uses `Cell` for interior mutability on animated state (center, animation)
/// to avoid wasm-bindgen RefCell borrow conflicts when read methods are called
/// concurrently with write methods from JavaScript event handlers.
#[derive(Debug)]
pub struct Viewport {
    /// Center position on infinite canvas (Cell for interior mutability during animation)
    center: Cell<Vec2>,
    /// Zoom level (1.0 = 100%, 0.5 = zoomed out, 2.0 = zoomed in)
    pub zoom: f32,
    /// Screen size in pixels
    pub screen_size: Size,
    /// Animation state for smooth panning
    animation: Cell<Option<ViewportAnimation>>,
}

/// Animation state for viewport transitions
#[derive(Clone, Copy, Debug)]
struct ViewportAnimation {
    /// Starting position
    start: Vec2,
    /// Target position
    target: Vec2,
    /// Animation start time (ms since epoch)
    start_time: f64,
    /// Animation duration in ms
    duration: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            center: Cell::new(Vec2::ZERO),
            zoom: 1.0,
            screen_size: Size::new(1920.0, 1080.0),
            animation: Cell::new(None),
        }
    }
}

impl Viewport {
    /// Create a new viewport with the given screen size
    pub fn new(screen_width: f32, screen_height: f32) -> Self {
        Self {
            center: Cell::new(Vec2::ZERO),
            zoom: 1.0,
            screen_size: Size::new(screen_width, screen_height),
            animation: Cell::new(None),
        }
    }

    /// Get the current center position (updates animation if active)
    pub fn center(&self) -> Vec2 {
        // Update animation before returning center
        self.update_animation();
        self.center.get()
    }

    /// Set the center position directly
    pub fn set_center(&self, center: Vec2) {
        self.center.set(center);
    }

    /// Convert screen coordinates to canvas coordinates
    pub fn screen_to_canvas(&self, screen: Vec2) -> Vec2 {
        let half_screen = self.screen_size.as_vec2() * 0.5;
        let offset = screen - half_screen;
        self.center() + offset / self.zoom
    }

    /// Convert canvas coordinates to screen coordinates
    pub fn canvas_to_screen(&self, canvas: Vec2) -> Vec2 {
        let offset = canvas - self.center();
        let half_screen = self.screen_size.as_vec2() * 0.5;
        offset * self.zoom + half_screen
    }

    /// Pan the viewport by the given delta (in screen pixels)
    pub fn pan(&mut self, dx: f32, dy: f32) {
        // Panning moves the viewport center in the opposite direction
        let c = self.center.get();
        self.center.set(Vec2::new(c.x - dx / self.zoom, c.y - dy / self.zoom));
    }

    /// Zoom the viewport around an anchor point (in screen coordinates)
    pub fn zoom_at(&mut self, factor: f32, anchor_x: f32, anchor_y: f32) {
        // Convert anchor to canvas coords before zoom
        let anchor_screen = Vec2::new(anchor_x, anchor_y);
        let anchor_canvas = self.screen_to_canvas(anchor_screen);

        // Apply zoom (clamped)
        let new_zoom = (self.zoom * factor).clamp(0.1, 10.0);
        self.zoom = new_zoom;

        // Adjust center so anchor point stays at same screen position
        let half_screen = self.screen_size.as_vec2() * 0.5;
        let anchor_offset = anchor_screen - half_screen;
        self.center.set(anchor_canvas - anchor_offset / self.zoom);
    }

    /// Get the visible rectangle on the canvas
    pub fn visible_rect(&self) -> Rect {
        let center = self.center();
        let half_size = self.screen_size.as_vec2() / self.zoom * 0.5;
        Rect::new(
            center.x - half_size.x,
            center.y - half_size.y,
            self.screen_size.width / self.zoom,
            self.screen_size.height / self.zoom,
        )
    }

    /// Animate viewport to center on a position
    pub fn animate_to(&self, target: Vec2, duration_ms: f32) {
        // Get current time from JavaScript
        let now = js_sys::Date::now();
        
        self.animation.set(Some(ViewportAnimation {
            start: self.center.get(),
            target,
            start_time: now,
            duration: duration_ms,
        }));
    }
    
    /// Update animation state (called automatically by center()).
    /// Returns true if an animation is in progress.
    fn update_animation(&self) -> bool {
        let anim = match self.animation.get() {
            Some(a) => a,
            None => return false,
        };
        
        let now = js_sys::Date::now();
        let elapsed = (now - anim.start_time) as f32;
        let t = (elapsed / anim.duration).clamp(0.0, 1.0);
        
        // Ease-out cubic for smooth deceleration
        let eased = 1.0 - (1.0 - t).powi(3);
        
        // Interpolate position
        let new_center = Vec2::new(
            anim.start.x + (anim.target.x - anim.start.x) * eased,
            anim.start.y + (anim.target.y - anim.start.y) * eased,
        );
        self.center.set(new_center);
        
        // Animation complete?
        if t >= 1.0 {
            self.center.set(anim.target);
            self.animation.set(None);
            return false;
        }
        
        true
    }
    
    /// Check if an animation is in progress
    pub fn is_animating(&self) -> bool {
        self.animation.get().is_some()
    }
    
    /// Cancel any in-progress animation
    pub fn cancel_animation(&self) {
        self.animation.set(None);
    }
}

impl Default for DesktopEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DesktopEngine {
    /// Create a new desktop engine
    pub fn new() -> Self {
        Self {
            viewport: Viewport::default(),
            windows: WindowManager::new(),
            workspaces: WorkspaceManager::new(),
            input: InputRouter::new(),
        }
    }

    /// Initialize the desktop with screen dimensions
    pub fn init(&mut self, width: f32, height: f32) {
        self.viewport.screen_size = Size::new(width, height);

        // Create default workspace centered at origin
        let workspace_id = self.workspaces.create("Main");

        // Center viewport on the first workspace
        if let Some(workspace) = self.workspaces.get(workspace_id) {
            self.viewport.set_center(workspace.bounds.center());
        }
    }

    /// Resize the viewport
    pub fn resize(&mut self, width: f32, height: f32) {
        self.viewport.screen_size = Size::new(width, height);
    }

    /// Pan the viewport
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.viewport.pan(dx, dy);
    }

    /// Zoom the viewport at anchor point
    pub fn zoom_at(&mut self, factor: f32, anchor_x: f32, anchor_y: f32) {
        self.viewport.zoom_at(factor, anchor_x, anchor_y);
    }

    /// Get all visible windows with their screen-space rectangles
    /// Returns JSON-serializable data for React positioning
    /// Only returns windows belonging to the active workspace
    ///
    /// Note: Takes &self (not &mut self) to avoid wasm-bindgen RefCell borrow
    /// conflicts when called concurrently with pointer event handlers.
    pub fn get_window_screen_rects(&self) -> Vec<WindowScreenRect> {
        // Animation is updated automatically when accessing viewport.center()
        let visible = self.viewport.visible_rect();
        let active_workspace = self.workspaces.active_workspace();
        let mut rects = Vec::new();

        for window in self.windows.windows_by_z() {
            // Skip windows not in the active workspace
            if !active_workspace.contains_window(window.id) {
                continue;
            }

            // Skip minimized windows
            if window.state == WindowState::Minimized {
                continue;
            }

            // Check if window intersects visible area
            let window_rect = window.rect();
            if !visible.intersects(&window_rect) {
                continue;
            }

            // Convert to screen coordinates
            let screen_pos = self.viewport.canvas_to_screen(window.position);
            let screen_size = Size::new(
                window.size.width * self.viewport.zoom,
                window.size.height * self.viewport.zoom,
            );

            rects.push(WindowScreenRect {
                id: window.id,
                title: window.title.clone(),
                app_id: window.app_id.clone(),
                state: window.state,
                focused: self.windows.focused() == Some(window.id),
                screen_rect: Rect::new(screen_pos.x, screen_pos.y, screen_size.width, screen_size.height),
            });
        }

        rects
    }

    /// Create a window and return its ID
    pub fn create_window(&mut self, config: WindowConfig) -> WindowId {
        let id = self.windows.create(config);

        // Add to current workspace
        let active = self.workspaces.active_index();
        self.workspaces.add_window_to_workspace(active, id);

        id
    }

    /// Close a window
    pub fn close_window(&mut self, id: WindowId) {
        self.workspaces.remove_window(id);
        self.windows.close(id);
    }

    /// Focus a window and pan to it if it's off-screen
    pub fn focus_window(&mut self, id: WindowId) {
        self.windows.focus(id);
        
        // Auto-pan to the window if it's off-screen
        if let Some(window) = self.windows.get(id) {
            // Skip minimized windows (they're not visible anyway)
            if window.state == WindowState::Minimized {
                return;
            }
            
            let window_rect = window.rect();
            let visible_rect = self.viewport.visible_rect();
            
            // Check if the window is fully or partially visible
            if !visible_rect.intersects(&window_rect) {
                // Window is completely off-screen, animate pan to center it
                let window_center = window_rect.center();
                self.viewport.animate_to(window_center, 250.0);
            }
        }
    }

    /// Move a window
    pub fn move_window(&mut self, id: WindowId, x: f32, y: f32) {
        self.windows.move_window(id, Vec2::new(x, y));
    }

    /// Resize a window
    pub fn resize_window(&mut self, id: WindowId, width: f32, height: f32) {
        self.windows.resize(id, Size::new(width, height));
    }

    /// Minimize a window
    pub fn minimize_window(&mut self, id: WindowId) {
        self.windows.minimize(id);
    }

    /// Maximize a window
    pub fn maximize_window(&mut self, id: WindowId) {
        // Get workspace bounds for maximized size
        let active_workspace = self.workspaces.active_workspace();
        self.windows.maximize(id, Some(active_workspace.bounds));
    }

    /// Restore a minimized window
    pub fn restore_window(&mut self, id: WindowId) {
        self.windows.restore(id);
    }

    /// Start a resize drag operation from a specific direction
    /// Called directly by React resize handles to bypass hit testing
    pub fn start_resize_drag(&mut self, id: WindowId, direction: &str, screen_x: f32, screen_y: f32) {
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
            self.input.start_window_resize(
                id,
                handle,
                window.position,
                window.size,
                canvas_pos,
            );
        }
    }

    /// Switch to a workspace by index
    pub fn switch_workspace(&mut self, index: usize) {
        if self.workspaces.switch_to(index) {
            // Animate viewport to workspace center
            if let Some(workspace) = self.workspaces.workspaces().get(index) {
                self.viewport.animate_to(workspace.bounds.center(), 300.0);
            }
        }
    }

    /// Create a new workspace
    pub fn create_workspace(&mut self, name: &str) -> WorkspaceId {
        self.workspaces.create(name)
    }

    /// Launch an application (creates window with app_id)
    pub fn launch_app(&mut self, app_id: &str) -> WindowId {
        // Size window to fit within the visible viewport
        let screen_w = self.viewport.screen_size.width;
        let screen_h = self.viewport.screen_size.height;
        
        // Window size: use preferred size but constrain to viewport (with padding for taskbar)
        let taskbar_height = 48.0;
        let padding = 20.0;
        let max_w = (screen_w - padding * 2.0).max(400.0);
        let max_h = (screen_h - taskbar_height - padding * 2.0).max(300.0);
        
        let win_w = 900.0_f32.min(max_w);
        let win_h = 600.0_f32.min(max_h);
        
        // Position window centered in the visible viewport
        let center = self.viewport.center();
        let pos_x = center.x - win_w / 2.0;
        let pos_y = center.y - win_h / 2.0;

        let config = WindowConfig {
            title: app_id.to_string(),
            position: Some(Vec2::new(pos_x, pos_y)),
            size: Size::new(win_w, win_h),
            min_size: Some(Size::new(400.0, 300.0)),
            max_size: None,
            app_id: app_id.to_string(),
            process_id: None,
        };

        self.create_window(config)
    }

    // =========================================================================
    // Input Handling - delegates to InputRouter but manages borrowing
    // =========================================================================

    /// Handle pointer down event
    pub fn handle_pointer_down(&mut self, x: f32, y: f32, button: u8, ctrl: bool, shift: bool) -> InputResult {
        let screen_pos = Vec2::new(x, y);
        let canvas_pos = self.viewport.screen_to_canvas(screen_pos);

        // Middle mouse button starts canvas pan
        if button == 1 {
            self.viewport.cancel_animation(); // Cancel any auto-pan animation
            self.input.start_pan(screen_pos, self.viewport.center());
            return InputResult::Handled;
        }

        // Ctrl or Shift + primary button also pans (even over windows)
        if button == 0 && (ctrl || shift) {
            self.viewport.cancel_animation(); // Cancel any auto-pan animation
            self.input.start_pan(screen_pos, self.viewport.center());
            return InputResult::Handled;
        }

        // Primary button - check for window interactions (only in active workspace)
        if button == 0 {
            let active_windows = &self.workspaces.active_workspace().windows;
            let zoom = self.viewport.zoom;
            
            if let Some((window_id, region)) = self.windows.region_at_filtered(canvas_pos, Some(active_windows), zoom) {
                match region {
                    WindowRegion::CloseButton => {
                        self.close_window(window_id);
                        return InputResult::Handled;
                    }
                    WindowRegion::MinimizeButton => {
                        self.minimize_window(window_id);
                        return InputResult::Handled;
                    }
                    WindowRegion::MaximizeButton => {
                        self.maximize_window(window_id);
                        return InputResult::Handled;
                    }
                    WindowRegion::TitleBar => {
                        self.focus_window(window_id);
                        if let Some(window) = self.windows.get(window_id) {
                            self.input.start_window_move(window_id, canvas_pos - window.position);
                        }
                        return InputResult::Handled;
                    }
                    WindowRegion::Content => {
                        self.focus_window(window_id);
                        if let Some(window) = self.windows.get(window_id) {
                            let local = canvas_pos - window.position;
                            return InputResult::Forward {
                                window_id,
                                local_x: local.x,
                                local_y: local.y,
                            };
                        }
                    }
                    // Resize handles
                    handle @ (WindowRegion::ResizeN
                    | WindowRegion::ResizeS
                    | WindowRegion::ResizeE
                    | WindowRegion::ResizeW
                    | WindowRegion::ResizeNE
                    | WindowRegion::ResizeNW
                    | WindowRegion::ResizeSE
                    | WindowRegion::ResizeSW) => {
                        self.focus_window(window_id);
                        if let Some(window) = self.windows.get(window_id) {
                            self.input.start_window_resize(
                                window_id,
                                handle,
                                window.position,
                                window.size,
                                canvas_pos,
                            );
                        }
                        return InputResult::Handled;
                    }
                }
            }
        }

        InputResult::Unhandled
    }

    /// Handle pointer move event
    pub fn handle_pointer_move(&mut self, x: f32, y: f32) -> InputResult {
        let screen_pos = Vec2::new(x, y);
        let canvas_pos = self.viewport.screen_to_canvas(screen_pos);

        if let Some(drag_state) = self.input.drag_state() {
            match drag_state {
                DragState::PanCanvas { start, start_center } => {
                    let delta = screen_pos - *start;
                    self.viewport.set_center(*start_center - delta / self.viewport.zoom);
                    return InputResult::Handled;
                }
                DragState::MoveWindow { window_id, offset } => {
                    let new_pos = canvas_pos - *offset;
                    let wid = *window_id;
                    self.move_window(wid, new_pos.x, new_pos.y);
                    return InputResult::Handled;
                }
                DragState::ResizeWindow {
                    window_id,
                    handle,
                    start_pos,
                    start_size,
                    start_mouse,
                } => {
                    let delta = canvas_pos - *start_mouse;
                    let (new_pos, new_size) = input::calculate_resize(*handle, *start_pos, *start_size, delta);
                    let wid = *window_id;
                    self.move_window(wid, new_pos.x, new_pos.y);
                    self.resize_window(wid, new_size.width, new_size.height);
                    return InputResult::Handled;
                }
            }
        }

        InputResult::Unhandled
    }

    /// Handle pointer up event
    pub fn handle_pointer_up(&mut self) -> InputResult {
        if self.input.is_dragging() {
            self.input.end_drag();
            return InputResult::Handled;
        }
        InputResult::Unhandled
    }

    /// Handle wheel event - Ctrl+scroll zooms the desktop
    pub fn handle_wheel(&mut self, _dx: f32, dy: f32, x: f32, y: f32, ctrl: bool) -> InputResult {
        if ctrl {
            // Ctrl+scroll = zoom
            let factor = if dy < 0.0 { 1.1 } else { 0.9 };
            self.zoom_at(factor, x, y);
            InputResult::Handled
        } else {
            InputResult::Unhandled
        }
    }
}

/// Window information with screen-space rectangle for React positioning
#[derive(Clone, Debug)]
pub struct WindowScreenRect {
    pub id: WindowId,
    pub title: String,
    pub app_id: String,
    pub state: WindowState,
    pub focused: bool,
    pub screen_rect: Rect,
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
        let c = viewport.center();
        assert!((c.x - 0.0).abs() < 0.001);
        assert!((c.y - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_desktop_engine_init() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        assert!((engine.viewport.screen_size.width - 1920.0).abs() < 0.001);
        assert_eq!(engine.workspaces.workspaces().len(), 1);
    }

    #[test]
    fn test_desktop_engine_window_creation() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        let id = engine.create_window(WindowConfig {
            title: "Test Window".to_string(),
            position: Some(Vec2::new(100.0, 100.0)),
            size: Size::new(800.0, 600.0),
            min_size: None,
            max_size: None,
            app_id: "test".to_string(),
            process_id: None,
        });

        assert!(engine.windows.get(id).is_some());
        assert_eq!(engine.workspaces.active_workspace().windows.len(), 1);
    }
}
