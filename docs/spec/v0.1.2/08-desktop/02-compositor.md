# Compositor

**Component:** 08-desktop/02-compositor  
**Status:** Specification

---

## Overview

The Compositor is the main engine coordinating all desktop environment subsystems. It owns the WebGPU context, scene state, and orchestrates the frame lifecycle.

---

## Compositor Structure

```rust
/// Main compositor engine
pub struct Compositor {
    /// WebGPU device and queue
    gpu: GpuContext,
    
    /// Scene state (desktops, windows, mode)
    state: CompositorState,
    
    /// Input router
    input: InputRouter,
    
    /// Transition system
    transitions: TransitionSystem,
    
    /// UI bridge for DOM mounts
    ui: UiBridge,
    
    /// Render graph
    renderer: Renderer,
    
    /// Event queue
    events: Vec<Event>,
    
    /// Configuration
    config: CompositorConfig,
}
```

### Internal Components

```rust
/// WebGPU context
struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface,
    surface_config: wgpu::SurfaceConfiguration,
}

/// Compositor state
struct CompositorState {
    /// All desktops
    desktops: HashMap<DesktopId, Desktop>,
    
    /// Currently active desktop
    active_desktop: DesktopId,
    
    /// Current mode
    mode: Mode,
    
    /// Void camera (when in Void mode)
    void_camera: Camera,
    
    /// Void layout configuration
    void_layout: VoidLayout,
    
    /// ID generators
    next_desktop_id: u64,
    next_window_id: u64,
}
```

---

## Frame Lifecycle

### Update Phase

```rust
impl Compositor {
    pub fn update(&mut self, dt_ms: f32) {
        // 1. Update transitions
        self.transitions.update(dt_ms);
        
        // 2. Apply transition effects to state
        self.apply_transition_effects();
        
        // 3. Update camera inertia (if any)
        self.update_camera_inertia(dt_ms);
        
        // 4. Cull windows outside viewport
        self.update_culling();
        
        // 5. Update DOM mounts
        self.ui.update(&self.state, &self.config);
    }
}
```

### Render Phase

```rust
impl Compositor {
    pub fn render(&mut self) {
        // 1. Begin frame
        let frame = self.gpu.surface.get_current_texture()
            .expect("surface texture");
        
        // 2. Create render pass
        let view = frame.texture.create_view(&Default::default());
        
        // 3. Render based on mode
        match self.state.mode {
            Mode::Desktop => self.render_desktop(&view),
            Mode::Void => self.render_void(&view),
        }
        
        // 4. Present
        frame.present();
    }
    
    fn render_desktop(&mut self, target: &wgpu::TextureView) {
        let desktop = self.state.active_desktop();
        self.renderer.render_desktop(
            &self.gpu,
            target,
            desktop,
            &self.config,
        );
    }
    
    fn render_void(&mut self, target: &wgpu::TextureView) {
        self.renderer.render_void(
            &self.gpu,
            target,
            &self.state,
            &self.config,
        );
    }
}
```

---

## Mode Management

```rust
impl Compositor {
    /// Current compositor mode
    pub fn mode(&self) -> Mode {
        self.state.mode
    }
    
    /// Enter Void overview mode
    pub fn enter_void(&mut self) {
        if self.state.mode == Mode::Void {
            return;
        }
        
        // Calculate target void camera position
        let target_camera = self.calculate_void_camera();
        
        // Start transition
        self.transitions.start(Transition::EnterVoid {
            from_camera: self.state.active_desktop().camera.clone(),
            to_camera: target_camera,
            spec: TransitionSpec::default(),
        });
        
        // Update mode
        self.state.mode = Mode::Void;
        self.events.push(Event::ModeChanged { mode: Mode::Void });
    }
    
    /// Exit Void to target desktop
    pub fn exit_void(&mut self, target: DesktopId) {
        if self.state.mode != Mode::Void {
            return;
        }
        
        let desktop = match self.state.desktops.get(&target) {
            Some(d) => d,
            None => return,
        };
        
        // Start transition
        self.transitions.start(Transition::ExitVoid {
            from_camera: self.state.void_camera.clone(),
            to_desktop: target,
            to_camera: desktop.camera.clone(),
            spec: TransitionSpec::default(),
        });
        
        // Update state
        self.state.active_desktop = target;
        self.state.mode = Mode::Desktop;
        
        self.events.push(Event::ModeChanged { mode: Mode::Desktop });
        self.events.push(Event::DesktopChanged { desktop: target });
    }
}
```

---

## Coordinate Systems

The compositor uses three coordinate systems:

| System | Type | Usage |
|--------|------|-------|
| **World** | `f64` | Window positions, camera center |
| **View** | `f32` | GPU-visible coords after origin rebasing |
| **Screen** | `f32` | Pixel coordinates on canvas |

### Camera and Projection

```rust
/// Camera state for a viewport
pub struct Camera {
    /// Center position in world space (high precision)
    pub center: (f64, f64),
    
    /// Zoom level (1.0 = 100%)
    pub zoom: f32,
}

/// Projection utilities
impl Camera {
    /// Convert world coordinates to screen coordinates
    pub fn world_to_screen(
        &self,
        world: (f64, f64),
        screen_size: (f32, f32),
    ) -> (f32, f32) {
        let offset_x = (world.0 - self.center.0) as f32 * self.zoom;
        let offset_y = (world.1 - self.center.1) as f32 * self.zoom;
        (
            screen_size.0 / 2.0 + offset_x,
            screen_size.1 / 2.0 + offset_y,
        )
    }
    
    /// Convert screen coordinates to world coordinates
    pub fn screen_to_world(
        &self,
        screen: (f32, f32),
        screen_size: (f32, f32),
    ) -> (f64, f64) {
        let offset_x = (screen.0 - screen_size.0 / 2.0) / self.zoom;
        let offset_y = (screen.1 - screen_size.1 / 2.0) / self.zoom;
        (
            self.center.0 + offset_x as f64,
            self.center.1 + offset_y as f64,
        )
    }
    
    /// Calculate visible world rect
    pub fn visible_rect(&self, screen_size: (f32, f32)) -> WorldRect {
        let half_w = (screen_size.0 / 2.0 / self.zoom) as f64;
        let half_h = (screen_size.1 / 2.0 / self.zoom) as f64;
        WorldRect {
            x: self.center.0 - half_w,
            y: self.center.1 - half_h,
            width: (half_w * 2.0) as f32,
            height: (half_h * 2.0) as f32,
        }
    }
}
```

---

## Origin Rebasing

For GPU stability at extreme coordinates:

```rust
impl Compositor {
    /// Calculate rebased origin for current frame
    fn calculate_rebase_origin(&self) -> (f64, f64) {
        match self.state.mode {
            Mode::Desktop => {
                let desktop = self.state.active_desktop();
                desktop.camera.center
            }
            Mode::Void => self.state.void_camera.center,
        }
    }
    
    /// Convert world position to view position (GPU-safe f32)
    fn world_to_view(&self, world: (f64, f64), origin: (f64, f64)) -> (f32, f32) {
        (
            (world.0 - origin.0) as f32,
            (world.1 - origin.1) as f32,
        )
    }
}
```

---

## Resource Management

```rust
impl Compositor {
    /// Handle canvas resize
    pub fn resize(&mut self, width: u32, height: u32) {
        // Update surface configuration
        self.gpu.surface_config.width = width;
        self.gpu.surface_config.height = height;
        self.gpu.surface.configure(&self.gpu.device, &self.gpu.surface_config);
        
        // Notify renderer
        self.renderer.resize(&self.gpu, width, height);
    }
    
    /// Handle DPI change
    pub fn set_dpi(&mut self, dpi: f32) {
        self.renderer.set_dpi(dpi);
        self.ui.set_dpi(dpi);
    }
}

impl Drop for Compositor {
    fn drop(&mut self) {
        // Destroy all DOM mounts
        self.ui.destroy_all_mounts();
    }
}
```

---

## Module Structure

To satisfy the 500-line file limit:

```
compositor/
├── mod.rs          # Public facade, Compositor struct
├── state.rs        # CompositorState, mode management
├── lifecycle.rs    # update(), render() implementation
├── camera.rs       # Camera, projection utilities
└── resources.rs    # resize(), DPI handling
```

---

*[Back to Desktop](README.md) | [Previous: API](01-api.md) | [Next: Desktops](03-desktops.md)*
