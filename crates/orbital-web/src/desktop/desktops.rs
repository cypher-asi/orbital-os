//! Desktop Manager for the desktop environment
//!
//! Manages desktops as isolated infinite canvases.
//! Each desktop has its own background, windows, and camera state.
//!
//! ## Conceptual Model
//!
//! - Each desktop is its own infinite canvas with (0,0) at center
//! - Windows exist within a single desktop in desktop-local coordinates
//! - Each desktop remembers its camera position and zoom
//! - Desktops are arranged horizontally in the void as a centered strip
//!
//! ## Layout (in Void view)
//!
//! Desktops are laid out horizontally with gaps between them.
//! This layout is only visible when in the Void view mode.

use super::types::{Camera, Rect, Size, Vec2};
use super::windows::WindowId;
use crate::background::BackgroundType;
use serde::{Deserialize, Serialize};

/// Unique desktop identifier
pub type DesktopId = u32;

/// A desktop - an isolated infinite canvas
///
/// Each desktop is a self-contained environment with:
/// - Its own background
/// - Its own set of windows (in desktop-local coordinates)
/// - Its own camera state (center and zoom)
///
/// The `bounds` field defines where this desktop appears in the void view,
/// not a limit on the desktop's internal size (which is infinite).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Desktop {
    /// Unique identifier
    pub id: DesktopId,
    /// Human-readable name
    pub name: String,
    /// Position in void view (where this desktop appears when zoomed out)
    /// The center of bounds is the desktop's "tile" position in void space
    pub bounds: Rect,
    /// Windows in this desktop (stored by ID, positions are in desktop-local coords)
    #[serde(skip)]
    pub windows: Vec<WindowId>,
    /// Background type for this desktop
    pub background: BackgroundType,
    /// Camera state (position and zoom within this desktop)
    #[serde(default)]
    pub camera: Camera,
}

impl Desktop {
    /// Create a new desktop at the given bounds
    pub fn new(id: DesktopId, name: String, bounds: Rect) -> Self {
        Self {
            id,
            name,
            bounds,
            windows: Vec::new(),
            background: BackgroundType::default(),
            camera: Camera::new(),
        }
    }

    /// Create a new desktop with a specific background
    pub fn with_background(
        id: DesktopId,
        name: String,
        bounds: Rect,
        background: BackgroundType,
    ) -> Self {
        Self {
            id,
            name,
            bounds,
            windows: Vec::new(),
            background,
            camera: Camera::new(),
        }
    }

    /// Get the background type
    pub fn background(&self) -> BackgroundType {
        self.background
    }

    /// Set the background type
    pub fn set_background(&mut self, background: BackgroundType) {
        self.background = background;
    }

    /// Get the camera state for this desktop
    pub fn camera(&self) -> Camera {
        self.camera
    }

    /// Set the camera state for this desktop
    pub fn set_camera(&mut self, camera: Camera) {
        self.camera = camera;
    }

    /// Save camera state (called when leaving this desktop)
    pub fn save_camera(&mut self, center: Vec2, zoom: f32) {
        self.camera = Camera::at(center, zoom);
    }

    /// Reset camera to default (centered on desktop origin, zoom 1.0)
    pub fn reset_camera(&mut self) {
        self.camera = Camera::new();
    }

    /// Add a window to this desktop
    pub fn add_window(&mut self, window_id: WindowId) {
        if !self.windows.contains(&window_id) {
            self.windows.push(window_id);
        }
    }

    /// Remove a window from this desktop
    pub fn remove_window(&mut self, window_id: WindowId) {
        self.windows.retain(|&id| id != window_id);
    }

    /// Check if desktop contains a window
    pub fn contains_window(&self, window_id: WindowId) -> bool {
        self.windows.contains(&window_id)
    }

    /// Get the center position of this desktop in void space
    pub fn void_center(&self) -> Vec2 {
        self.bounds.center()
    }
}

// =============================================================================
// VoidState - Camera state for the void (meta-layer)
// =============================================================================

/// State for the Void layer where all desktops appear as tiles
///
/// The void is a separate coordinate space where desktops are arranged
/// horizontally in a centered strip. It has its own camera state that's
/// independent of any desktop's internal camera.
///
/// Void camera constraints:
/// - Zoom is limited (can't zoom in past 1.0 where tiles are screen-sized)
/// - Pan is constrained to keep desktop tiles visible
#[derive(Clone, Debug)]
pub struct VoidState {
    /// Camera for the void view (center, zoom)
    pub camera: Camera,
    /// Screen size for constraint calculations
    screen_size: Size,
}

impl Default for VoidState {
    fn default() -> Self {
        Self::new(Size::new(1920.0, 1080.0))
    }
}

impl VoidState {
    /// Create a new void state with the given screen size
    pub fn new(screen_size: Size) -> Self {
        Self {
            camera: Camera::new(),
            screen_size,
        }
    }

    /// Update screen size
    pub fn set_screen_size(&mut self, size: Size) {
        self.screen_size = size;
    }

    /// Get the camera
    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    /// Get mutable camera reference
    pub fn camera_mut(&mut self) -> &mut Camera {
        &mut self.camera
    }

    /// Set the camera state
    pub fn set_camera(&mut self, camera: Camera) {
        self.camera = camera;
    }

    /// Center the void camera on a specific position
    pub fn center_on(&mut self, position: Vec2) {
        self.camera.center = position;
    }

    /// Zoom the void camera with constraints (min: 0.1, max: 1.0)
    /// Returns true if zoom changed
    pub fn zoom_at(&mut self, factor: f32, anchor: Vec2) -> bool {
        let old_zoom = self.camera.zoom;
        self.camera
            .zoom_at_clamped(factor, anchor, self.screen_size, 0.1, 1.0);
        (self.camera.zoom - old_zoom).abs() > 0.0001
    }

    /// Pan the void camera
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.camera.pan(dx, dy);
    }

    /// Constrain the void camera to keep desktops visible
    ///
    /// Call this after panning to ensure at least some desktops remain on screen.
    pub fn constrain_to_desktops(&mut self, desktop_bounds: &[Rect], padding: f32) {
        if desktop_bounds.is_empty() {
            return;
        }

        // Calculate bounding box of all desktops
        let min_x = desktop_bounds
            .iter()
            .map(|b| b.x)
            .fold(f32::INFINITY, f32::min);
        let max_x = desktop_bounds
            .iter()
            .map(|b| b.x + b.width)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = desktop_bounds
            .iter()
            .map(|b| b.y)
            .fold(f32::INFINITY, f32::min);
        let max_y = desktop_bounds
            .iter()
            .map(|b| b.y + b.height)
            .fold(f32::NEG_INFINITY, f32::max);

        // Add padding
        let bounds = Rect::new(
            min_x - padding,
            min_y - padding,
            (max_x - min_x) + padding * 2.0,
            (max_y - min_y) + padding * 2.0,
        );

        // Calculate visible area
        let half_visible_w = self.screen_size.width / self.camera.zoom / 2.0;
        let half_visible_h = self.screen_size.height / self.camera.zoom / 2.0;

        // Clamp center to keep some desktops visible
        let center_min_x = bounds.x + half_visible_w;
        let center_max_x = bounds.x + bounds.width - half_visible_w;
        let center_min_y = bounds.y + half_visible_h;
        let center_max_y = bounds.y + bounds.height - half_visible_h;

        if center_min_x <= center_max_x {
            self.camera.center.x = self.camera.center.x.clamp(center_min_x, center_max_x);
        } else {
            self.camera.center.x = bounds.x + bounds.width / 2.0;
        }

        if center_min_y <= center_max_y {
            self.camera.center.y = self.camera.center.y.clamp(center_min_y, center_max_y);
        } else {
            self.camera.center.y = bounds.y + bounds.height / 2.0;
        }
    }

    /// Calculate the center of all desktops (for initial void view)
    pub fn calculate_void_center(desktop_bounds: &[Rect]) -> Vec2 {
        if desktop_bounds.is_empty() {
            return Vec2::ZERO;
        }

        let min_x = desktop_bounds
            .iter()
            .map(|b| b.x)
            .fold(f32::INFINITY, f32::min);
        let max_x = desktop_bounds
            .iter()
            .map(|b| b.x + b.width)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = desktop_bounds
            .iter()
            .map(|b| b.y)
            .fold(f32::INFINITY, f32::min);
        let max_y = desktop_bounds
            .iter()
            .map(|b| b.y + b.height)
            .fold(f32::NEG_INFINITY, f32::max);

        Vec2::new((min_x + max_x) / 2.0, (min_y + max_y) / 2.0)
    }

    /// Calculate zoom level to fit all desktops in view
    pub fn calculate_fit_zoom(desktop_bounds: &[Rect], screen_size: Size) -> f32 {
        if desktop_bounds.is_empty() {
            return 0.4;
        }

        let min_x = desktop_bounds
            .iter()
            .map(|b| b.x)
            .fold(f32::INFINITY, f32::min);
        let max_x = desktop_bounds
            .iter()
            .map(|b| b.x + b.width)
            .fold(f32::NEG_INFINITY, f32::max);

        let total_width = max_x - min_x;
        if total_width <= 0.0 {
            return 0.4;
        }

        // Fit to screen with some padding
        let fit_zoom = screen_size.width / (total_width * 1.2);
        fit_zoom.clamp(0.15, 0.5)
    }
}

// =============================================================================
// DesktopManager - Manages all desktops
// =============================================================================

/// Desktop manager for infinite canvas regions
pub struct DesktopManager {
    /// All desktops
    desktops: Vec<Desktop>,
    /// Currently active desktop index
    active: usize,
    /// Next desktop ID
    next_id: DesktopId,
    /// Standard desktop size (matches screen size)
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

        // Calculate bounds - desktops are arranged horizontally, centered on y-axis
        let index = self.desktops.len();
        let x = index as f32 * (self.desktop_size.width + self.desktop_gap);
        // Center the bounds so that (0,0) is at the center of the first desktop
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

        // If this is the first desktop, set it as active
        if self.desktops.len() == 1 {
            self.active = 0;
        }

        id
    }

    /// Switch to desktop by index
    /// Returns true if switched, false if index out of bounds
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
    pub fn active_desktop(&self) -> &Desktop {
        &self.desktops[self.active]
    }

    /// Get the currently active desktop mutably
    pub fn active_desktop_mut(&mut self) -> &mut Desktop {
        &mut self.desktops[self.active]
    }

    /// Get the active desktop index
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Get all desktops
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
    ///
    /// This recalculates bounds for all desktops to maintain consistent layout.
    /// Windows within desktops are NOT moved - they keep their positions relative
    /// to the desktop origin.
    pub fn set_desktop_size(&mut self, size: Size) {
        // Only recalculate if size actually changed
        if self.desktop_size.width == size.width && self.desktop_size.height == size.height {
            return;
        }

        self.desktop_size = size;

        // Recalculate bounds for all existing desktops
        let half_w = size.width / 2.0;
        let half_h = size.height / 2.0;

        for (index, desktop) in self.desktops.iter_mut().enumerate() {
            let x = index as f32 * (size.width + self.desktop_gap);
            desktop.bounds = Rect::new(x - half_w, -half_h, size.width, size.height);
        }
    }

    /// Get the current desktop size
    pub fn desktop_size(&self) -> Size {
        self.desktop_size
    }

    /// Get the desktop gap
    pub fn desktop_gap(&self) -> f32 {
        self.desktop_gap
    }

    /// Get the number of desktops
    pub fn count(&self) -> usize {
        self.desktops.len()
    }

    /// Delete a desktop by index (cannot delete if it's the last one)
    pub fn delete(&mut self, index: usize) -> bool {
        if self.desktops.len() <= 1 || index >= self.desktops.len() {
            return false;
        }

        self.desktops.remove(index);

        // Adjust active index if needed
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

    /// Get the background of a desktop by index
    pub fn get_background(&self, index: usize) -> Option<BackgroundType> {
        self.desktops.get(index).map(|d| d.background)
    }

    /// Set the background of a desktop by index
    pub fn set_background(&mut self, index: usize, background: BackgroundType) {
        if let Some(desktop) = self.desktops.get_mut(index) {
            desktop.set_background(background);
        }
    }

    /// Get the background of the active desktop
    pub fn active_background(&self) -> BackgroundType {
        self.desktops
            .get(self.active)
            .map(|d| d.background)
            .unwrap_or_default()
    }

    /// Set the background of the active desktop
    pub fn set_active_background(&mut self, background: BackgroundType) {
        if let Some(desktop) = self.desktops.get_mut(self.active) {
            desktop.set_background(background);
        }
    }

    /// Export desktops for persistence (excludes transient window data)
    pub fn export_for_persistence(&self) -> Vec<&Desktop> {
        self.desktops.iter().collect()
    }

    /// Import desktop settings from persistence
    /// Updates backgrounds, names, and camera state for existing desktops
    pub fn import_from_persistence(&mut self, persisted: &[PersistedDesktop]) {
        for p in persisted {
            if let Some(d) = self.desktops.iter_mut().find(|d| d.id == p.id) {
                d.name = p.name.clone();
                d.background = p.background;
                d.camera = p.camera;
            }
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

    // =========================================================================
    // Backward compatibility aliases (deprecated)
    // =========================================================================

    /// Alias for `desktops()` - deprecated, use `desktops()` instead
    #[deprecated(note = "Use desktops() instead")]
    pub fn workspaces(&self) -> &[Desktop] {
        self.desktops()
    }

    /// Alias for `active_desktop()` - deprecated, use `active_desktop()` instead
    #[deprecated(note = "Use active_desktop() instead")]
    pub fn active_workspace(&self) -> &Desktop {
        self.active_desktop()
    }

    /// Alias for `active_desktop_mut()` - deprecated
    #[deprecated(note = "Use active_desktop_mut() instead")]
    pub fn active_workspace_mut(&mut self) -> &mut Desktop {
        self.active_desktop_mut()
    }

    /// Alias for `get_desktop_center()` - deprecated
    #[deprecated(note = "Use get_desktop_center() instead")]
    pub fn get_workspace_center(&self, index: usize) -> Option<Vec2> {
        self.get_desktop_center(index)
    }

    /// Alias for `add_window_to_desktop()` - deprecated
    #[deprecated(note = "Use add_window_to_desktop() instead")]
    pub fn add_window_to_workspace(&mut self, workspace_index: usize, window_id: WindowId) {
        self.add_window_to_desktop(workspace_index, window_id)
    }

    /// Alias for `desktop_containing()` - deprecated
    #[deprecated(note = "Use desktop_containing() instead")]
    pub fn workspace_containing(&self, window_id: WindowId) -> Option<&Desktop> {
        self.desktop_containing(window_id)
    }

    /// Alias for `set_desktop_size()` - deprecated
    #[deprecated(note = "Use set_desktop_size() instead")]
    pub fn set_workspace_size(&mut self, size: Size) {
        self.set_desktop_size(size)
    }

    /// Alias for `desktop_size()` - deprecated
    #[deprecated(note = "Use desktop_size() instead")]
    pub fn workspace_size(&self) -> Size {
        self.desktop_size()
    }

    /// Alias for `desktop_gap()` - deprecated
    #[deprecated(note = "Use desktop_gap() instead")]
    pub fn workspace_gap(&self) -> f32 {
        self.desktop_gap()
    }

    /// Alias for `save_desktop_camera()` - deprecated
    #[deprecated(note = "Use save_desktop_camera() instead")]
    pub fn save_workspace_viewport(&mut self, index: usize, center: Vec2, zoom: f32) {
        self.save_desktop_camera(index, center, zoom)
    }

    /// Alias for `get_desktop_camera()` - deprecated, returns Camera wrapped in compatibility type
    #[deprecated(note = "Use get_desktop_camera() instead")]
    pub fn get_workspace_viewport(&self, index: usize) -> Option<WorkspaceViewport> {
        self.get_desktop_camera(index).map(WorkspaceViewport::from_camera)
    }

    /// Alias for `save_active_camera()` - deprecated
    #[deprecated(note = "Use save_active_camera() instead")]
    pub fn save_active_viewport(&mut self, center: Vec2, zoom: f32) {
        self.save_active_camera(center, zoom)
    }

    /// Alias for `get_active_camera()` - deprecated
    #[deprecated(note = "Use get_active_camera() instead")]
    pub fn get_active_viewport(&self) -> WorkspaceViewport {
        WorkspaceViewport::from_camera_ref(&self.get_active_camera())
    }
}

// =============================================================================
// Persistence Types
// =============================================================================

/// Persisted desktop data (for localStorage)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersistedDesktop {
    pub id: DesktopId,
    pub name: String,
    pub background: BackgroundType,
    /// Saved camera position and zoom
    #[serde(default)]
    pub camera: Camera,
}

// =============================================================================
// Backward Compatibility Types (deprecated)
// =============================================================================

/// Type alias for backward compatibility
#[deprecated(note = "Use DesktopId instead")]
pub type WorkspaceId = DesktopId;

/// Type alias for backward compatibility
#[deprecated(note = "Use Desktop instead")]
pub type Workspace = Desktop;

/// Type alias for backward compatibility
#[deprecated(note = "Use DesktopManager instead")]
pub type WorkspaceManager = DesktopManager;

/// Type alias for backward compatibility
#[deprecated(note = "Use PersistedDesktop instead")]
pub type PersistedWorkspace = PersistedDesktop;

/// Viewport state for backward compatibility - wraps Camera
///
/// This type is deprecated. Use `Camera` directly instead.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[deprecated(note = "Use Camera directly instead")]
pub struct WorkspaceViewport {
    pub center: Vec2,
    pub zoom: f32,
}

#[allow(deprecated)]
impl WorkspaceViewport {
    pub fn new() -> Self {
        Self {
            center: Vec2::ZERO,
            zoom: 1.0,
        }
    }

    pub fn at(center: Vec2, zoom: f32) -> Self {
        Self { center, zoom }
    }

    pub fn to_camera(&self) -> Camera {
        Camera::at(self.center, self.zoom)
    }

    pub fn from_camera(camera: Camera) -> Self {
        Self {
            center: camera.center,
            zoom: camera.zoom,
        }
    }

    /// Create from Camera reference
    pub fn from_camera_ref(camera: &Camera) -> Self {
        Self {
            center: camera.center,
            zoom: camera.zoom,
        }
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

        // First desktop centered at origin
        let center1 = d1.bounds.center();
        assert!((center1.x - 0.0).abs() < 0.001);
        assert!((center1.y - 0.0).abs() < 0.001);

        // Second desktop offset by desktop_size + gap
        let center2 = d2.bounds.center();
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

        assert!(!dm.switch_to(10)); // Out of bounds
        assert_eq!(dm.active_index(), 2); // Should not change
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

        // Find desktop containing window
        let d = dm.desktop_containing(100).unwrap();
        assert_eq!(d.name, "Desktop 1");

        // Remove window from all desktops
        dm.remove_window(100);
        assert_eq!(dm.desktops()[0].windows.len(), 1);
    }

    #[test]
    fn test_desktop_bounds_update_on_resize() {
        let mut dm = DesktopManager::new();

        // Create desktops with default size (1920x1080)
        dm.create("Desktop 1");
        dm.create("Desktop 2");

        // Verify initial positions
        let center1 = dm.desktops()[0].bounds.center();
        let center2 = dm.desktops()[1].bounds.center();
        assert!((center1.x - 0.0).abs() < 0.001);
        assert!((center2.x - 2020.0).abs() < 0.001); // 1920 + 100 gap

        // Resize to larger screen
        dm.set_desktop_size(Size::new(2560.0, 1440.0));

        // Verify bounds were updated
        let new_center1 = dm.desktops()[0].bounds.center();
        let new_center2 = dm.desktops()[1].bounds.center();
        assert!((new_center1.x - 0.0).abs() < 0.001);
        assert!((new_center2.x - 2660.0).abs() < 0.001); // 2560 + 100 gap

        // Verify desktop_size() getter returns new size
        assert!((dm.desktop_size().width - 2560.0).abs() < 0.001);
        assert!((dm.desktop_size().height - 1440.0).abs() < 0.001);
    }

    #[test]
    fn test_desktop_camera() {
        let mut dm = DesktopManager::new();
        dm.create("Desktop 1");

        // Default camera is at origin with zoom 1.0
        let camera = dm.get_desktop_camera(0).unwrap();
        assert!((camera.center.x - 0.0).abs() < 0.001);
        assert!((camera.zoom - 1.0).abs() < 0.001);

        // Save new camera state
        dm.save_desktop_camera(0, Vec2::new(100.0, 200.0), 2.0);

        let camera = dm.get_desktop_camera(0).unwrap();
        assert!((camera.center.x - 100.0).abs() < 0.001);
        assert!((camera.center.y - 200.0).abs() < 0.001);
        assert!((camera.zoom - 2.0).abs() < 0.001);
    }
}
