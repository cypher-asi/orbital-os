//! Window Manager for the desktop environment
//!
//! Manages window lifecycle (create, close), positioning, sizing,
//! z-order, focus stack, and hit testing for input routing.
//!
//! ## Key Invariants
//!
//! - Each window has a unique ID
//! - Z-order is maintained as an ordered stack (higher = on top)
//! - Focus stack tracks window activation order
//! - Window state is ephemeral (not persisted to Axiom)

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::types::{Rect, Size, Vec2, FRAME_STYLE};

/// Unique window identifier
pub type WindowId = u64;

/// Window state
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowState {
    #[default]
    Normal,
    Minimized,
    Maximized,
    Fullscreen,
}

/// Region of a window for hit testing
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowRegion {
    TitleBar,
    Content,
    CloseButton,
    MinimizeButton,
    MaximizeButton,
    ResizeN,
    ResizeS,
    ResizeE,
    ResizeW,
    ResizeNE,
    ResizeNW,
    ResizeSE,
    ResizeSW,
}

/// A window in the desktop environment
#[derive(Clone, Debug)]
pub struct Window {
    /// Unique identifier
    pub id: WindowId,
    /// Window title
    pub title: String,
    /// Application identifier (for React routing)
    pub app_id: String,
    /// Position on infinite canvas (not screen)
    pub position: Vec2,
    /// Window size including frame
    pub size: Size,
    /// Minimum size
    pub min_size: Size,
    /// Maximum size (None = no limit)
    pub max_size: Option<Size>,
    /// Current state
    pub state: WindowState,
    /// Associated process ID (if any)
    pub process_id: Option<u64>,
    /// Z-order (higher = on top)
    pub z_order: u32,
    /// Saved position/size for restore after maximize
    pub(crate) restore_rect: Option<(Vec2, Size)>,
    /// Previous state before minimize
    pub(crate) prev_state: Option<WindowState>,
}

impl Window {
    /// Get the window's bounding rectangle
    pub fn rect(&self) -> Rect {
        Rect::from_pos_size(self.position, self.size)
    }

    /// Get the title bar rectangle
    pub fn title_bar_rect(&self) -> Rect {
        Rect::new(
            self.position.x,
            self.position.y,
            self.size.width,
            FRAME_STYLE.title_bar_height,
        )
    }

    /// Get the content area rectangle (excludes title bar)
    pub fn content_rect(&self) -> Rect {
        Rect::new(
            self.position.x,
            self.position.y + FRAME_STYLE.title_bar_height,
            self.size.width,
            self.size.height - FRAME_STYLE.title_bar_height,
        )
    }

    /// Get the close button rectangle
    fn close_button_rect(&self) -> Rect {
        let x =
            self.position.x + self.size.width - FRAME_STYLE.button_margin - FRAME_STYLE.button_size;
        let y = self.position.y + (FRAME_STYLE.title_bar_height - FRAME_STYLE.button_size) / 2.0;
        Rect::new(x, y, FRAME_STYLE.button_size, FRAME_STYLE.button_size)
    }

    /// Get the maximize button rectangle
    fn maximize_button_rect(&self) -> Rect {
        let x = self.position.x + self.size.width
            - FRAME_STYLE.button_margin
            - FRAME_STYLE.button_size * 2.0
            - FRAME_STYLE.button_spacing;
        let y = self.position.y + (FRAME_STYLE.title_bar_height - FRAME_STYLE.button_size) / 2.0;
        Rect::new(x, y, FRAME_STYLE.button_size, FRAME_STYLE.button_size)
    }

    /// Get the minimize button rectangle
    fn minimize_button_rect(&self) -> Rect {
        let x = self.position.x + self.size.width
            - FRAME_STYLE.button_margin
            - FRAME_STYLE.button_size * 3.0
            - FRAME_STYLE.button_spacing * 2.0;
        let y = self.position.y + (FRAME_STYLE.title_bar_height - FRAME_STYLE.button_size) / 2.0;
        Rect::new(x, y, FRAME_STYLE.button_size, FRAME_STYLE.button_size)
    }
}

/// Configuration for creating a window
#[derive(Clone, Debug, Default)]
pub struct WindowConfig {
    pub title: String,
    pub position: Option<Vec2>,
    pub size: Size,
    pub min_size: Option<Size>,
    pub max_size: Option<Size>,
    pub app_id: String,
    pub process_id: Option<u64>,
}

/// Window manager handling window lifecycle, z-order, and focus
pub struct WindowManager {
    /// All windows by ID
    windows: HashMap<WindowId, Window>,
    /// Focus stack (most recently focused at end)
    focus_stack: Vec<WindowId>,
    /// Next window ID
    next_id: u64,
    /// Next z-order value
    next_z: u32,
}

impl Default for WindowManager {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowManager {
    /// Create a new window manager
    pub fn new() -> Self {
        Self {
            windows: HashMap::new(),
            focus_stack: Vec::new(),
            next_id: 1,
            next_z: 1,
        }
    }

    /// Create a new window
    pub fn create(&mut self, config: WindowConfig) -> WindowId {
        let id = self.next_id;
        self.next_id += 1;

        let z_order = self.next_z;
        self.next_z += 1;

        // Default position if not specified
        let position = config.position.unwrap_or_else(|| {
            // Cascade windows
            let offset = (id as f32 % 10.0) * 30.0;
            Vec2::new(100.0 + offset, 100.0 + offset)
        });

        let window = Window {
            id,
            title: config.title,
            app_id: config.app_id,
            position,
            size: config.size,
            min_size: config.min_size.unwrap_or(Size::new(200.0, 150.0)),
            max_size: config.max_size,
            state: WindowState::Normal,
            process_id: config.process_id,
            z_order,
            restore_rect: None,
            prev_state: None,
        };

        self.windows.insert(id, window);
        self.focus_stack.push(id);

        id
    }

    /// Close a window
    pub fn close(&mut self, id: WindowId) {
        self.windows.remove(&id);
        self.focus_stack.retain(|&wid| wid != id);
    }

    /// Get a window by ID
    pub fn get(&self, id: WindowId) -> Option<&Window> {
        self.windows.get(&id)
    }

    /// Get a mutable window by ID
    pub fn get_mut(&mut self, id: WindowId) -> Option<&mut Window> {
        self.windows.get_mut(&id)
    }

    /// Focus a window (brings to top)
    pub fn focus(&mut self, id: WindowId) {
        if !self.windows.contains_key(&id) {
            return;
        }

        // Remove from focus stack if present
        self.focus_stack.retain(|&wid| wid != id);
        // Add to top of focus stack
        self.focus_stack.push(id);

        // Update z-order
        if let Some(window) = self.windows.get_mut(&id) {
            window.z_order = self.next_z;
            self.next_z += 1;
        }
    }

    /// Get the currently focused window ID
    pub fn focused(&self) -> Option<WindowId> {
        // Find the topmost non-minimized window in focus stack
        for &id in self.focus_stack.iter().rev() {
            if let Some(window) = self.windows.get(&id) {
                if window.state != WindowState::Minimized {
                    return Some(id);
                }
            }
        }
        None
    }

    /// Move a window to a new position
    pub fn move_window(&mut self, id: WindowId, position: Vec2) {
        if let Some(window) = self.windows.get_mut(&id) {
            window.position = position;
        }
    }

    /// Resize a window
    pub fn resize(&mut self, id: WindowId, size: Size) {
        if let Some(window) = self.windows.get_mut(&id) {
            // Clamp to min/max
            let width = size.width.max(window.min_size.width);
            let height = size.height.max(window.min_size.height);

            let width = if let Some(max) = window.max_size {
                width.min(max.width)
            } else {
                width
            };
            let height = if let Some(max) = window.max_size {
                height.min(max.height)
            } else {
                height
            };

            window.size = Size::new(width, height);
        }
    }

    /// Set window state
    pub fn set_state(&mut self, id: WindowId, state: WindowState) {
        if let Some(window) = self.windows.get_mut(&id) {
            window.state = state;
        }
    }

    /// Minimize a window
    pub fn minimize(&mut self, id: WindowId) {
        if let Some(window) = self.windows.get_mut(&id) {
            if window.state != WindowState::Minimized {
                window.prev_state = Some(window.state);
                window.state = WindowState::Minimized;
            }
        }
    }

    /// Maximize a window (or restore if already maximized)
    pub fn maximize(&mut self, id: WindowId, workspace_bounds: Option<Rect>) {
        if let Some(window) = self.windows.get_mut(&id) {
            if window.state == WindowState::Maximized {
                // Restore
                window.state = WindowState::Normal;
                if let Some((pos, size)) = window.restore_rect.take() {
                    window.position = pos;
                    window.size = size;
                }
            } else {
                // Maximize
                window.restore_rect = Some((window.position, window.size));
                window.state = WindowState::Maximized;

                // Size to workspace bounds if provided
                if let Some(bounds) = workspace_bounds {
                    window.position = bounds.position();
                    window.size = bounds.size();
                }
            }
        }
    }

    /// Restore a minimized window
    pub fn restore(&mut self, id: WindowId) {
        if let Some(window) = self.windows.get_mut(&id) {
            if window.state == WindowState::Minimized {
                window.state = window.prev_state.unwrap_or(WindowState::Normal);
                window.prev_state = None;
            }
        }
    }

    /// Get windows sorted by z-order (back to front)
    pub fn windows_by_z(&self) -> Vec<&Window> {
        let mut windows: Vec<&Window> = self.windows.values().collect();
        windows.sort_by_key(|w| w.z_order);
        windows
    }

    /// Get all windows
    pub fn all_windows(&self) -> impl Iterator<Item = &Window> {
        self.windows.values()
    }

    /// Find window at a canvas position (topmost)
    pub fn window_at(&self, pos: Vec2) -> Option<WindowId> {
        // Check windows in reverse z-order (top to bottom)
        let mut windows: Vec<&Window> = self.windows.values().collect();
        windows.sort_by_key(|w| std::cmp::Reverse(w.z_order));

        for window in windows {
            if window.state == WindowState::Minimized {
                continue;
            }
            if window.rect().contains(pos) {
                return Some(window.id);
            }
        }
        None
    }

    /// Find which region of which window is at a canvas position
    /// Only considers windows in the provided filter set (if Some)
    pub fn region_at(&self, pos: Vec2) -> Option<(WindowId, WindowRegion)> {
        self.region_at_filtered(pos, None, 1.0)
    }

    /// Find which region of which window is at a canvas position
    /// Only considers windows in the provided filter set (if Some)
    /// The zoom parameter is used to adjust handle sizes to match screen-space handles
    pub fn region_at_filtered(
        &self,
        pos: Vec2,
        filter: Option<&[WindowId]>,
        zoom: f32,
    ) -> Option<(WindowId, WindowRegion)> {
        // Check windows in reverse z-order (top to bottom)
        let mut windows: Vec<&Window> = self.windows.values().collect();
        windows.sort_by_key(|w| std::cmp::Reverse(w.z_order));

        for window in windows {
            // Skip windows not in the filter (if filter is provided)
            if let Some(visible_ids) = filter {
                if !visible_ids.contains(&window.id) {
                    #[cfg(target_arch = "wasm32")]
                    web_sys::console::log_1(
                        &format!("[hit-test] SKIP id={} (not in filter)", window.id).into(),
                    );
                    continue;
                }
            }

            if window.state == WindowState::Minimized {
                continue;
            }

            let rect = window.rect();
            #[cfg(target_arch = "wasm32")]
            web_sys::console::log_1(&format!(
                "[hit-test] CHECK id={} z={} rect=({:.0},{:.0},{:.0},{:.0}) pos=({:.0},{:.0}) contains={}",
                window.id, window.z_order,
                rect.x, rect.y, rect.width, rect.height,
                pos.x, pos.y,
                rect.contains(pos)
            ).into());

            if !rect.contains(pos) {
                continue;
            }

            // Check title bar buttons first
            if window.close_button_rect().contains(pos) {
                return Some((window.id, WindowRegion::CloseButton));
            }
            if window.maximize_button_rect().contains(pos) {
                return Some((window.id, WindowRegion::MaximizeButton));
            }
            if window.minimize_button_rect().contains(pos) {
                return Some((window.id, WindowRegion::MinimizeButton));
            }

            // Check resize handles
            // Handle size is in screen space, so convert to canvas space based on zoom
            // Cap the sizes to reasonable maximums so they don't overwhelm the title bar when zoomed out
            let edge_handle_size = (FRAME_STYLE.resize_handle_size / zoom).min(12.0);
            // Corner handles are larger for easier diagonal targeting, but capped to not exceed title bar
            let corner_handle_size = (12.0 / zoom).min(16.0); // Max 16px so it's ~half the title bar height
            let left = rect.x;
            let right = rect.right();
            let top = rect.y;
            let bottom = rect.bottom();

            // Check corners first with larger hit area
            let in_left_corner = pos.x < left + corner_handle_size;
            let in_right_corner = pos.x > right - corner_handle_size;
            let in_top_corner = pos.y < top + corner_handle_size;
            let in_bottom_corner = pos.y > bottom - corner_handle_size;

            // Check edges with standard handle size
            let in_left = pos.x < left + edge_handle_size;
            let in_right = pos.x > right - edge_handle_size;
            let in_top = pos.y < top + edge_handle_size;
            let in_bottom = pos.y > bottom - edge_handle_size;

            // All corners first (corners take priority for diagonal resize)
            if in_top_corner && in_left_corner {
                return Some((window.id, WindowRegion::ResizeNW));
            }
            if in_top_corner && in_right_corner {
                return Some((window.id, WindowRegion::ResizeNE));
            }
            if in_bottom_corner && in_left_corner {
                return Some((window.id, WindowRegion::ResizeSW));
            }
            if in_bottom_corner && in_right_corner {
                return Some((window.id, WindowRegion::ResizeSE));
            }

            // Title bar (before edge handles - React resize handles handle the edges directly)
            // Use a minimum screen-space height for the title bar hit area when zoomed out
            // This ensures the title bar is always at least 24px on screen for easy clicking
            let min_title_bar_screen_height = 24.0;
            let title_bar_canvas_height =
                (min_title_bar_screen_height / zoom).max(FRAME_STYLE.title_bar_height);
            let title_bar_hit_rect = Rect::new(
                window.position.x,
                window.position.y,
                window.size.width,
                title_bar_canvas_height,
            );
            if title_bar_hit_rect.contains(pos) {
                return Some((window.id, WindowRegion::TitleBar));
            }

            // Edge resize handles (fallback - React handles typically intercept these first)
            if in_top {
                return Some((window.id, WindowRegion::ResizeN));
            }
            if in_bottom {
                return Some((window.id, WindowRegion::ResizeS));
            }
            if in_left {
                return Some((window.id, WindowRegion::ResizeW));
            }
            if in_right {
                return Some((window.id, WindowRegion::ResizeE));
            }

            // Content area
            return Some((window.id, WindowRegion::Content));
        }

        None
    }

    /// Get the number of windows
    pub fn count(&self) -> usize {
        self.windows.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_creation() {
        let mut wm = WindowManager::new();

        let id = wm.create(WindowConfig {
            title: "Test".to_string(),
            position: Some(Vec2::new(100.0, 100.0)),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        assert!(wm.get(id).is_some());
        assert_eq!(wm.count(), 1);
    }

    #[test]
    fn test_window_focus() {
        let mut wm = WindowManager::new();

        let id1 = wm.create(WindowConfig {
            title: "Window 1".to_string(),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        let id2 = wm.create(WindowConfig {
            title: "Window 2".to_string(),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        // Last created should be focused
        assert_eq!(wm.focused(), Some(id2));

        // Focus first window
        wm.focus(id1);
        assert_eq!(wm.focused(), Some(id1));
    }

    #[test]
    fn test_window_close() {
        let mut wm = WindowManager::new();

        let id = wm.create(WindowConfig {
            title: "Test".to_string(),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        assert_eq!(wm.count(), 1);
        wm.close(id);
        assert_eq!(wm.count(), 0);
        assert!(wm.get(id).is_none());
    }

    #[test]
    fn test_window_minimize_restore() {
        let mut wm = WindowManager::new();

        let id = wm.create(WindowConfig {
            title: "Test".to_string(),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        wm.minimize(id);
        assert_eq!(wm.get(id).unwrap().state, WindowState::Minimized);

        wm.restore(id);
        assert_eq!(wm.get(id).unwrap().state, WindowState::Normal);
    }

    #[test]
    fn test_window_maximize_restore() {
        let mut wm = WindowManager::new();

        let id = wm.create(WindowConfig {
            title: "Test".to_string(),
            position: Some(Vec2::new(100.0, 100.0)),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        let workspace_bounds = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        wm.maximize(id, Some(workspace_bounds));

        let window = wm.get(id).unwrap();
        assert_eq!(window.state, WindowState::Maximized);
        assert!((window.size.width - 1920.0).abs() < 0.001);

        // Maximize again should restore
        wm.maximize(id, Some(workspace_bounds));
        let window = wm.get(id).unwrap();
        assert_eq!(window.state, WindowState::Normal);
        assert!((window.size.width - 800.0).abs() < 0.001);
    }

    #[test]
    fn test_hit_testing() {
        let mut wm = WindowManager::new();

        let id = wm.create(WindowConfig {
            title: "Test".to_string(),
            position: Some(Vec2::new(100.0, 100.0)),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        // Point in title bar
        let (hit_id, region) = wm.region_at(Vec2::new(200.0, 116.0)).unwrap();
        assert_eq!(hit_id, id);
        assert_eq!(region, WindowRegion::TitleBar);

        // Point in content
        let (hit_id, region) = wm.region_at(Vec2::new(500.0, 400.0)).unwrap();
        assert_eq!(hit_id, id);
        assert_eq!(region, WindowRegion::Content);

        // Point outside
        assert!(wm.region_at(Vec2::new(50.0, 50.0)).is_none());
    }
}
