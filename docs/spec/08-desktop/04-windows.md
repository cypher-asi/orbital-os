# Window Management

**Component:** 08-desktop/04-windows  
**Status:** Specification

---

## Overview

Windows are movable, resizable rectangles in desktop world-space. The compositor renders window chrome (frame, titlebar, shadow) and manages window lifecycle, z-order, and focus.

---

## Window Structure

```rust
/// Window identifier
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WindowId(pub u64);

/// Window state
pub struct Window {
    /// Unique identifier
    pub id: WindowId,
    
    /// Parent desktop
    pub desktop: DesktopId,
    
    /// Window title
    pub title: String,
    
    /// Position in world space (top-left, high precision)
    pub position: (f64, f64),
    
    /// Size in logical pixels
    pub size: (f32, f32),
    
    /// Size constraints
    pub min_size: (f32, f32),
    pub max_size: Option<(f32, f32)>,
    
    /// Z-order (higher = on top)
    pub z_order: u32,
    
    /// Surface kind
    pub surface: SurfaceKind,
    
    /// Whether window is focused
    pub focused: bool,
    
    /// Window state
    pub state: WindowState,
    
    /// DOM mount (if ReactDom surface)
    pub mount: Option<WindowMount>,
}

/// Window state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowState {
    Normal,
    Minimized,
    Maximized,
}

/// Surface rendering kind
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SurfaceKind {
    /// Content rendered via WebGPU texture
    Gpu,
    
    /// Content rendered via React DOM mount
    ReactDom,
    
    /// Both GPU underlay and React overlay
    Hybrid,
}
```

---

## Window Manager

```rust
/// Window manager (internal)
pub struct WindowManager {
    /// All windows by ID
    windows: HashMap<WindowId, Window>,
    
    /// Windows by desktop
    by_desktop: HashMap<DesktopId, Vec<WindowId>>,
    
    /// Focus stack (most recent first)
    focus_stack: Vec<WindowId>,
    
    /// Next window ID
    next_id: u64,
    
    /// Next z-order value
    next_z: u32,
}

impl WindowManager {
    pub fn new() -> Self {
        Self {
            windows: HashMap::new(),
            by_desktop: HashMap::new(),
            focus_stack: Vec::new(),
            next_id: 1,
            next_z: 1,
        }
    }
}
```

---

## Window Lifecycle

### Creation

```rust
impl WindowManager {
    pub fn create(&mut self, desktop: DesktopId, spec: WindowSpec) -> WindowId {
        let id = WindowId(self.next_id);
        self.next_id += 1;
        
        let position = spec.position.unwrap_or_else(|| self.cascade_position(desktop));
        
        let window = Window {
            id,
            desktop,
            title: spec.title,
            position,
            size: spec.size,
            min_size: spec.min_size.unwrap_or((100.0, 100.0)),
            max_size: spec.max_size,
            z_order: self.next_z,
            surface: spec.surface,
            focused: false,
            state: WindowState::Normal,
            mount: None,
        };
        self.next_z += 1;
        
        self.windows.insert(id, window);
        self.by_desktop.entry(desktop).or_default().push(id);
        
        id
    }
    
    fn cascade_position(&self, desktop: DesktopId) -> (f64, f64) {
        let count = self.by_desktop.get(&desktop).map(|w| w.len()).unwrap_or(0);
        let offset = (count % 10) as f64 * 30.0;
        (100.0 + offset, 100.0 + offset)
    }
}
```

### Destruction

```rust
impl WindowManager {
    pub fn close(&mut self, id: WindowId) -> Option<Window> {
        let window = self.windows.remove(&id)?;
        
        // Remove from desktop list
        if let Some(windows) = self.by_desktop.get_mut(&window.desktop) {
            windows.retain(|&w| w != id);
        }
        
        // Remove from focus stack
        self.focus_stack.retain(|&w| w != id);
        
        Some(window)
    }
    
    pub fn close_all_on_desktop(&mut self, desktop: DesktopId) -> Vec<Window> {
        let ids = self.by_desktop.remove(&desktop).unwrap_or_default();
        ids.into_iter()
            .filter_map(|id| self.windows.remove(&id))
            .collect()
    }
}
```

---

## Focus Management

```rust
impl WindowManager {
    /// Focus a window (brings to front by default)
    pub fn focus(&mut self, id: WindowId, bring_to_front: bool) -> bool {
        if !self.windows.contains_key(&id) {
            return false;
        }
        
        // Unfocus previous
        if let Some(&prev) = self.focus_stack.first() {
            if let Some(window) = self.windows.get_mut(&prev) {
                window.focused = false;
            }
        }
        
        // Remove from current position
        self.focus_stack.retain(|&w| w != id);
        
        // Add to front
        self.focus_stack.insert(0, id);
        
        // Update window
        if let Some(window) = self.windows.get_mut(&id) {
            window.focused = true;
            
            if bring_to_front {
                window.z_order = self.next_z;
                self.next_z += 1;
            }
        }
        
        true
    }
    
    /// Get currently focused window
    pub fn focused(&self) -> Option<WindowId> {
        self.focus_stack.first().copied()
    }
    
    /// Unfocus all windows
    pub fn unfocus_all(&mut self) {
        for window in self.windows.values_mut() {
            window.focused = false;
        }
        self.focus_stack.clear();
    }
}
```

---

## Position and Size

```rust
impl WindowManager {
    /// Move window to new position
    pub fn move_window(&mut self, id: WindowId, position: (f64, f64)) {
        if let Some(window) = self.windows.get_mut(&id) {
            window.position = position;
        }
    }
    
    /// Resize window with constraint checking
    pub fn resize(&mut self, id: WindowId, size: (f32, f32)) {
        if let Some(window) = self.windows.get_mut(&id) {
            let width = size.0.max(window.min_size.0);
            let height = size.1.max(window.min_size.1);
            
            let width = window.max_size.map(|m| width.min(m.0)).unwrap_or(width);
            let height = window.max_size.map(|m| height.min(m.1)).unwrap_or(height);
            
            window.size = (width, height);
        }
    }
    
    /// Set explicit z-order
    pub fn set_z(&mut self, id: WindowId, z: u32) {
        if let Some(window) = self.windows.get_mut(&id) {
            window.z_order = z;
        }
    }
    
    /// Set window state
    pub fn set_state(&mut self, id: WindowId, state: WindowState) {
        if let Some(window) = self.windows.get_mut(&id) {
            window.state = state;
        }
    }
}
```

---

## Window Chrome

### Chrome Configuration

```rust
/// Window chrome (frame) configuration
pub struct ChromeConfig {
    /// Title bar height
    pub title_bar_height: f32,
    
    /// Corner radius
    pub corner_radius: f32,
    
    /// Border width
    pub border_width: f32,
    
    /// Shadow radius
    pub shadow_radius: f32,
    
    /// Shadow opacity
    pub shadow_opacity: f32,
    
    /// Resize handle size
    pub resize_handle_size: f32,
}

impl Default for ChromeConfig {
    fn default() -> Self {
        Self {
            title_bar_height: 32.0,
            corner_radius: 8.0,
            border_width: 1.0,
            shadow_radius: 16.0,
            shadow_opacity: 0.3,
            resize_handle_size: 8.0,
        }
    }
}
```

### Chrome Regions

```rust
/// Window region for hit testing
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowRegion {
    /// Title bar (drag to move)
    TitleBar,
    
    /// Content area (forward to app)
    Content,
    
    /// Close button
    CloseButton,
    
    /// Minimize button
    MinimizeButton,
    
    /// Maximize button
    MaximizeButton,
    
    /// Resize handles
    ResizeTop,
    ResizeBottom,
    ResizeLeft,
    ResizeRight,
    ResizeTopLeft,
    ResizeTopRight,
    ResizeBottomLeft,
    ResizeBottomRight,
}

impl Window {
    /// Calculate chrome rects
    pub fn chrome_rects(&self, config: &ChromeConfig) -> ChromeRects {
        let (x, y) = self.position;
        let (w, h) = self.size;
        let handle = config.resize_handle_size;
        let title_h = config.title_bar_height;
        
        ChromeRects {
            frame: WorldRect { x, y, width: w, height: h },
            title_bar: WorldRect { x, y, width: w, height: title_h },
            content: WorldRect {
                x,
                y: y + title_h as f64,
                width: w,
                height: h - title_h,
            },
            close_button: WorldRect {
                x: x + (w - 32.0) as f64,
                y,
                width: 32.0,
                height: title_h,
            },
            resize_bottom_right: WorldRect {
                x: x + (w - handle) as f64,
                y: y + (h - handle) as f64,
                width: handle,
                height: handle,
            },
            // ... other regions
        }
    }
    
    /// Hit test a world position
    pub fn region_at(
        &self,
        world: (f64, f64),
        config: &ChromeConfig,
    ) -> Option<WindowRegion> {
        let rects = self.chrome_rects(config);
        
        // Check from most specific to least specific
        if rects.close_button.contains(world) {
            return Some(WindowRegion::CloseButton);
        }
        if rects.resize_bottom_right.contains(world) {
            return Some(WindowRegion::ResizeBottomRight);
        }
        if rects.title_bar.contains(world) {
            return Some(WindowRegion::TitleBar);
        }
        if rects.content.contains(world) {
            return Some(WindowRegion::Content);
        }
        
        None
    }
}

struct ChromeRects {
    frame: WorldRect,
    title_bar: WorldRect,
    content: WorldRect,
    close_button: WorldRect,
    resize_bottom_right: WorldRect,
    // ... other rects
}
```

---

## Query Operations

```rust
impl WindowManager {
    /// Get windows sorted by z-order (bottom to top)
    pub fn windows_by_z(&self) -> Vec<&Window> {
        let mut windows: Vec<_> = self.windows.values().collect();
        windows.sort_by_key(|w| w.z_order);
        windows
    }
    
    /// Get windows on a desktop sorted by z-order
    pub fn windows_on_desktop(&self, desktop: DesktopId) -> Vec<&Window> {
        let mut windows: Vec<_> = self.by_desktop
            .get(&desktop)
            .into_iter()
            .flatten()
            .filter_map(|id| self.windows.get(id))
            .collect();
        windows.sort_by_key(|w| w.z_order);
        windows
    }
    
    /// Get window info
    pub fn get(&self, id: WindowId) -> Option<&Window> {
        self.windows.get(&id)
    }
    
    /// Get mutable window
    pub fn get_mut(&mut self, id: WindowId) -> Option<&mut Window> {
        self.windows.get_mut(&id)
    }
    
    /// Find window at world position
    pub fn window_at(
        &self,
        desktop: DesktopId,
        world: (f64, f64),
        config: &ChromeConfig,
    ) -> Option<(WindowId, WindowRegion)> {
        // Check in reverse z-order (top first)
        let windows = self.windows_on_desktop(desktop);
        for window in windows.into_iter().rev() {
            if let Some(region) = window.region_at(world, config) {
                return Some((window.id, region));
            }
        }
        None
    }
}
```

---

## Module Structure

```
window/
├── manager.rs      # WindowManager, lifecycle
├── window.rs       # Window struct, state
├── chrome.rs       # ChromeConfig, regions, hit testing
└── layout.rs       # Constraints, cascading
```

---

*[Back to Desktop](README.md) | [Previous: Desktops](03-desktops.md) | [Next: Rendering](05-rendering.md)*
