//! Desktop manager for multiple desktops
//!
//! ## Invariants
//!
//! - At least one desktop always exists after initialization (via `DesktopEngine::init`)
//! - The `active` index is always valid (`active < desktops.len()`)
//! - Desktop IDs are globally unique and monotonically increasing
//! - Window IDs in a desktop are unique within that desktop
//!
//! ## Failure Modes
//!
//! - Operations on non-existent desktops are no-ops (return false or Option::None)
//! - Cannot delete the last remaining desktop
//! - Switching to an invalid index returns false and leaves state unchanged

use super::{Desktop, DesktopId, PersistedDesktop};
use crate::math::{Camera, Rect, Size, Vec2};
use crate::window::WindowId;

/// Desktop manager for managing multiple desktops
pub struct DesktopManager {
    /// All desktops
    desktops: Vec<Desktop>,
    /// Currently active desktop index
    active: usize,
    /// Next desktop ID
    next_id: DesktopId,
    /// Standard desktop size
    desktop_size: Size,
    /// Gap between desktops in void view
    desktop_gap: f32,
}

impl Default for DesktopManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DesktopManager {
    /// Create a new desktop manager
    pub fn new() -> Self {
        Self {
            desktops: Vec::new(),
            active: 0,
            next_id: 1,
            desktop_size: Size::new(1920.0, 1080.0),
            desktop_gap: 100.0,
        }
    }

    /// Create a new desktop
    pub fn create(&mut self, name: &str) -> DesktopId {
        let id = self.next_id;
        self.next_id += 1;

        let index = self.desktops.len();
        let x = index as f32 * (self.desktop_size.width + self.desktop_gap);
        let half_w = self.desktop_size.width / 2.0;
        let half_h = self.desktop_size.height / 2.0;
        let bounds = Rect::new(
            x - half_w,
            -half_h,
            self.desktop_size.width,
            self.desktop_size.height,
        );

        let desktop = Desktop::new(id, name.to_string(), bounds);
        self.desktops.push(desktop);

        if self.desktops.len() == 1 {
            self.active = 0;
        }

        id
    }

    /// Switch to desktop by index
    pub fn switch_to(&mut self, index: usize) -> bool {
        if index < self.desktops.len() {
            self.active = index;
            true
        } else {
            false
        }
    }

    /// Get the center position of a desktop
    pub fn get_desktop_center(&self, index: usize) -> Option<Vec2> {
        self.desktops.get(index).map(|d| d.bounds.center())
    }

    /// Get the currently active desktop
    ///
    /// # Panics
    ///
    /// Panics if no desktops exist. This should never happen after `DesktopEngine::init()`
    /// has been called, as it guarantees at least one desktop exists.
    #[inline]
    #[must_use]
    pub fn active_desktop(&self) -> &Desktop {
        debug_assert!(
            !self.desktops.is_empty(),
            "active_desktop() called with no desktops - call init() first"
        );
        debug_assert!(
            self.active < self.desktops.len(),
            "active index {} out of bounds (len: {})",
            self.active,
            self.desktops.len()
        );
        &self.desktops[self.active]
    }

    /// Get the currently active desktop mutably
    ///
    /// # Panics
    ///
    /// Panics if no desktops exist. This should never happen after `DesktopEngine::init()`
    /// has been called, as it guarantees at least one desktop exists.
    #[inline]
    pub fn active_desktop_mut(&mut self) -> &mut Desktop {
        debug_assert!(
            !self.desktops.is_empty(),
            "active_desktop_mut() called with no desktops - call init() first"
        );
        debug_assert!(
            self.active < self.desktops.len(),
            "active index {} out of bounds (len: {})",
            self.active,
            self.desktops.len()
        );
        &mut self.desktops[self.active]
    }

    /// Get the active desktop index
    #[inline]
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Get all desktops
    #[inline]
    pub fn desktops(&self) -> &[Desktop] {
        &self.desktops
    }

    /// Get a desktop by ID
    pub fn get(&self, id: DesktopId) -> Option<&Desktop> {
        self.desktops.iter().find(|d| d.id == id)
    }

    /// Get a desktop by ID mutably
    pub fn get_mut(&mut self, id: DesktopId) -> Option<&mut Desktop> {
        self.desktops.iter_mut().find(|d| d.id == id)
    }

    /// Get desktop index by ID
    pub fn index_of(&self, id: DesktopId) -> Option<usize> {
        self.desktops.iter().position(|d| d.id == id)
    }

    /// Add a window to a desktop
    pub fn add_window_to_desktop(&mut self, desktop_index: usize, window_id: WindowId) {
        if let Some(desktop) = self.desktops.get_mut(desktop_index) {
            desktop.add_window(window_id);
        }
    }

    /// Remove a window from all desktops
    pub fn remove_window(&mut self, window_id: WindowId) {
        for desktop in &mut self.desktops {
            desktop.remove_window(window_id);
        }
    }

    /// Find which desktop contains a window
    pub fn desktop_containing(&self, window_id: WindowId) -> Option<&Desktop> {
        self.desktops.iter().find(|d| d.contains_window(window_id))
    }

    /// Set desktop size and update all existing desktop bounds
    pub fn set_desktop_size(&mut self, size: Size) {
        if self.desktop_size.width == size.width && self.desktop_size.height == size.height {
            return;
        }

        self.desktop_size = size;

        let half_w = size.width / 2.0;
        let half_h = size.height / 2.0;

        for (index, desktop) in self.desktops.iter_mut().enumerate() {
            let x = index as f32 * (size.width + self.desktop_gap);
            desktop.bounds = Rect::new(x - half_w, -half_h, size.width, size.height);
        }
    }

    /// Get the current desktop size
    #[inline]
    pub fn desktop_size(&self) -> Size {
        self.desktop_size
    }

    /// Get the desktop gap
    #[inline]
    pub fn desktop_gap(&self) -> f32 {
        self.desktop_gap
    }

    /// Get the number of desktops
    #[inline]
    pub fn count(&self) -> usize {
        self.desktops.len()
    }

    /// Delete a desktop by index (cannot delete last one)
    pub fn delete(&mut self, index: usize) -> bool {
        if self.desktops.len() <= 1 || index >= self.desktops.len() {
            return false;
        }

        self.desktops.remove(index);

        if self.active >= self.desktops.len() {
            self.active = self.desktops.len() - 1;
        }

        true
    }

    /// Rename a desktop
    pub fn rename(&mut self, index: usize, name: &str) {
        if let Some(desktop) = self.desktops.get_mut(index) {
            desktop.name = name.to_string();
        }
    }

    /// Save camera state for a desktop
    pub fn save_desktop_camera(&mut self, index: usize, center: Vec2, zoom: f32) {
        if let Some(d) = self.desktops.get_mut(index) {
            d.save_camera(center, zoom);
        }
    }

    /// Get camera state for a desktop
    pub fn get_desktop_camera(&self, index: usize) -> Option<Camera> {
        self.desktops.get(index).map(|d| d.camera)
    }

    /// Save camera state for the active desktop
    pub fn save_active_camera(&mut self, center: Vec2, zoom: f32) {
        self.save_desktop_camera(self.active, center, zoom);
    }

    /// Get camera state for the active desktop
    pub fn get_active_camera(&self) -> Camera {
        self.desktops
            .get(self.active)
            .map(|d| d.camera)
            .unwrap_or_default()
    }

    /// Export desktops for persistence
    pub fn export_for_persistence(&self) -> Vec<PersistedDesktop> {
        self.desktops.iter().map(PersistedDesktop::from).collect()
    }

    /// Import desktop settings from persistence
    pub fn import_from_persistence(&mut self, persisted: &[PersistedDesktop]) {
        for p in persisted {
            if let Some(d) = self.desktops.iter_mut().find(|d| d.id == p.id) {
                d.name = p.name.clone();
                d.camera = p.camera;
                d.background = p.background.clone();
            }
        }
    }

    /// Set background for a desktop by index
    pub fn set_desktop_background(&mut self, index: usize, background: &str) {
        if let Some(desktop) = self.desktops.get_mut(index) {
            desktop.set_background(background);
        }
    }

    /// Get background for a desktop by index
    pub fn get_desktop_background(&self, index: usize) -> Option<String> {
        self.desktops.get(index).map(|d| d.background().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desktop_creation() {
        let mut dm = DesktopManager::new();
        let id1 = dm.create("Desktop 1");
        let id2 = dm.create("Desktop 2");

        assert_eq!(dm.count(), 2);
        assert!(dm.get(id1).is_some());
        assert!(dm.get(id2).is_some());
    }

    #[test]
    fn test_desktop_positions() {
        let mut dm = DesktopManager::new();
        dm.create("Desktop 1");
        dm.create("Desktop 2");

        let d1 = &dm.desktops()[0];
        let d2 = &dm.desktops()[1];

        let center1 = d1.bounds.center();
        let center2 = d2.bounds.center();

        assert!((center1.x - 0.0).abs() < 0.001);
        assert!((center1.y - 0.0).abs() < 0.001);

        let expected_x = dm.desktop_size.width + dm.desktop_gap;
        assert!((center2.x - expected_x).abs() < 0.001);
    }

    #[test]
    fn test_desktop_switching() {
        let mut dm = DesktopManager::new();
        dm.create("Desktop 1");
        dm.create("Desktop 2");
        dm.create("Desktop 3");

        assert_eq!(dm.active_index(), 0);

        assert!(dm.switch_to(2));
        assert_eq!(dm.active_index(), 2);

        assert!(!dm.switch_to(10));
        assert_eq!(dm.active_index(), 2);
    }

    #[test]
    fn test_desktop_windows() {
        let mut dm = DesktopManager::new();
        dm.create("Desktop 1");
        dm.create("Desktop 2");

        dm.add_window_to_desktop(0, 100);
        dm.add_window_to_desktop(0, 101);
        dm.add_window_to_desktop(1, 200);

        assert_eq!(dm.desktops()[0].windows.len(), 2);
        assert_eq!(dm.desktops()[1].windows.len(), 1);

        let d = dm.desktop_containing(100).unwrap();
        assert_eq!(d.name, "Desktop 1");

        dm.remove_window(100);
        assert_eq!(dm.desktops()[0].windows.len(), 1);
    }

    #[test]
    fn test_desktop_camera() {
        let mut dm = DesktopManager::new();
        dm.create("Desktop 1");

        let camera = dm.get_desktop_camera(0).unwrap();
        assert!((camera.center.x - 0.0).abs() < 0.001);
        assert!((camera.zoom - 1.0).abs() < 0.001);

        dm.save_desktop_camera(0, Vec2::new(100.0, 200.0), 2.0);

        let camera = dm.get_desktop_camera(0).unwrap();
        assert!((camera.center.x - 100.0).abs() < 0.001);
        assert!((camera.center.y - 200.0).abs() < 0.001);
        assert!((camera.zoom - 2.0).abs() < 0.001);
    }
}
