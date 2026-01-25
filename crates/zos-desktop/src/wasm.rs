//! WASM exports for the desktop compositor
//!
//! This module provides wasm-bindgen exports for the DesktopEngine,
//! allowing React to interact with the desktop directly.

use wasm_bindgen::prelude::*;

use crate::engine::DesktopEngine;
use crate::math::{Size, Vec2};
use crate::window::{WindowConfig, WindowState, WindowType};

// Import js_sys::Date for timestamps
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = Date, js_name = now)]
    fn date_now() -> f64;
}

/// Desktop controller for WASM - wraps DesktopEngine with JS-friendly API
#[wasm_bindgen]
pub struct DesktopController {
    engine: DesktopEngine,
}

#[wasm_bindgen]
impl DesktopController {
    /// Create a new desktop controller
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            engine: DesktopEngine::new(),
        }
    }

    // =========================================================================
    // Initialization
    // =========================================================================

    /// Initialize the desktop with screen dimensions
    #[wasm_bindgen]
    pub fn init(&mut self, width: f32, height: f32) {
        self.engine.init(width, height);
    }

    /// Resize the desktop viewport
    #[wasm_bindgen]
    pub fn resize(&mut self, width: f32, height: f32) {
        self.engine.resize(width, height);
    }

    // =========================================================================
    // Viewport
    // =========================================================================

    /// Pan the viewport
    #[wasm_bindgen]
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.engine.pan(dx, dy);
        self.engine.mark_activity(date_now());
    }

    /// Zoom at anchor point
    #[wasm_bindgen]
    pub fn zoom_at(&mut self, factor: f32, anchor_x: f32, anchor_y: f32) {
        self.engine.zoom_at(factor, anchor_x, anchor_y);
        self.engine.mark_activity(date_now());
    }

    /// Get viewport state as JSON
    #[wasm_bindgen]
    pub fn get_viewport_json(&self) -> String {
        serde_json::to_string(&serde_json::json!({
            "center": { "x": self.engine.viewport.center.x, "y": self.engine.viewport.center.y },
            "zoom": self.engine.viewport.zoom,
            "screenSize": {
                "width": self.engine.viewport.screen_size.width,
                "height": self.engine.viewport.screen_size.height
            }
        }))
        .unwrap_or_else(|_| "{}".to_string())
    }

    // =========================================================================
    // Windows
    // =========================================================================

    /// Create a new window
    #[wasm_bindgen]
    #[allow(clippy::too_many_arguments)]
    pub fn create_window(
        &mut self,
        title: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        app_id: &str,
        content_interactive: bool,
    ) -> u64 {
        let config = WindowConfig {
            title: title.to_string(),
            position: Some(Vec2::new(x, y)),
            size: Size::new(w, h),
            min_size: Some(Size::new(200.0, 150.0)),
            max_size: None,
            app_id: app_id.to_string(),
            process_id: None,
            content_interactive,
            window_type: WindowType::Standard,
        };
        self.engine.create_window(config)
    }

    /// Close a window
    #[wasm_bindgen]
    pub fn close_window(&mut self, id: u64) {
        self.engine.close_window(id);
    }

    /// Get the process ID for a window (if any)
    #[wasm_bindgen]
    pub fn get_window_process_id(&self, id: u64) -> Option<u64> {
        self.engine.get_window_process_id(id)
    }

    /// Set the process ID for a window
    ///
    /// Links a window to its associated process for:
    /// - Automatic process termination when window closes
    /// - Per-process console output routing
    #[wasm_bindgen]
    pub fn set_window_process_id(&mut self, window_id: u64, process_id: u64) {
        self.engine.set_window_process_id(window_id, process_id);
    }

    /// Focus a window
    #[wasm_bindgen]
    pub fn focus_window(&mut self, id: u64) {
        self.engine.focus_window(id);
    }

    /// Move a window
    #[wasm_bindgen]
    pub fn move_window(&mut self, id: u64, x: f32, y: f32) {
        self.engine.move_window(id, x, y);
    }

    /// Resize a window
    #[wasm_bindgen]
    pub fn resize_window(&mut self, id: u64, w: f32, h: f32) {
        self.engine.resize_window(id, w, h);
    }

    /// Minimize a window
    #[wasm_bindgen]
    pub fn minimize_window(&mut self, id: u64) {
        self.engine.minimize_window(id);
    }

    /// Maximize a window
    #[wasm_bindgen]
    pub fn maximize_window(&mut self, id: u64) {
        self.engine.maximize_window(id);
    }

    /// Restore a window
    #[wasm_bindgen]
    pub fn restore_window(&mut self, id: u64) {
        self.engine.restore_window(id);
    }

    /// Get the focused window ID
    #[wasm_bindgen]
    pub fn get_focused_window(&self) -> Option<u64> {
        self.engine.windows.focused()
    }

    /// Pan the camera to center on a window
    #[wasm_bindgen]
    pub fn pan_to_window(&mut self, id: u64) {
        self.engine.pan_to_window(id, date_now());
    }

    /// Get all windows as JSON
    #[wasm_bindgen]
    pub fn get_windows_json(&self) -> String {
        let active_desktop = self.engine.desktops.active_desktop();
        let focused_id = self.engine.windows.focused();

        let mut windows_vec: Vec<_> = self
            .engine
            .windows
            .all_windows()
            .filter(|w| active_desktop.contains_window(w.id))
            .collect();
        windows_vec.sort_by_key(|w| w.id);

        let windows: Vec<serde_json::Value> = windows_vec
            .iter()
            .map(|w| window_to_json(w, focused_id))
            .collect();
        serde_json::to_string(&windows).unwrap_or_else(|_| "[]".to_string())
    }

    /// Get window screen rects as JSON
    #[wasm_bindgen]
    pub fn get_window_screen_rects_json(&self) -> String {
        let now = date_now();
        let rects = self.engine.get_window_screen_rects(now);
        let json_rects: Vec<serde_json::Value> = rects
            .into_iter()
            .enumerate()
            .map(|(z_order, r)| {
                serde_json::json!({
                    "id": r.id,
                    "title": r.title,
                    "appId": r.app_id,
                    "state": window_state_to_str(r.state),
                    "windowType": window_type_to_str(r.window_type),
                    "focused": r.focused,
                    "zOrder": z_order,
                    "opacity": r.opacity,
                    "screenRect": {
                        "x": r.screen_rect.x,
                        "y": r.screen_rect.y,
                        "width": r.screen_rect.width,
                        "height": r.screen_rect.height
                    }
                })
            })
            .collect();
        serde_json::to_string(&json_rects).unwrap_or_else(|_| "[]".to_string())
    }

    /// Launch an application
    #[wasm_bindgen]
    pub fn launch_app(&mut self, app_id: &str) -> u64 {
        self.engine.launch_app(app_id)
    }

    // =========================================================================
    // Desktops (Workspaces)
    // =========================================================================

    /// Create a new desktop and automatically switch to it
    #[wasm_bindgen]
    pub fn create_desktop(&mut self, name: &str) -> u32 {
        let desktop_id = self.engine.create_desktop(name);

        // Automatically switch to the newly created desktop
        if let Some(index) = self.engine.desktops.index_of(desktop_id) {
            self.engine.switch_desktop(index, date_now());
        }

        desktop_id
    }

    /// Switch to a desktop by index
    #[wasm_bindgen]
    pub fn switch_desktop(&mut self, index: u32) {
        self.engine.switch_desktop(index as usize, date_now());
    }

    /// Get active desktop index
    #[wasm_bindgen]
    pub fn get_active_desktop(&self) -> u32 {
        self.engine.desktops.active_index() as u32
    }

    /// Get visual active desktop (during transitions)
    #[wasm_bindgen]
    pub fn get_visual_active_desktop(&self) -> u32 {
        self.engine.get_visual_active_workspace() as u32
    }

    /// Set background for a desktop
    #[wasm_bindgen]
    pub fn set_desktop_background(&mut self, desktop_index: u32, background: &str) {
        self.engine
            .set_desktop_background(desktop_index as usize, background);
    }

    /// Get all desktops as JSON
    #[wasm_bindgen]
    pub fn get_desktops_json(&self) -> String {
        let active = self.engine.desktops.active_index();
        let desktops: Vec<serde_json::Value> = self
            .engine
            .desktops
            .desktops()
            .iter()
            .enumerate()
            .map(|(i, d)| {
                serde_json::json!({
                    "id": d.id,
                    "name": d.name,
                    "active": i == active,
                    "windowCount": d.windows.len()
                })
            })
            .collect();
        serde_json::to_string(&desktops).unwrap_or_else(|_| "[]".to_string())
    }

    /// Get desktop dimensions as JSON
    #[wasm_bindgen]
    pub fn get_desktop_dimensions_json(&self) -> String {
        let size = self.engine.desktops.desktop_size();
        let gap = self.engine.desktops.desktop_gap();
        serde_json::to_string(&serde_json::json!({
            "width": size.width,
            "height": size.height,
            "gap": gap
        }))
        .unwrap_or_else(|_| r#"{"width":1920,"height":1080,"gap":100}"#.to_string())
    }

    // =========================================================================
    // Void Mode
    // =========================================================================

    /// Get the current view mode
    #[wasm_bindgen]
    pub fn get_view_mode(&self) -> String {
        if self.engine.is_transitioning() {
            return "transitioning".to_string();
        }
        match self.engine.get_view_mode() {
            crate::ViewMode::Desktop { .. } => "desktop".to_string(),
            crate::ViewMode::Void => "void".to_string(),
        }
    }

    /// Check if in void mode
    #[wasm_bindgen]
    pub fn is_in_void(&self) -> bool {
        self.engine.is_in_void()
    }

    /// Enter the void
    #[wasm_bindgen]
    pub fn enter_void(&mut self) {
        self.engine.enter_void(date_now());
    }

    /// Exit the void to a specific desktop
    #[wasm_bindgen]
    pub fn exit_void(&mut self, desktop_index: u32) {
        self.engine.exit_void(desktop_index as usize, date_now());
    }

    // =========================================================================
    // Animation State
    // =========================================================================

    /// Check if any animation is active
    #[wasm_bindgen]
    pub fn is_animating(&self) -> bool {
        self.engine.is_animating(date_now())
    }

    /// Check if viewport animation is in progress
    #[wasm_bindgen]
    pub fn is_animating_viewport(&self) -> bool {
        self.engine.is_animating_viewport()
    }

    /// Check if a transition is in progress
    #[wasm_bindgen]
    pub fn is_transitioning(&self) -> bool {
        self.engine.is_transitioning()
    }

    /// Tick the transition state machine
    #[wasm_bindgen]
    pub fn tick_transition(&mut self) -> bool {
        self.engine.tick_transition(date_now())
    }

    // =========================================================================
    // Input Handling
    // =========================================================================

    /// Handle pointer down event
    #[wasm_bindgen]
    pub fn pointer_down(&mut self, x: f32, y: f32, button: u8, ctrl: bool, shift: bool) -> String {
        let result = self.engine.handle_pointer_down(x, y, button, ctrl, shift);
        serde_json::to_string(&result).unwrap_or_else(|_| r#"{"type":"unhandled"}"#.to_string())
    }

    /// Handle pointer move event
    #[wasm_bindgen]
    pub fn pointer_move(&mut self, x: f32, y: f32) -> String {
        let result = self.engine.handle_pointer_move(x, y);
        serde_json::to_string(&result).unwrap_or_else(|_| r#"{"type":"unhandled"}"#.to_string())
    }

    /// Handle pointer up event
    #[wasm_bindgen]
    pub fn pointer_up(&mut self) -> String {
        let result = self.engine.handle_pointer_up();
        serde_json::to_string(&result).unwrap_or_else(|_| r#"{"type":"unhandled"}"#.to_string())
    }

    /// Handle wheel event
    #[wasm_bindgen]
    pub fn wheel(&mut self, dx: f32, dy: f32, x: f32, y: f32, ctrl: bool) -> String {
        let result = self.engine.handle_wheel(dx, dy, x, y, ctrl);
        serde_json::to_string(&result).unwrap_or_else(|_| r#"{"type":"unhandled"}"#.to_string())
    }

    /// Start a window resize operation
    #[wasm_bindgen]
    pub fn start_window_resize(&mut self, window_id: u64, direction: &str, x: f32, y: f32) {
        self.engine.start_resize_drag(window_id, direction, x, y);
    }

    /// Start a window drag operation
    #[wasm_bindgen]
    pub fn start_window_drag(&mut self, window_id: u64, x: f32, y: f32) {
        self.engine.start_move_drag(window_id, x, y);
    }

    // =========================================================================
    // Unified Frame Tick
    // =========================================================================

    /// Unified frame tick - updates animations and returns complete frame data
    #[wasm_bindgen]
    pub fn tick_frame(&mut self) -> String {
        let now = date_now();
        self.engine.tick_transition(now);

        let windows = self.build_windows_json(now);
        let view_mode = self.get_view_mode_str();
        let workspace_info = self.build_workspace_info_json(now);
        let workspace_dims = self.build_workspace_dimensions_json();

        serde_json::to_string(&serde_json::json!({
            "viewport": {
                "center": { "x": self.engine.viewport.center.x, "y": self.engine.viewport.center.y },
                "zoom": self.engine.viewport.zoom
            },
            "windows": windows,
            "animating": self.engine.is_animating(now),
            "transitioning": self.engine.is_animating_viewport(),
            "showVoid": self.engine.should_show_void(),
            "viewMode": view_mode,
            "workspaceInfo": workspace_info,
            "workspaceDimensions": workspace_dims
        }))
        .unwrap_or_else(|_| "{}".to_string())
    }

    /// Build JSON array of window screen rects
    fn build_windows_json(&self, now: f64) -> Vec<serde_json::Value> {
        self.engine
            .get_window_screen_rects(now)
            .into_iter()
            .enumerate()
            .map(|(z_order, r)| build_window_rect_json(&r, z_order))
            .collect()
    }

    /// Get the view mode as a string
    fn get_view_mode_str(&self) -> &'static str {
        if self.engine.is_transitioning() {
            "transitioning"
        } else {
            match self.engine.get_view_mode() {
                crate::ViewMode::Desktop { .. } => "desktop",
                crate::ViewMode::Void => "void",
            }
        }
    }

    /// Build workspace info JSON
    fn build_workspace_info_json(&self, now: f64) -> serde_json::Value {
        let desktops = self.engine.desktops.desktops();
        let backgrounds: Vec<String> = desktops.iter().map(|d| d.background.clone()).collect();

        serde_json::json!({
            "count": desktops.len(),
            "active": self.engine.get_visual_active_workspace_at(now),
            "actualActive": self.engine.desktops.active_index(),
            "backgrounds": backgrounds
        })
    }

    /// Build workspace dimensions JSON
    fn build_workspace_dimensions_json(&self) -> serde_json::Value {
        let size = self.engine.desktops.desktop_size();
        let gap = self.engine.desktops.desktop_gap();

        serde_json::json!({
            "width": size.width,
            "height": size.height,
            "gap": gap
        })
    }
}

/// Build JSON for a single window screen rect
fn build_window_rect_json(
    r: &crate::engine::WindowScreenRect,
    z_order: usize,
) -> serde_json::Value {
    serde_json::json!({
        "id": r.id,
        "title": r.title,
        "appId": r.app_id,
        "processId": r.process_id,
        "state": window_state_to_str(r.state),
        "windowType": window_type_to_str(r.window_type),
        "focused": r.focused,
        "zOrder": z_order,
        "opacity": r.opacity,
        "contentInteractive": r.content_interactive,
        "screenRect": {
            "x": r.screen_rect.x,
            "y": r.screen_rect.y,
            "width": r.screen_rect.width,
            "height": r.screen_rect.height
        }
    })
}

fn window_to_json(window: &crate::window::Window, focused_id: Option<u64>) -> serde_json::Value {
    serde_json::json!({
        "id": window.id,
        "title": window.title,
        "appId": window.app_id,
        "processId": window.process_id,
        "position": { "x": window.position.x, "y": window.position.y },
        "size": { "width": window.size.width, "height": window.size.height },
        "state": window_state_to_str(window.state),
        "windowType": window_type_to_str(window.window_type),
        "zOrder": window.z_order,
        "focused": focused_id == Some(window.id)
    })
}

/// Convert WindowState to JSON-friendly string
fn window_state_to_str(state: WindowState) -> &'static str {
    match state {
        WindowState::Normal => "normal",
        WindowState::Minimized => "minimized",
        WindowState::Maximized => "maximized",
        WindowState::Fullscreen => "fullscreen",
    }
}

/// Convert WindowType to JSON-friendly string
fn window_type_to_str(window_type: WindowType) -> &'static str {
    match window_type {
        WindowType::Standard => "standard",
        WindowType::Widget => "widget",
    }
}

impl Default for DesktopController {
    fn default() -> Self {
        Self::new()
    }
}
