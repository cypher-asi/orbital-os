# Desktops and Void

**Component:** 08-desktop/03-desktops  
**Status:** Specification

---

## Overview

This document specifies desktop management and the Void overview mode. Each desktop is an infinite 2D canvas containing windows, with its own camera state. The Void provides a meta-view of all desktops.

---

## Desktop Structure

```rust
/// A desktop (infinite 2D world)
pub struct Desktop {
    /// Unique identifier
    pub id: DesktopId,
    
    /// Display name
    pub name: String,
    
    /// Camera state
    pub camera: Camera,
    
    /// Windows on this desktop
    pub windows: Vec<WindowId>,
    
    /// Background configuration
    pub background: Background,
    
    /// Preview texture (for Void mode)
    preview: Option<PreviewTexture>,
    
    /// Preview dirty flag
    preview_dirty: bool,
}

/// Background configuration
pub enum Background {
    Solid { color: (f32, f32, f32, f32) },
    Gradient { stops: Vec<GradientStop> },
    Procedural(ProceduralBackground),
}

/// Procedural animated backgrounds (see [13-backgrounds.md](13-backgrounds.md))
/// These backgrounds are GPU-rendered and animated.
pub enum ProceduralBackground {
    /// Subtle animated film grain on near-black (default)
    Grain,
    /// Animated misty smoke with glass overlay effect
    Mist,
}

/// Preview texture for Void mode
struct PreviewTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
}
```

---

## Desktop Management API

```rust
impl Compositor {
    /// Create a new desktop
    pub fn desktop_create(&mut self, spec: DesktopSpec) -> DesktopId {
        let id = DesktopId(self.state.next_desktop_id);
        self.state.next_desktop_id += 1;
        
        let desktop = Desktop {
            id,
            name: spec.name,
            camera: Camera::default(),
            windows: Vec::new(),
            background: spec.background.unwrap_or(Background::default()),
            preview: None,
            preview_dirty: true,
        };
        
        self.state.desktops.insert(id, desktop);
        id
    }
    
    /// Remove a desktop and all its windows
    pub fn desktop_remove(&mut self, id: DesktopId) -> Result<()> {
        // Cannot remove last desktop
        if self.state.desktops.len() <= 1 {
            return Err(Error::LastDesktop);
        }
        
        // Cannot remove non-existent desktop
        let desktop = self.state.desktops.remove(&id)
            .ok_or(Error::InvalidDesktop(id))?;
        
        // Close all windows on this desktop
        for window_id in desktop.windows {
            self.window_close_internal(window_id);
        }
        
        // Switch active if needed
        if self.state.active_desktop == id {
            let new_active = *self.state.desktops.keys().next()
                .expect("at least one desktop remains");
            self.state.active_desktop = new_active;
            self.events.push(Event::DesktopChanged { desktop: new_active });
        }
        
        Ok(())
    }
    
    /// List all desktop IDs
    pub fn desktop_list(&self) -> Vec<DesktopId> {
        self.state.desktops.keys().copied().collect()
    }
    
    /// Get active desktop
    pub fn desktop_active(&self) -> DesktopId {
        self.state.active_desktop
    }
    
    /// Set active desktop
    pub fn desktop_set_active(&mut self, id: DesktopId) {
        if !self.state.desktops.contains_key(&id) {
            return;
        }
        
        if self.state.active_desktop == id {
            return;
        }
        
        self.state.active_desktop = id;
        self.events.push(Event::DesktopChanged { desktop: id });
    }
}
```

---

## Desktop Camera Operations

```rust
impl Compositor {
    /// Pan the active desktop camera
    pub fn pan(&mut self, delta: (f32, f32)) {
        let desktop = self.state.active_desktop_mut();
        let scale = 1.0 / desktop.camera.zoom;
        desktop.camera.center.0 -= delta.0 as f64 * scale as f64;
        desktop.camera.center.1 -= delta.1 as f64 * scale as f64;
        desktop.preview_dirty = true;
    }
    
    /// Zoom the active desktop camera
    pub fn zoom(&mut self, factor: f32, anchor: (f32, f32)) {
        let screen_size = self.screen_size();
        let desktop = self.state.active_desktop_mut();
        
        // Get world position under anchor before zoom
        let world_anchor = desktop.camera.screen_to_world(anchor, screen_size);
        
        // Apply zoom with limits
        let new_zoom = (desktop.camera.zoom * factor)
            .clamp(self.config.zoom.min, self.config.zoom.max);
        desktop.camera.zoom = new_zoom;
        
        // Adjust center to keep anchor in place
        let new_screen = desktop.camera.world_to_screen(world_anchor, screen_size);
        let correction = (
            (anchor.0 - new_screen.0) / new_zoom,
            (anchor.1 - new_screen.1) / new_zoom,
        );
        desktop.camera.center.0 -= correction.0 as f64;
        desktop.camera.center.1 -= correction.1 as f64;
        
        desktop.preview_dirty = true;
    }
    
    /// Animate camera to target position and zoom
    pub fn animate_to(&mut self, target: (f64, f64), target_zoom: f32) {
        let desktop = self.state.active_desktop();
        
        self.transitions.start(Transition::CameraMove {
            from: desktop.camera.clone(),
            to: Camera {
                center: target,
                zoom: target_zoom,
            },
            spec: TransitionSpec::default(),
        });
    }
}
```

---

## Void Mode

### Void Structure

```rust
impl CompositorState {
    /// Void camera (used when in Void mode)
    void_camera: Camera,
    
    /// Void layout configuration
    void_layout: VoidLayout,
    
    /// Selected desktop in Void (for keyboard navigation)
    void_selection: Option<DesktopId>,
}

/// Void layout options
pub enum VoidLayout {
    /// Horizontal strip of portals
    StripHorizontal {
        gap: f32,
        portal_size: (f32, f32),
    },
    
    /// Grid of portals
    Grid {
        cols: u32,
        gap: f32,
        portal_size: (f32, f32),
    },
}
```

### Void Layout Calculation

```rust
impl Compositor {
    /// Calculate portal positions for Void mode
    fn calculate_void_portals(&self) -> Vec<VoidPortal> {
        let desktops: Vec<_> = self.state.desktops.values().collect();
        
        match &self.state.void_layout {
            VoidLayout::StripHorizontal { gap, portal_size } => {
                self.layout_strip_horizontal(&desktops, *gap, *portal_size)
            }
            VoidLayout::Grid { cols, gap, portal_size } => {
                self.layout_grid(&desktops, *cols, *gap, *portal_size)
            }
        }
    }
    
    fn layout_strip_horizontal(
        &self,
        desktops: &[&Desktop],
        gap: f32,
        portal_size: (f32, f32),
    ) -> Vec<VoidPortal> {
        let total_width = desktops.len() as f32 * portal_size.0
            + (desktops.len() - 1).max(0) as f32 * gap;
        let start_x = -total_width / 2.0;
        
        desktops.iter().enumerate().map(|(i, desktop)| {
            let x = start_x + i as f32 * (portal_size.0 + gap);
            VoidPortal {
                desktop_id: desktop.id,
                rect: WorldRect {
                    x: x as f64,
                    y: -(portal_size.1 / 2.0) as f64,
                    width: portal_size.0,
                    height: portal_size.1,
                },
            }
        }).collect()
    }
    
    fn layout_grid(
        &self,
        desktops: &[&Desktop],
        cols: u32,
        gap: f32,
        portal_size: (f32, f32),
    ) -> Vec<VoidPortal> {
        let cols = cols as usize;
        let rows = (desktops.len() + cols - 1) / cols;
        
        let total_width = cols as f32 * portal_size.0
            + (cols - 1).max(0) as f32 * gap;
        let total_height = rows as f32 * portal_size.1
            + (rows - 1).max(0) as f32 * gap;
        
        let start_x = -total_width / 2.0;
        let start_y = -total_height / 2.0;
        
        desktops.iter().enumerate().map(|(i, desktop)| {
            let col = i % cols;
            let row = i / cols;
            let x = start_x + col as f32 * (portal_size.0 + gap);
            let y = start_y + row as f32 * (portal_size.1 + gap);
            VoidPortal {
                desktop_id: desktop.id,
                rect: WorldRect {
                    x: x as f64,
                    y: y as f64,
                    width: portal_size.0,
                    height: portal_size.1,
                },
            }
        }).collect()
    }
}

/// A portal in Void mode
struct VoidPortal {
    desktop_id: DesktopId,
    rect: WorldRect,
}
```

---

## Void Interactions

```rust
impl Compositor {
    /// Handle click in Void mode
    fn void_handle_click(&mut self, position: (f32, f32)) -> bool {
        let world = self.state.void_camera.screen_to_world(
            position,
            self.screen_size(),
        );
        
        let portals = self.calculate_void_portals();
        
        for portal in portals {
            if portal.rect.contains(world) {
                self.state.void_selection = Some(portal.desktop_id);
                return true;
            }
        }
        
        self.state.void_selection = None;
        false
    }
    
    /// Handle double-click in Void mode (enter desktop)
    fn void_handle_double_click(&mut self, position: (f32, f32)) -> bool {
        let world = self.state.void_camera.screen_to_world(
            position,
            self.screen_size(),
        );
        
        let portals = self.calculate_void_portals();
        
        for portal in portals {
            if portal.rect.contains(world) {
                self.exit_void(portal.desktop_id);
                return true;
            }
        }
        
        false
    }
    
    /// Handle keyboard in Void mode
    fn void_handle_key(&mut self, key: &str) -> bool {
        match key {
            "Escape" => {
                // Return to previously active desktop
                self.exit_void(self.state.active_desktop);
                true
            }
            "Enter" => {
                // Enter selected desktop
                if let Some(id) = self.state.void_selection {
                    self.exit_void(id);
                }
                true
            }
            "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" => {
                self.void_navigate_selection(key);
                true
            }
            _ => false,
        }
    }
    
    fn void_navigate_selection(&mut self, direction: &str) {
        // Implementation depends on layout
        // Moves void_selection to adjacent portal
    }
}
```

---

## Preview Textures

```rust
impl Compositor {
    /// Update preview textures for Void mode
    fn update_previews(&mut self) {
        if self.state.mode != Mode::Void {
            return;
        }
        
        let budget = self.config.preview_budget;
        let mut rendered = 0;
        
        for desktop in self.state.desktops.values_mut() {
            if rendered >= budget {
                break;
            }
            
            if desktop.preview_dirty {
                self.render_desktop_preview(desktop);
                desktop.preview_dirty = false;
                rendered += 1;
            }
        }
    }
    
    fn render_desktop_preview(&mut self, desktop: &mut Desktop) {
        let size = self.config.preview_size;
        
        // Ensure preview texture exists
        if desktop.preview.is_none() {
            desktop.preview = Some(PreviewTexture::new(&self.gpu, size));
        }
        
        let preview = desktop.preview.as_ref().unwrap();
        
        // Render desktop to preview texture
        self.renderer.render_desktop_to_texture(
            &self.gpu,
            &preview.view,
            desktop,
            &self.config,
        );
    }
}
```

---

## Module Structure

```
scene/
├── desktop.rs      # Desktop struct, camera operations
├── void.rs         # Void mode, layout, portals
└── preview.rs      # Preview texture management
```

---

*[Back to Desktop](README.md) | [Previous: Compositor](02-compositor.md) | [Next: Windows](04-windows.md)*
