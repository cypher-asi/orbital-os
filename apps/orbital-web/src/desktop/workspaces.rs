//! Workspace Manager for the desktop environment
//!
//! Manages workspaces as isolated infinite canvases.
//! Each workspace has its own background, windows, and viewport state.
//!
//! ## Conceptual Model
//!
//! - Each workspace is its own infinite canvas
//! - Windows exist within a single workspace
//! - Each workspace remembers its viewport position and zoom
//! - Workspaces are arranged horizontally for void view
//!
//! ## Layout (in Void view)
//!
//! Workspaces are laid out horizontally with gaps between them.
//! This layout is only visible when in the Void view mode.

use super::types::{Rect, Size, Vec2};
use super::windows::WindowId;
use crate::background::BackgroundType;
use serde::{Deserialize, Serialize};

/// Unique workspace identifier
pub type WorkspaceId = u32;

/// Viewport state for a workspace
///
/// Each workspace remembers where the user was looking and at what zoom level.
/// This state is restored when returning to the workspace.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct WorkspaceViewport {
    /// Center position within the workspace's coordinate space
    /// Relative to the workspace's bounds center (0,0 = workspace center)
    pub center: Vec2,
    /// Zoom level (1.0 = normal, >1.0 = zoomed in)
    pub zoom: f32,
}

impl WorkspaceViewport {
    /// Create a new workspace viewport at default position
    pub fn new() -> Self {
        Self {
            center: Vec2::ZERO,
            zoom: 1.0,
        }
    }
    
    /// Create a viewport at a specific position and zoom
    pub fn at(center: Vec2, zoom: f32) -> Self {
        Self { center, zoom }
    }
}

/// A workspace - an isolated infinite canvas
///
/// Each workspace is a self-contained environment with:
/// - Its own background
/// - Its own set of windows
/// - Its own viewport state (position and zoom)
///
/// The `bounds` field defines where this workspace appears in the void view,
/// not a limit on the workspace's internal size (which is infinite).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Workspace {
    /// Unique identifier
    pub id: WorkspaceId,
    /// Human-readable name
    pub name: String,
    /// Position in void view (where this workspace appears when zoomed out)
    pub bounds: Rect,
    /// Windows in this workspace
    #[serde(skip)]
    pub windows: Vec<WindowId>,
    /// Background type for this workspace
    pub background: BackgroundType,
    /// Saved viewport state (position and zoom within this workspace)
    #[serde(default)]
    pub viewport: WorkspaceViewport,
}

impl Workspace {
    /// Create a new workspace at the given bounds
    pub fn new(id: WorkspaceId, name: String, bounds: Rect) -> Self {
        Self {
            id,
            name,
            bounds,
            windows: Vec::new(),
            background: BackgroundType::default(),
            viewport: WorkspaceViewport::new(),
        }
    }

    /// Create a new workspace with a specific background
    pub fn with_background(id: WorkspaceId, name: String, bounds: Rect, background: BackgroundType) -> Self {
        Self {
            id,
            name,
            bounds,
            windows: Vec::new(),
            background,
            viewport: WorkspaceViewport::new(),
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

    /// Get the saved viewport state
    pub fn viewport(&self) -> WorkspaceViewport {
        self.viewport
    }

    /// Save viewport state (called when leaving this workspace)
    pub fn save_viewport(&mut self, center: Vec2, zoom: f32) {
        self.viewport = WorkspaceViewport::at(center, zoom);
    }

    /// Reset viewport to default (centered, zoom 1.0)
    pub fn reset_viewport(&mut self) {
        self.viewport = WorkspaceViewport::new();
    }

    /// Add a window to this workspace
    pub fn add_window(&mut self, window_id: WindowId) {
        if !self.windows.contains(&window_id) {
            self.windows.push(window_id);
        }
    }

    /// Remove a window from this workspace
    pub fn remove_window(&mut self, window_id: WindowId) {
        self.windows.retain(|&id| id != window_id);
    }

    /// Check if workspace contains a window
    pub fn contains_window(&self, window_id: WindowId) -> bool {
        self.windows.contains(&window_id)
    }
}

/// Workspace manager for infinite canvas regions
pub struct WorkspaceManager {
    /// All workspaces
    workspaces: Vec<Workspace>,
    /// Currently active workspace index
    active: usize,
    /// Next workspace ID
    next_id: WorkspaceId,
    /// Standard workspace size
    workspace_size: Size,
    /// Gap between workspaces
    workspace_gap: f32,
}

impl Default for WorkspaceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceManager {
    /// Create a new workspace manager
    pub fn new() -> Self {
        Self {
            workspaces: Vec::new(),
            active: 0,
            next_id: 1,
            workspace_size: Size::new(1920.0, 1080.0),
            workspace_gap: 100.0,
        }
    }

    /// Create a new workspace
    pub fn create(&mut self, name: &str) -> WorkspaceId {
        let id = self.next_id;
        self.next_id += 1;

        // Calculate bounds - workspaces are arranged horizontally, centered on y-axis
        let index = self.workspaces.len();
        let x = index as f32 * (self.workspace_size.width + self.workspace_gap);
        // Center the bounds so that (0,0) is at the center of the first workspace
        let half_w = self.workspace_size.width / 2.0;
        let half_h = self.workspace_size.height / 2.0;
        let bounds = Rect::new(x - half_w, -half_h, self.workspace_size.width, self.workspace_size.height);

        let workspace = Workspace::new(id, name.to_string(), bounds);
        self.workspaces.push(workspace);

        // If this is the first workspace, set it as active
        if self.workspaces.len() == 1 {
            self.active = 0;
        }

        id
    }

    /// Switch to workspace by index
    /// Returns true if switched, false if index out of bounds
    pub fn switch_to(&mut self, index: usize) -> bool {
        if index < self.workspaces.len() {
            self.active = index;
            true
        } else {
            false
        }
    }

    /// Get the center position of a workspace
    pub fn get_workspace_center(&self, index: usize) -> Option<Vec2> {
        self.workspaces.get(index).map(|ws| ws.bounds.center())
    }

    /// Get the currently active workspace
    pub fn active_workspace(&self) -> &Workspace {
        &self.workspaces[self.active]
    }

    /// Get the currently active workspace mutably
    pub fn active_workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.active]
    }

    /// Get the active workspace index
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Get all workspaces
    pub fn workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }

    /// Get a workspace by ID
    pub fn get(&self, id: WorkspaceId) -> Option<&Workspace> {
        self.workspaces.iter().find(|ws| ws.id == id)
    }

    /// Get a workspace by ID mutably
    pub fn get_mut(&mut self, id: WorkspaceId) -> Option<&mut Workspace> {
        self.workspaces.iter_mut().find(|ws| ws.id == id)
    }

    /// Get workspace index by ID
    pub fn index_of(&self, id: WorkspaceId) -> Option<usize> {
        self.workspaces.iter().position(|ws| ws.id == id)
    }

    /// Add a window to a workspace
    pub fn add_window_to_workspace(&mut self, workspace_index: usize, window_id: WindowId) {
        if let Some(workspace) = self.workspaces.get_mut(workspace_index) {
            workspace.add_window(window_id);
        }
    }

    /// Remove a window from all workspaces
    pub fn remove_window(&mut self, window_id: WindowId) {
        for workspace in &mut self.workspaces {
            workspace.remove_window(window_id);
        }
    }

    /// Find which workspace contains a window
    pub fn workspace_containing(&self, window_id: WindowId) -> Option<&Workspace> {
        self.workspaces.iter().find(|ws| ws.contains_window(window_id))
    }

    /// Set workspace size and update all existing workspace bounds
    /// 
    /// This recalculates bounds for all workspaces to maintain consistent layout.
    /// Windows within workspaces are NOT moved - they keep their positions relative
    /// to the workspace origin.
    pub fn set_workspace_size(&mut self, size: Size) {
        // Only recalculate if size actually changed
        if self.workspace_size.width == size.width && self.workspace_size.height == size.height {
            return;
        }
        
        self.workspace_size = size;
        
        // Recalculate bounds for all existing workspaces
        // This ensures workspace centers match what the shader expects
        let half_w = size.width / 2.0;
        let half_h = size.height / 2.0;
        
        for (index, workspace) in self.workspaces.iter_mut().enumerate() {
            let x = index as f32 * (size.width + self.workspace_gap);
            workspace.bounds = Rect::new(x - half_w, -half_h, size.width, size.height);
        }
    }
    
    /// Get the current workspace size
    pub fn workspace_size(&self) -> Size {
        self.workspace_size
    }
    
    /// Get the workspace gap
    pub fn workspace_gap(&self) -> f32 {
        self.workspace_gap
    }

    /// Get the number of workspaces
    pub fn count(&self) -> usize {
        self.workspaces.len()
    }

    /// Delete a workspace by index (cannot delete if it's the last one)
    pub fn delete(&mut self, index: usize) -> bool {
        if self.workspaces.len() <= 1 || index >= self.workspaces.len() {
            return false;
        }

        self.workspaces.remove(index);

        // Adjust active index if needed
        if self.active >= self.workspaces.len() {
            self.active = self.workspaces.len() - 1;
        }

        true
    }

    /// Rename a workspace
    pub fn rename(&mut self, index: usize, name: &str) {
        if let Some(workspace) = self.workspaces.get_mut(index) {
            workspace.name = name.to_string();
        }
    }

    /// Get the background of a workspace by index
    pub fn get_background(&self, index: usize) -> Option<BackgroundType> {
        self.workspaces.get(index).map(|ws| ws.background)
    }

    /// Set the background of a workspace by index
    pub fn set_background(&mut self, index: usize, background: BackgroundType) {
        if let Some(workspace) = self.workspaces.get_mut(index) {
            workspace.set_background(background);
        }
    }

    /// Get the background of the active workspace
    pub fn active_background(&self) -> BackgroundType {
        self.workspaces
            .get(self.active)
            .map(|ws| ws.background)
            .unwrap_or_default()
    }

    /// Set the background of the active workspace
    pub fn set_active_background(&mut self, background: BackgroundType) {
        if let Some(workspace) = self.workspaces.get_mut(self.active) {
            workspace.set_background(background);
        }
    }

    /// Export workspaces for persistence (excludes transient window data)
    pub fn export_for_persistence(&self) -> Vec<&Workspace> {
        self.workspaces.iter().collect()
    }

    /// Import workspace settings from persistence
    /// Updates backgrounds, names, and viewport state for existing workspaces
    pub fn import_from_persistence(&mut self, persisted: &[PersistedWorkspace]) {
        for p in persisted {
            if let Some(ws) = self.workspaces.iter_mut().find(|ws| ws.id == p.id) {
                ws.name = p.name.clone();
                ws.background = p.background;
                ws.viewport = p.viewport;
            }
        }
    }
    
    /// Save viewport state for a workspace
    pub fn save_workspace_viewport(&mut self, index: usize, center: Vec2, zoom: f32) {
        if let Some(ws) = self.workspaces.get_mut(index) {
            ws.save_viewport(center, zoom);
        }
    }
    
    /// Get viewport state for a workspace
    pub fn get_workspace_viewport(&self, index: usize) -> Option<WorkspaceViewport> {
        self.workspaces.get(index).map(|ws| ws.viewport)
    }
    
    /// Save viewport state for the active workspace
    pub fn save_active_viewport(&mut self, center: Vec2, zoom: f32) {
        self.save_workspace_viewport(self.active, center, zoom);
    }
    
    /// Get viewport state for the active workspace
    pub fn get_active_viewport(&self) -> WorkspaceViewport {
        self.workspaces
            .get(self.active)
            .map(|ws| ws.viewport)
            .unwrap_or_default()
    }
}

/// Persisted workspace data (for localStorage)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersistedWorkspace {
    pub id: WorkspaceId,
    pub name: String,
    pub background: BackgroundType,
    /// Saved viewport position and zoom
    #[serde(default)]
    pub viewport: WorkspaceViewport,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_creation() {
        let mut wm = WorkspaceManager::new();

        let id1 = wm.create("Workspace 1");
        let id2 = wm.create("Workspace 2");

        assert_eq!(wm.count(), 2);
        assert!(wm.get(id1).is_some());
        assert!(wm.get(id2).is_some());
    }

    #[test]
    fn test_workspace_positions() {
        let mut wm = WorkspaceManager::new();

        wm.create("Workspace 1");
        wm.create("Workspace 2");

        let ws1 = &wm.workspaces()[0];
        let ws2 = &wm.workspaces()[1];

        // First workspace centered at origin
        let center1 = ws1.bounds.center();
        assert!((center1.x - 0.0).abs() < 0.001);
        assert!((center1.y - 0.0).abs() < 0.001);

        // Second workspace offset by workspace_size + gap
        let center2 = ws2.bounds.center();
        let expected_x = wm.workspace_size.width + wm.workspace_gap;
        assert!((center2.x - expected_x).abs() < 0.001);
    }

    #[test]
    fn test_workspace_switching() {
        let mut wm = WorkspaceManager::new();

        wm.create("Workspace 1");
        wm.create("Workspace 2");
        wm.create("Workspace 3");

        assert_eq!(wm.active_index(), 0);

        assert!(wm.switch_to(2));
        assert_eq!(wm.active_index(), 2);

        assert!(!wm.switch_to(10)); // Out of bounds
        assert_eq!(wm.active_index(), 2); // Should not change
    }

    #[test]
    fn test_workspace_windows() {
        let mut wm = WorkspaceManager::new();

        wm.create("Workspace 1");
        wm.create("Workspace 2");

        wm.add_window_to_workspace(0, 100);
        wm.add_window_to_workspace(0, 101);
        wm.add_window_to_workspace(1, 200);

        assert_eq!(wm.workspaces()[0].windows.len(), 2);
        assert_eq!(wm.workspaces()[1].windows.len(), 1);

        // Find workspace containing window
        let ws = wm.workspace_containing(100).unwrap();
        assert_eq!(ws.name, "Workspace 1");

        // Remove window from all workspaces
        wm.remove_window(100);
        assert_eq!(wm.workspaces()[0].windows.len(), 1);
    }

    #[test]
    fn test_workspace_bounds_update_on_resize() {
        let mut wm = WorkspaceManager::new();

        // Create workspaces with default size (1920x1080)
        wm.create("Workspace 1");
        wm.create("Workspace 2");

        // Verify initial positions
        let center1 = wm.workspaces()[0].bounds.center();
        let center2 = wm.workspaces()[1].bounds.center();
        assert!((center1.x - 0.0).abs() < 0.001);
        assert!((center2.x - 2020.0).abs() < 0.001); // 1920 + 100 gap

        // Resize to larger screen
        wm.set_workspace_size(Size::new(2560.0, 1440.0));

        // Verify bounds were updated
        let new_center1 = wm.workspaces()[0].bounds.center();
        let new_center2 = wm.workspaces()[1].bounds.center();
        assert!((new_center1.x - 0.0).abs() < 0.001);
        assert!((new_center2.x - 2660.0).abs() < 0.001); // 2560 + 100 gap

        // Verify workspace_size() getter returns new size
        assert!((wm.workspace_size().width - 2560.0).abs() < 0.001);
        assert!((wm.workspace_size().height - 1440.0).abs() < 0.001);
    }
}
