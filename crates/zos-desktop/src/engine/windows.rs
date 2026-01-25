//! Window lifecycle and operations

use super::DesktopEngine;
use crate::desktop::DesktopId;
use crate::math::{Camera, Rect, Size, Vec2};
use crate::window::{WindowConfig, WindowId, WindowType};

impl DesktopEngine {
    /// Create a window
    pub fn create_window(&mut self, mut config: WindowConfig) -> WindowId {
        if config.position.is_none() {
            config.position = Some(self.calculate_cascade_position(&config));
        }

        let id = self.windows.create(config);
        let active = self.desktops.active_index();
        self.desktops.add_window_to_desktop(active, id);

        id
    }

    /// Calculate cascade position for a new window
    fn calculate_cascade_position(&self, config: &WindowConfig) -> Vec2 {
        let cascade_offset = 50.0;
        let max_cascade = 5.0;

        // Get the most recently CREATED window to cascade from (highest window ID)
        let last_window_pos = self
            .windows
            .all_windows()
            .max_by_key(|w| w.id)
            .map(|w| w.position);

        if let Some(last_pos) = last_window_pos {
            // Cascade from the last window's position
            let _window_count = self.windows.count() as f32;
            let _cascade_index = (_window_count % max_cascade).max(1.0);

            Vec2::new(last_pos.x + cascade_offset, last_pos.y + cascade_offset)
        } else {
            // First window - center on desktop canvas origin (0, 0)
            let desktop_center = Vec2::ZERO;
            let half_w = config.size.width / 2.0;
            let half_h = config.size.height / 2.0;
            Vec2::new(desktop_center.x - half_w, desktop_center.y - half_h)
        }
    }

    /// Close a window
    pub fn close_window(&mut self, id: WindowId) {
        // Cancel any camera animation when closing a window to prevent unwanted panning
        self.camera_animation = None;

        self.desktops.remove_window(id);
        self.windows.close(id);
        // Clean up saved camera position for this window
        self.window_cameras.remove(&id);
    }

    /// Get the process ID for a window (if any)
    pub fn get_window_process_id(&self, id: WindowId) -> Option<u64> {
        self.windows.get(id).and_then(|w| w.process_id)
    }

    /// Set the process ID for a window
    ///
    /// This links a window to its associated process, enabling:
    /// - Process termination when window is closed
    /// - Per-process console callbacks routing
    /// - Title updated to show PID for terminal windows
    pub fn set_window_process_id(&mut self, id: WindowId, process_id: u64) {
        if let Some(window) = self.windows.get_mut(id) {
            window.process_id = Some(process_id);

            // Update title to show PID for terminal windows
            if window.app_id == "terminal" {
                window.title = format!("Terminal p{}", process_id);
            }
        }
    }

    /// Focus a window
    pub fn focus_window(&mut self, id: WindowId) {
        // Save current camera position for the previously focused window
        if let Some(prev_id) = self.windows.focused() {
            if prev_id != id {
                self.window_cameras.insert(
                    prev_id,
                    Camera::at(self.viewport.center, self.viewport.zoom),
                );
            }
        }

        self.windows.focus(id);
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
        let taskbar_height = 48.0;

        // Get the visible canvas area considering current camera position and zoom
        let visible = self.viewport.visible_rect();

        // Adjust for taskbar at bottom of screen (taskbar height is in screen pixels, so scale by zoom)
        let maximize_bounds = Rect::new(
            visible.x,
            visible.y,
            visible.width,
            visible.height - taskbar_height / self.viewport.zoom,
        );
        self.windows.maximize(id, Some(maximize_bounds));
    }

    /// Restore a window
    pub fn restore_window(&mut self, id: WindowId) {
        self.windows.restore(id);
    }

    /// Create a desktop
    pub fn create_desktop(&mut self, name: &str) -> DesktopId {
        self.desktops.create(name)
    }

    /// Set background for a desktop by index
    pub fn set_desktop_background(&mut self, desktop_index: usize, background: &str) {
        self.desktops
            .set_desktop_background(desktop_index, background);
    }

    /// Switch to desktop by index
    pub fn switch_desktop(&mut self, index: usize, now_ms: f64) {
        if !self.can_switch_desktop() {
            return;
        }

        let current_index = self.desktops.active_index();
        if current_index == index {
            return;
        }

        self.desktops
            .save_desktop_camera(current_index, self.viewport.center, self.viewport.zoom);

        if self.desktops.switch_to(index) {
            self.focus_top_window_on_desktop(index);
            self.crossfade = Some(crate::transition::Crossfade::switch_desktop(
                now_ms,
                current_index,
                index,
            ));
        }
    }

    /// Check if we can switch desktops
    fn can_switch_desktop(&self) -> bool {
        // Don't allow switching during drag operations
        if self.input.is_dragging() {
            return false;
        }

        // Block if transitioning to/from void (preserve those animations)
        // but allow interrupting desktop-to-desktop switches for responsive navigation
        if let Some(ref crossfade) = self.crossfade {
            match crossfade.direction {
                crate::transition::CrossfadeDirection::ToVoid
                | crate::transition::CrossfadeDirection::ToDesktop => false,
                crate::transition::CrossfadeDirection::SwitchDesktop => true,
            }
        } else {
            true
        }
    }

    /// Launch an application (creates window with app_id)
    pub fn launch_app(&mut self, app_id: &str) -> WindowId {
        let app_config = self.get_app_config(app_id);
        let (win_w, win_h) = self.calculate_app_window_size(&app_config);

        let config = WindowConfig {
            title: app_config.title.to_string(),
            position: None,
            size: Size::new(win_w, win_h),
            min_size: Some(Size::new(app_config.min_width, app_config.min_height)),
            max_size: None,
            app_id: app_id.to_string(),
            process_id: None,
            content_interactive: app_config.content_interactive,
            window_type: app_config.window_type,
        };

        self.create_window(config)
    }

    /// Calculate window size based on screen dimensions and app config
    fn calculate_app_window_size(&self, config: &AppConfig) -> (f32, f32) {
        let screen_w = self.viewport.screen_size.width;
        let screen_h = self.viewport.screen_size.height;

        let taskbar_height = 48.0;
        let padding = 20.0;
        let max_w = (screen_w - padding * 2.0).max(400.0);
        let max_h = (screen_h - taskbar_height - padding * 2.0).max(300.0);

        // Use app-specific preferred size, clamped to screen bounds
        let win_w = config.preferred_width.min(max_w);
        let win_h = config.preferred_height.min(max_h);
        (win_w, win_h)
    }

    /// Get configuration for an app
    fn get_app_config<'a>(&self, app_id: &'a str) -> AppConfig<'a> {
        match app_id {
            "terminal" => standard_app_config("Terminal"),
            "browser" => standard_app_config("Browser"),
            "settings" => standard_app_config("Settings"),
            "clock" | "com.zero.clock" => widget_app_config(
                "Clock", 150.0, 100.0,
                // Clock: icon (64px) + time (48px) + date + info row + padding
                280.0, 280.0,
            ),
            "calculator" | "com.zero.calculator" => widget_app_config(
                "Calculator",
                200.0,
                200.0,
                // Calculator: display (~100px) + 5 rows of buttons (52px each) + gaps + padding + space for close button
                360.0,
                480.0,
            ),
            _ => standard_app_config(app_id),
        }
    }
}

fn standard_app_config(title: &str) -> AppConfig<'_> {
    AppConfig {
        title,
        content_interactive: false,
        window_type: WindowType::Standard,
        min_width: 200.0,
        min_height: 150.0,
        preferred_width: 900.0,
        preferred_height: 600.0,
    }
}

fn widget_app_config(
    title: &str,
    min_width: f32,
    min_height: f32,
    preferred_width: f32,
    preferred_height: f32,
) -> AppConfig<'_> {
    AppConfig {
        title,
        content_interactive: false,
        window_type: WindowType::Widget,
        min_width,
        min_height,
        preferred_width,
        preferred_height,
    }
}

/// Configuration for an application window
struct AppConfig<'a> {
    title: &'a str,
    content_interactive: bool,
    window_type: WindowType,
    min_width: f32,
    min_height: f32,
    /// Preferred window width (used for initial sizing)
    preferred_width: f32,
    /// Preferred window height (used for initial sizing)
    preferred_height: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::WindowState;

    fn create_test_engine() -> DesktopEngine {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);
        engine
    }

    #[test]
    fn test_create_window_with_position() {
        let mut engine = create_test_engine();

        let id = engine.create_window(WindowConfig {
            title: "Test".to_string(),
            position: Some(Vec2::new(200.0, 150.0)),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        let window = engine.windows.get(id).unwrap();
        assert!((window.position.x - 200.0).abs() < 0.001);
        assert!((window.position.y - 150.0).abs() < 0.001);
    }

    #[test]
    fn test_create_window_auto_cascade() {
        let mut engine = create_test_engine();

        // Create first window without position
        let id1 = engine.create_window(WindowConfig {
            title: "Window 1".to_string(),
            position: None,
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        let pos1 = engine.windows.get(id1).unwrap().position;

        // Create second window without position - should cascade
        let id2 = engine.create_window(WindowConfig {
            title: "Window 2".to_string(),
            position: None,
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        let pos2 = engine.windows.get(id2).unwrap().position;

        // Second window should be offset from first
        assert!(pos2.x > pos1.x);
        assert!(pos2.y > pos1.y);
    }

    #[test]
    fn test_close_window_removes_from_desktop() {
        let mut engine = create_test_engine();

        let id = engine.create_window(WindowConfig {
            title: "Test".to_string(),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        assert!(engine.desktops.active_desktop().contains_window(id));

        engine.close_window(id);

        assert!(!engine.desktops.active_desktop().contains_window(id));
        assert!(engine.windows.get(id).is_none());
    }

    #[test]
    fn test_focus_window_saves_camera() {
        let mut engine = create_test_engine();

        let id1 = engine.create_window(WindowConfig {
            title: "Window 1".to_string(),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        let id2 = engine.create_window(WindowConfig {
            title: "Window 2".to_string(),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        // Set viewport to specific position
        engine.viewport.center = Vec2::new(500.0, 500.0);
        engine.viewport.zoom = 1.5;

        // Focus window 1 (which will save camera for id2)
        engine.focus_window(id1);

        // Verify camera was saved for the previously focused window
        let saved = engine.window_cameras.get(&id2);
        assert!(saved.is_some());
    }

    #[test]
    fn test_move_window() {
        let mut engine = create_test_engine();

        let id = engine.create_window(WindowConfig {
            title: "Test".to_string(),
            position: Some(Vec2::new(100.0, 100.0)),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        engine.move_window(id, 300.0, 400.0);

        let window = engine.windows.get(id).unwrap();
        assert!((window.position.x - 300.0).abs() < 0.001);
        assert!((window.position.y - 400.0).abs() < 0.001);
    }

    #[test]
    fn test_resize_window() {
        let mut engine = create_test_engine();

        let id = engine.create_window(WindowConfig {
            title: "Test".to_string(),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        engine.resize_window(id, 1000.0, 800.0);

        let window = engine.windows.get(id).unwrap();
        assert!((window.size.width - 1000.0).abs() < 0.001);
        assert!((window.size.height - 800.0).abs() < 0.001);
    }

    #[test]
    fn test_minimize_window() {
        let mut engine = create_test_engine();

        let id = engine.create_window(WindowConfig {
            title: "Test".to_string(),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        engine.minimize_window(id);

        let window = engine.windows.get(id).unwrap();
        assert_eq!(window.state, WindowState::Minimized);
    }

    #[test]
    fn test_maximize_window() {
        let mut engine = create_test_engine();

        let id = engine.create_window(WindowConfig {
            title: "Test".to_string(),
            position: Some(Vec2::new(100.0, 100.0)),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        engine.maximize_window(id);

        let window = engine.windows.get(id).unwrap();
        assert_eq!(window.state, WindowState::Maximized);
        // Size should be approximately screen size minus taskbar
        assert!(window.size.width > 800.0);
    }

    #[test]
    fn test_restore_window() {
        let mut engine = create_test_engine();

        let id = engine.create_window(WindowConfig {
            title: "Test".to_string(),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        engine.minimize_window(id);
        assert_eq!(
            engine.windows.get(id).unwrap().state,
            WindowState::Minimized
        );

        engine.restore_window(id);
        assert_eq!(engine.windows.get(id).unwrap().state, WindowState::Normal);
    }

    #[test]
    fn test_create_desktop() {
        let mut engine = create_test_engine();

        assert_eq!(engine.desktops.count(), 1);

        engine.create_desktop("Second Desktop");

        assert_eq!(engine.desktops.count(), 2);
    }

    #[test]
    fn test_switch_desktop() {
        let mut engine = create_test_engine();
        engine.create_desktop("Second");

        assert_eq!(engine.desktops.active_index(), 0);

        engine.switch_desktop(1, 0.0);

        assert_eq!(engine.desktops.active_index(), 1);
    }

    #[test]
    fn test_switch_desktop_saves_camera() {
        let mut engine = create_test_engine();
        engine.create_desktop("Second");

        // Set specific camera position
        engine.viewport.center = Vec2::new(500.0, 500.0);
        engine.viewport.zoom = 2.0;

        engine.switch_desktop(1, 0.0);

        // Camera should have been saved for desktop 0
        let saved = engine.desktops.get_desktop_camera(0).unwrap();
        assert!((saved.center.x - 500.0).abs() < 0.001);
        assert!((saved.zoom - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_cannot_switch_desktop_during_void_transition() {
        let mut engine = create_test_engine();
        engine.create_desktop("Second");

        // Enter void to start a transition
        engine.enter_void(0.0);
        assert!(engine.is_crossfading());

        // Try to switch desktop
        engine.switch_desktop(1, 100.0);

        // Should still be on desktop 0 (switch blocked)
        assert_eq!(engine.desktops.active_index(), 0);
    }

    #[test]
    fn test_launch_app_terminal() {
        let mut engine = create_test_engine();

        let id = engine.launch_app("terminal");

        let window = engine.windows.get(id).unwrap();
        assert_eq!(window.title, "Terminal");
        assert_eq!(window.app_id, "terminal");
    }

    #[test]
    fn test_launch_app_unknown() {
        let mut engine = create_test_engine();

        let id = engine.launch_app("my-custom-app");

        let window = engine.windows.get(id).unwrap();
        assert_eq!(window.title, "my-custom-app");
        assert_eq!(window.app_id, "my-custom-app");
    }

    #[test]
    fn test_launch_app_clock_is_widget() {
        use crate::window::WindowType;
        let mut engine = create_test_engine();

        let id = engine.launch_app("clock");

        let window = engine.windows.get(id).unwrap();
        assert_eq!(window.title, "Clock");
        assert_eq!(window.app_id, "clock");
        assert_eq!(window.window_type, WindowType::Widget);
        // Widget windows should be smaller
        assert!(window.size.width < 400.0);
        assert!(window.size.height < 400.0);
    }

    #[test]
    fn test_launch_app_calculator_is_widget() {
        use crate::window::WindowType;
        let mut engine = create_test_engine();

        let id = engine.launch_app("calculator");

        let window = engine.windows.get(id).unwrap();
        assert_eq!(window.title, "Calculator");
        assert_eq!(window.app_id, "calculator");
        assert_eq!(window.window_type, WindowType::Widget);
        // Calculator widget should fit its content (300x420)
        assert!(window.size.width <= 400.0);
        assert!(window.size.height <= 500.0);
    }

    #[test]
    fn test_terminal_title_includes_pid() {
        let mut engine = create_test_engine();

        let id = engine.launch_app("terminal");

        // Before setting process ID, title is just "Terminal"
        let window = engine.windows.get(id).unwrap();
        assert_eq!(window.title, "Terminal");

        // After setting process ID, title includes PID
        engine.set_window_process_id(id, 42);
        let window = engine.windows.get(id).unwrap();
        assert_eq!(window.title, "Terminal p42");
    }

    #[test]
    fn test_set_desktop_background() {
        let mut engine = create_test_engine();

        engine.set_desktop_background(0, "mist");

        let bg = engine.desktops.get_desktop_background(0).unwrap();
        assert_eq!(bg, "mist");
    }
}
