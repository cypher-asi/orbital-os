# Persistence

**Component:** 08-desktop/09-persistence  
**Status:** Specification

---

## Overview

The compositor supports state persistence through snapshots. The crate provides serialization/deserialization; the host provides storage backend.

---

## Snapshot Structure

```rust
use serde::{Deserialize, Serialize};

/// Serializable compositor state
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    /// Schema version for forward compatibility
    pub version: u32,
    
    /// Snapshot timestamp (optional)
    pub timestamp: Option<u64>,
    
    /// Desktop states
    pub desktops: Vec<DesktopSnapshot>,
    
    /// Active desktop ID
    pub active_desktop: u64,
    
    /// Void layout configuration
    pub void_layout: VoidLayoutSnapshot,
}

/// Current snapshot version
pub const SNAPSHOT_VERSION: u32 = 1;

/// Desktop state snapshot
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DesktopSnapshot {
    /// Desktop ID
    pub id: u64,
    
    /// Display name
    pub name: String,
    
    /// Camera state
    pub camera: CameraSnapshot,
    
    /// Background (optional - uses default if not present)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<BackgroundSnapshot>,
    
    /// Windows on this desktop
    pub windows: Vec<WindowSnapshot>,
}

/// Camera state snapshot
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CameraSnapshot {
    /// Center X coordinate
    pub center_x: f64,
    
    /// Center Y coordinate
    pub center_y: f64,
    
    /// Zoom level
    pub zoom: f32,
}

/// Background snapshot
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BackgroundSnapshot {
    Solid {
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    },
    Gradient {
        stops: Vec<GradientStopSnapshot>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GradientStopSnapshot {
    pub position: f32,
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

/// Window state snapshot
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindowSnapshot {
    /// Window ID
    pub id: u64,
    
    /// Window title
    pub title: String,
    
    /// Position X
    pub x: f64,
    
    /// Position Y
    pub y: f64,
    
    /// Width
    pub width: f32,
    
    /// Height
    pub height: f32,
    
    /// Z-order
    pub z_order: u32,
    
    /// Surface kind
    pub surface: SurfaceKindSnapshot,
    
    /// Application identifier (for reopening)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
}

/// Surface kind snapshot
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum SurfaceKindSnapshot {
    Gpu,
    ReactDom,
    Hybrid,
}

/// Void layout snapshot
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum VoidLayoutSnapshot {
    StripHorizontal {
        gap: f32,
        portal_width: f32,
        portal_height: f32,
    },
    Grid {
        cols: u32,
        gap: f32,
        portal_width: f32,
        portal_height: f32,
    },
}
```

---

## Snapshot Creation

```rust
impl Compositor {
    /// Create snapshot of current state
    pub fn snapshot(&self) -> Snapshot {
        let desktops = self.state.desktops.values()
            .map(|d| self.snapshot_desktop(d))
            .collect();
        
        Snapshot {
            version: SNAPSHOT_VERSION,
            timestamp: Some(current_timestamp()),
            desktops,
            active_desktop: self.state.active_desktop.0,
            void_layout: self.snapshot_void_layout(),
        }
    }
    
    fn snapshot_desktop(&self, desktop: &Desktop) -> DesktopSnapshot {
        let windows = self.windows.windows_on_desktop(desktop.id)
            .into_iter()
            .map(|w| self.snapshot_window(w))
            .collect();
        
        DesktopSnapshot {
            id: desktop.id.0,
            name: desktop.name.clone(),
            camera: CameraSnapshot {
                center_x: desktop.camera.center.0,
                center_y: desktop.camera.center.1,
                zoom: desktop.camera.zoom,
            },
            background: self.snapshot_background(&desktop.background),
            windows,
        }
    }
    
    fn snapshot_window(&self, window: &Window) -> WindowSnapshot {
        WindowSnapshot {
            id: window.id.0,
            title: window.title.clone(),
            x: window.position.0,
            y: window.position.1,
            width: window.size.0,
            height: window.size.1,
            z_order: window.z_order,
            surface: match window.surface {
                SurfaceKind::Gpu => SurfaceKindSnapshot::Gpu,
                SurfaceKind::ReactDom => SurfaceKindSnapshot::ReactDom,
                SurfaceKind::Hybrid => SurfaceKindSnapshot::Hybrid,
            },
            app_id: None, // Host can add this via extension
        }
    }
    
    fn snapshot_void_layout(&self) -> VoidLayoutSnapshot {
        match &self.state.void_layout {
            VoidLayout::StripHorizontal { gap, portal_size } => {
                VoidLayoutSnapshot::StripHorizontal {
                    gap: *gap,
                    portal_width: portal_size.0,
                    portal_height: portal_size.1,
                }
            }
            VoidLayout::Grid { cols, gap, portal_size } => {
                VoidLayoutSnapshot::Grid {
                    cols: *cols,
                    gap: *gap,
                    portal_width: portal_size.0,
                    portal_height: portal_size.1,
                }
            }
        }
    }
    
    fn snapshot_background(&self, bg: &Background) -> Option<BackgroundSnapshot> {
        match bg {
            Background::Solid { color } => Some(BackgroundSnapshot::Solid {
                r: color.0,
                g: color.1,
                b: color.2,
                a: color.3,
            }),
            Background::Gradient { stops } => Some(BackgroundSnapshot::Gradient {
                stops: stops.iter().map(|s| GradientStopSnapshot {
                    position: s.position,
                    r: s.color.0,
                    g: s.color.1,
                    b: s.color.2,
                    a: s.color.3,
                }).collect(),
            }),
        }
    }
}
```

---

## Snapshot Restoration

```rust
impl Compositor {
    /// Restore state from snapshot
    pub fn restore(&mut self, snapshot: Snapshot) -> Result<()> {
        // Validate version
        if snapshot.version > SNAPSHOT_VERSION {
            return Err(Error::InvalidSnapshot(
                format!("unsupported version: {}", snapshot.version)
            ));
        }
        
        // Validate at least one desktop
        if snapshot.desktops.is_empty() {
            return Err(Error::InvalidSnapshot("no desktops".into()));
        }
        
        // Clear current state
        self.clear_state();
        
        // Restore desktops
        for desktop_snap in snapshot.desktops {
            self.restore_desktop(desktop_snap)?;
        }
        
        // Restore active desktop
        let active_id = DesktopId(snapshot.active_desktop);
        if self.state.desktops.contains_key(&active_id) {
            self.state.active_desktop = active_id;
        } else {
            // Fall back to first desktop
            self.state.active_desktop = *self.state.desktops.keys()
                .next()
                .expect("at least one desktop");
        }
        
        // Restore void layout
        self.restore_void_layout(snapshot.void_layout);
        
        Ok(())
    }
    
    fn clear_state(&mut self) {
        // Destroy all DOM mounts
        self.ui.destroy_all_mounts();
        
        // Clear windows
        self.windows = WindowManager::new();
        
        // Clear desktops
        self.state.desktops.clear();
    }
    
    fn restore_desktop(&mut self, snap: DesktopSnapshot) -> Result<()> {
        let id = DesktopId(snap.id);
        
        let camera = Camera {
            center: (snap.camera.center_x, snap.camera.center_y),
            zoom: snap.camera.zoom.clamp(0.1, 10.0),
        };
        
        let background = snap.background
            .map(|b| self.restore_background(b))
            .unwrap_or_default();
        
        let desktop = Desktop {
            id,
            name: snap.name,
            camera,
            windows: Vec::new(),
            background,
            preview: None,
            preview_dirty: true,
        };
        
        self.state.desktops.insert(id, desktop);
        
        // Update next ID
        self.state.next_desktop_id = self.state.next_desktop_id.max(snap.id + 1);
        
        // Restore windows
        for window_snap in snap.windows {
            self.restore_window(id, window_snap)?;
        }
        
        Ok(())
    }
    
    fn restore_window(&mut self, desktop: DesktopId, snap: WindowSnapshot) -> Result<()> {
        let id = WindowId(snap.id);
        
        let surface = match snap.surface {
            SurfaceKindSnapshot::Gpu => SurfaceKind::Gpu,
            SurfaceKindSnapshot::ReactDom => SurfaceKind::ReactDom,
            SurfaceKindSnapshot::Hybrid => SurfaceKind::Hybrid,
        };
        
        let window = Window {
            id,
            desktop,
            title: snap.title,
            position: (snap.x, snap.y),
            size: (snap.width, snap.height),
            min_size: (100.0, 100.0),
            max_size: None,
            z_order: snap.z_order,
            surface,
            focused: false,
            state: WindowState::Normal,
            mount: None,
        };
        
        self.windows.windows.insert(id, window.clone());
        self.windows.by_desktop.entry(desktop).or_default().push(id);
        
        // Update next IDs
        self.windows.next_id = self.windows.next_id.max(snap.id + 1);
        self.windows.next_z = self.windows.next_z.max(snap.z_order + 1);
        
        // Create mount if needed
        self.ui.create_mount(&window)?;
        
        // Add to desktop's window list
        if let Some(d) = self.state.desktops.get_mut(&desktop) {
            d.windows.push(id);
        }
        
        Ok(())
    }
    
    fn restore_void_layout(&mut self, snap: VoidLayoutSnapshot) {
        self.state.void_layout = match snap {
            VoidLayoutSnapshot::StripHorizontal { gap, portal_width, portal_height } => {
                VoidLayout::StripHorizontal {
                    gap,
                    portal_size: (portal_width, portal_height),
                }
            }
            VoidLayoutSnapshot::Grid { cols, gap, portal_width, portal_height } => {
                VoidLayout::Grid {
                    cols,
                    gap,
                    portal_size: (portal_width, portal_height),
                }
            }
        };
    }
    
    fn restore_background(&self, snap: BackgroundSnapshot) -> Background {
        match snap {
            BackgroundSnapshot::Solid { r, g, b, a } => {
                Background::Solid { color: (r, g, b, a) }
            }
            BackgroundSnapshot::Gradient { stops } => {
                Background::Gradient {
                    stops: stops.into_iter().map(|s| GradientStop {
                        position: s.position,
                        color: (s.r, s.g, s.b, s.a),
                    }).collect(),
                }
            }
        }
    }
}
```

---

## Persistence Backend Trait

```rust
/// Persistence backend trait (implemented by host)
pub trait PersistenceBackend {
    /// Save snapshot to storage
    fn save(&self, snapshot: &Snapshot) -> Result<()>;
    
    /// Load snapshot from storage
    fn load(&self) -> Result<Option<Snapshot>>;
    
    /// Delete stored snapshot
    fn delete(&self) -> Result<()>;
}
```

### Example: LocalStorage Backend

```rust
/// LocalStorage backend (for browser)
pub struct LocalStorageBackend {
    key: String,
}

impl LocalStorageBackend {
    pub fn new(key: &str) -> Self {
        Self { key: key.to_string() }
    }
}

impl PersistenceBackend for LocalStorageBackend {
    fn save(&self, snapshot: &Snapshot) -> Result<()> {
        let json = serde_json::to_string(snapshot)
            .map_err(|e| Error::InvalidSnapshot(e.to_string()))?;
        
        let storage = web_sys::window()
            .and_then(|w| w.local_storage().ok())
            .flatten()
            .ok_or(Error::NoDom)?;
        
        storage.set_item(&self.key, &json)
            .map_err(|_| Error::StorageError)?;
        
        Ok(())
    }
    
    fn load(&self) -> Result<Option<Snapshot>> {
        let storage = web_sys::window()
            .and_then(|w| w.local_storage().ok())
            .flatten()
            .ok_or(Error::NoDom)?;
        
        let json = match storage.get_item(&self.key).ok().flatten() {
            Some(j) => j,
            None => return Ok(None),
        };
        
        let snapshot = serde_json::from_str(&json)
            .map_err(|e| Error::InvalidSnapshot(e.to_string()))?;
        
        Ok(Some(snapshot))
    }
    
    fn delete(&self) -> Result<()> {
        let storage = web_sys::window()
            .and_then(|w| w.local_storage().ok())
            .flatten()
            .ok_or(Error::NoDom)?;
        
        storage.remove_item(&self.key)
            .map_err(|_| Error::StorageError)?;
        
        Ok(())
    }
}
```

---

## Auto-Save (Optional)

```rust
/// Auto-save configuration
pub struct AutoSaveConfig {
    /// Interval in milliseconds
    pub interval_ms: u32,
    
    /// Save on window close
    pub save_on_close: bool,
    
    /// Debounce changes
    pub debounce_ms: u32,
}

impl Default for AutoSaveConfig {
    fn default() -> Self {
        Self {
            interval_ms: 30_000, // 30 seconds
            save_on_close: true,
            debounce_ms: 1_000,  // 1 second
        }
    }
}
```

---

## Module Structure

```
persistence/
├── snapshot.rs     # Snapshot types, serialization
├── save.rs         # Snapshot creation
├── restore.rs      # Snapshot restoration
└── backend.rs      # PersistenceBackend trait
```

---

*[Back to Desktop](README.md) | [Previous: Transitions](08-transitions.md) | [Next: Configuration](10-configuration.md)*
