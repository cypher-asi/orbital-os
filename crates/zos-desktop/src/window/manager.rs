//! Window manager for lifecycle, focus, and z-order

use super::{Window, WindowConfig, WindowId, WindowRegion, WindowState};
use crate::math::{Rect, Size, Vec2, FRAME_STYLE};
use std::collections::HashMap;

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
            window_type: config.window_type,
            process_id: config.process_id,
            z_order,
            restore_rect: None,
            prev_state: None,
            content_interactive: config.content_interactive,
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

        self.focus_stack.retain(|&wid| wid != id);
        self.focus_stack.push(id);

        if let Some(window) = self.windows.get_mut(&id) {
            window.z_order = self.next_z;
            self.next_z += 1;
        }
    }

    /// Get the currently focused window ID
    pub fn focused(&self) -> Option<WindowId> {
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
            let mut width = size.width.max(window.min_size.width);
            let mut height = size.height.max(window.min_size.height);

            if let Some(max) = window.max_size {
                width = width.min(max.width);
                height = height.min(max.height);
            }

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
    pub fn maximize(&mut self, id: WindowId, bounds: Option<Rect>) {
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

                if let Some(b) = bounds {
                    window.position = b.position();
                    window.size = b.size();
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
    pub fn region_at(&self, pos: Vec2) -> Option<(WindowId, WindowRegion)> {
        self.region_at_filtered(pos, None, 1.0)
    }

    /// Find region with optional filter and zoom
    pub fn region_at_filtered(
        &self,
        pos: Vec2,
        filter: Option<&[WindowId]>,
        zoom: f32,
    ) -> Option<(WindowId, WindowRegion)> {
        let mut windows: Vec<&Window> = self.windows.values().collect();
        windows.sort_by_key(|w| std::cmp::Reverse(w.z_order));

        for window in windows {
            if !self.should_test_window(window, filter) {
                continue;
            }

            if !window.rect().contains(pos) {
                continue;
            }

            if let Some(region) = self.hit_test_window(window, pos, zoom) {
                return Some((window.id, region));
            }
        }

        None
    }

    /// Check if a window should be included in hit testing
    fn should_test_window(&self, window: &Window, filter: Option<&[WindowId]>) -> bool {
        if let Some(visible_ids) = filter {
            if !visible_ids.contains(&window.id) {
                return false;
            }
        }
        window.state != WindowState::Minimized
    }

    /// Hit test a specific window at a position
    fn hit_test_window(&self, window: &Window, pos: Vec2, zoom: f32) -> Option<WindowRegion> {
        // Check buttons first (highest priority)
        if let Some(region) = hit_test_buttons(window, pos) {
            return Some(region);
        }

        // Check resize corners (before title bar to allow corner grabs)
        if let Some(region) = hit_test_resize_corners(window, pos, zoom) {
            return Some(region);
        }

        // Check title bar
        if let Some(region) = hit_test_title_bar(window, pos, zoom) {
            return Some(region);
        }

        // Check resize edges
        if let Some(region) = hit_test_resize_edges(window, pos, zoom) {
            return Some(region);
        }

        // Default to content
        Some(WindowRegion::Content)
    }

    /// Get the number of windows
    pub fn count(&self) -> usize {
        self.windows.len()
    }
}

// =============================================================================
// Hit testing helper functions
// =============================================================================

/// Hit test window buttons (close, maximize, minimize)
fn hit_test_buttons(window: &Window, pos: Vec2) -> Option<WindowRegion> {
    if window.close_button_rect().contains(pos) {
        return Some(WindowRegion::CloseButton);
    }
    if window.maximize_button_rect().contains(pos) {
        return Some(WindowRegion::MaximizeButton);
    }
    if window.minimize_button_rect().contains(pos) {
        return Some(WindowRegion::MinimizeButton);
    }
    None
}

/// Hit test resize corner handles
fn hit_test_resize_corners(window: &Window, pos: Vec2, zoom: f32) -> Option<WindowRegion> {
    let corner_handle = (12.0 / zoom).min(16.0);
    let rect = window.rect();

    let in_left_corner = pos.x < rect.x + corner_handle;
    let in_right_corner = pos.x > rect.right() - corner_handle;
    let in_top_corner = pos.y < rect.y + corner_handle;
    let in_bottom_corner = pos.y > rect.bottom() - corner_handle;

    if in_top_corner && in_left_corner {
        return Some(WindowRegion::ResizeNW);
    }
    if in_top_corner && in_right_corner {
        return Some(WindowRegion::ResizeNE);
    }
    if in_bottom_corner && in_left_corner {
        return Some(WindowRegion::ResizeSW);
    }
    if in_bottom_corner && in_right_corner {
        return Some(WindowRegion::ResizeSE);
    }
    None
}

/// Hit test title bar region
fn hit_test_title_bar(window: &Window, pos: Vec2, zoom: f32) -> Option<WindowRegion> {
    let min_title_height = 24.0;
    let title_height = (min_title_height / zoom).max(FRAME_STYLE.title_bar_height);
    let title_rect = Rect::new(
        window.position.x,
        window.position.y,
        window.size.width,
        title_height,
    );

    if title_rect.contains(pos) {
        Some(WindowRegion::TitleBar)
    } else {
        None
    }
}

/// Hit test resize edge handles (non-corner)
fn hit_test_resize_edges(window: &Window, pos: Vec2, zoom: f32) -> Option<WindowRegion> {
    let edge_handle = (FRAME_STYLE.resize_handle_size / zoom).min(12.0);
    let rect = window.rect();

    let in_left = pos.x < rect.x + edge_handle;
    let in_right = pos.x > rect.right() - edge_handle;
    let in_top = pos.y < rect.y + edge_handle;
    let in_bottom = pos.y > rect.bottom() - edge_handle;

    if in_top {
        return Some(WindowRegion::ResizeN);
    }
    if in_bottom {
        return Some(WindowRegion::ResizeS);
    }
    if in_left {
        return Some(WindowRegion::ResizeW);
    }
    if in_right {
        return Some(WindowRegion::ResizeE);
    }
    None
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

        assert_eq!(wm.focused(), Some(id2));

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

        let bounds = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        wm.maximize(id, Some(bounds));

        let window = wm.get(id).unwrap();
        assert_eq!(window.state, WindowState::Maximized);
        assert!((window.size.width - 1920.0).abs() < 0.001);

        wm.maximize(id, Some(bounds));
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
