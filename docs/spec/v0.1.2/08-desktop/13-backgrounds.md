# Interactive Backgrounds

**Component:** 08-desktop/13-backgrounds  
**Status:** Specification

---

## Overview

This document specifies the interactive procedural background system for the desktop compositor. Unlike static solid or gradient backgrounds, procedural backgrounds are animated, GPU-rendered, and respond to viewport state and desktop transitions.

---

## Background Types

The compositor supports multiple procedural background types that can be assigned per-desktop:

| Type | Description | Performance |
|------|-------------|-------------|
| **Grain** | Subtle animated film grain on near-black | Minimal GPU cost (single pass) |
| **Mist** | Animated smoke with glass overlay effect | Moderate GPU cost (two-pass) |

### Grain Background (Default)

A subtle, animated film grain effect on a near-black base color. Designed to add visual texture without distraction.

**Visual characteristics:**
- Base color: Dark gray (#0e0e10 approximate)
- Grain: Hash-based noise with temporal variation
- Animation: Continuous subtle flickering

**Technical details:**
- Single-pass fullscreen shader
- Two hash functions blended for organic grain
- Time-based seed variation for animation
- Minimal GPU overhead

### Mist Background

An atmospheric animated smoke effect with a glass overlay, creating depth and visual interest.

**Visual characteristics:**
- Multi-layer smoke with parallax depth
- Slow drift animation
- Vignette darkening at edges
- Glass overlay with fresnel glow, specular highlights, and dust particles

**Technical details:**
- Two-pass rendering for performance
- Pass 1: Smoke at quarter resolution (max 480x270)
- Pass 2: Static glass overlay composited at full resolution
- Value noise for organic smoke patterns

---

## BackgroundType Enum

```rust
/// Available procedural background types
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackgroundType {
    /// Subtle film grain on dark background (default)
    Grain,
    
    /// Animated misty/smoky atmosphere with glass overlay
    Mist,
}

impl BackgroundType {
    /// Get all available background types
    pub fn all() -> &'static [BackgroundType] {
        &[BackgroundType::Grain, BackgroundType::Mist]
    }
    
    /// Get the display name for this background
    pub fn name(&self) -> &'static str {
        match self {
            BackgroundType::Grain => "Film Grain",
            BackgroundType::Mist => "Misty Smoke",
        }
    }
    
    /// Parse from string ID
    pub fn from_id(id: &str) -> Option<Self> {
        match id.to_lowercase().as_str() {
            "grain" => Some(BackgroundType::Grain),
            "mist" => Some(BackgroundType::Mist),
            _ => None,
        }
    }
    
    /// Get the string ID
    pub fn id(&self) -> &'static str {
        match self {
            BackgroundType::Grain => "grain",
            BackgroundType::Mist => "mist",
        }
    }
}

impl Default for BackgroundType {
    fn default() -> Self {
        BackgroundType::Grain
    }
}
```

---

## Shader Architecture

### Shared Vertex Shader

All backgrounds use a fullscreen triangle technique (no geometry buffer needed):

```wgsl
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
    var out: VsOut;
    
    // Generate oversized triangle covering the screen
    // vertex 0: (-1, -1), vertex 1: (3, -1), vertex 2: (-1, 3)
    let x = f32(i32(vertex_index & 1u) * 4 - 1);
    let y = f32(i32(vertex_index >> 1u) * 4 - 1);
    
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    
    return out;
}
```

### Uniform Buffer Layout

All backgrounds share a common uniform buffer:

```rust
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    /// Elapsed time in seconds
    time: f32,
    
    /// Current zoom level
    zoom: f32,
    
    /// Screen resolution (width, height)
    resolution: [f32; 2],
    
    /// Viewport center in world coordinates
    viewport_center: [f32; 2],
    
    /// Number of workspaces
    workspace_count: f32,
    
    /// Index of active workspace
    active_workspace: f32,
    
    /// Per-workspace background types (0=grain, 1=mist)
    /// Supports up to 4 workspaces
    workspace_backgrounds: [f32; 4],
    
    /// Whether transitioning between workspaces (0 or 1)
    transitioning: f32,
    
    /// Workspace dimensions for layout
    workspace_width: f32,
    workspace_height: f32,
    workspace_gap: f32,
    
    /// Padding for alignment
    _pad: [f32; 4],
}
```

---

## Multi-Workspace Rendering

The background shader renders differently based on the current view mode:

### Workspace Mode (Normal)

When viewing a single desktop at zoom >= 1.0:

- Renders the active workspace's background fullscreen
- Uses screen UV coordinates for stable pattern
- Ignores other workspaces entirely

### Void Mode (Overview)

When zoomed out to see all desktops:

- Renders a grid of workspace backgrounds
- Each workspace shows its own background type
- Workspaces have visible borders
- Active workspace is slightly highlighted
- Areas outside workspaces render as "the void" (pure darkness)

### Transition Mode (Sliding)

During desktop-to-desktop transitions:

- Both backgrounds render with a sliding wipe effect
- The boundary between workspaces moves across screen
- Each side of the boundary shows its workspace's background
- Subtle edge effect at the boundary for visual clarity

```
┌─────────────────────────────────────────────┐
│                                             │
│   Workspace A      │     Workspace B        │
│   (Grain)          │     (Mist)             │
│                    │                        │
│                    │                        │
│                    │ <- Boundary moves      │
└─────────────────────────────────────────────┘
```

---

## Mist Two-Pass Rendering

The Mist background uses a two-pass architecture for performance:

### Pass 1: Smoke (Quarter Resolution)

Renders animated smoke to an offscreen texture at 1/4 resolution (capped at 480x270):

- 2-octave value noise for organic patterns
- Slow drift animation (time * 0.030)
- Vignette darkening at edges
- Dark base color with smoke color overlay

### Pass 2: Composite (Full Resolution)

Combines the upscaled smoke with a static glass overlay:

- Glass overlay is rendered once on init/resize
- Contains: fresnel edge glow, specular highlight streak, dust/grain
- Alpha channel stores UV distortion amount
- Final composite: smoke (with distortion) + glass (additive)

```
┌─────────────────┐     ┌─────────────────┐
│   Smoke Pass    │     │  Glass Overlay  │
│  (animated,     │  +  │  (static,       │
│   quarter res)  │     │   full res)     │
└────────┬────────┘     └────────┬────────┘
         │                       │
         └───────────┬───────────┘
                     │
              ┌──────▼──────┐
              │  Composite  │
              │  (distort   │
              │   + blend)  │
              └─────────────┘
```

---

## BackgroundRenderer API

```rust
/// Background renderer with multiple switchable shaders
pub struct BackgroundRenderer {
    // ... internal state
}

impl BackgroundRenderer {
    /// Create a new background renderer
    pub async fn new(canvas: web_sys::HtmlCanvasElement) -> Result<Self, String>;
    
    /// Get the current background type
    pub fn current_background(&self) -> BackgroundType;
    
    /// Set the background type (instant switch)
    pub fn set_background(&mut self, bg_type: BackgroundType);
    
    /// Resize the renderer
    pub fn resize(&mut self, width: u32, height: u32);
    
    /// Set viewport state for zoom effects
    pub fn set_viewport(&mut self, zoom: f32, center_x: f32, center_y: f32);
    
    /// Set workspace layout dimensions
    pub fn set_workspace_dimensions(&mut self, width: f32, height: f32, gap: f32);
    
    /// Set workspace info for multi-workspace rendering
    pub fn set_workspace_info(
        &mut self,
        count: usize,
        active: usize,
        backgrounds: &[BackgroundType],
    );
    
    /// Set view mode (workspace vs void/transitioning)
    pub fn set_view_mode(&mut self, in_void_or_transitioning: bool);
    
    /// Render a frame
    pub fn render(&mut self) -> Result<(), String>;
}
```

---

## Per-Desktop Background Configuration

Each desktop stores its own background type:

```rust
pub struct Desktop {
    pub id: DesktopId,
    pub name: String,
    pub camera: Camera,
    pub windows: Vec<WindowId>,
    
    /// Procedural background for this desktop
    pub background: BackgroundType,
    
    // ... other fields
}
```

When switching desktops, the compositor:
1. Updates the active workspace index
2. Passes all workspace background types to the renderer
3. Sets the transitioning flag during animations

---

## Integration with Compositor

The compositor integrates the background renderer in its frame lifecycle:

```rust
impl Compositor {
    pub fn render(&mut self) {
        // Update background renderer state
        self.background.set_viewport(
            camera.zoom,
            camera.center.0 as f32,
            camera.center.1 as f32,
        );
        
        self.background.set_workspace_info(
            self.state.desktops.len(),
            self.state.active_desktop_index(),
            &self.collect_workspace_backgrounds(),
        );
        
        self.background.set_view_mode(
            self.state.mode == Mode::Void || self.transitions.is_active()
        );
        
        // Render background first
        self.background.render()?;
        
        // Then render windows on top
        self.renderer.render_windows(...);
    }
}
```

---

## Performance Considerations

| Background | GPU Load | Memory | Notes |
|------------|----------|--------|-------|
| Grain | Very Low | Minimal | Single fullscreen pass |
| Mist | Moderate | ~1.5 MB | Two passes, offscreen textures |

### Optimization Strategies

1. **Quarter-resolution smoke**: Mist smoke renders at 1/4 resolution (max 480x270)
2. **Static glass overlay**: Rendered once on init/resize, reused every frame
3. **Shared uniforms**: Single uniform buffer for all background types
4. **Hot-swappable pipelines**: Background type changes without recompilation

---

## Adding New Backgrounds

To add a new procedural background:

1. Add variant to `BackgroundType` enum
2. Implement `shader_source()` method returning WGSL code
3. Create pipeline in `BackgroundRenderer::new()`
4. Handle any multi-pass rendering in `render()`
5. Update `from_id()` and `id()` methods
6. Add to persistence snapshot/restore

---

## Module Structure

```
render/
├── background.rs      # BackgroundRenderer, BackgroundType
└── shaders/
    ├── grain.wgsl     # Grain fragment shader
    ├── mist_smoke.wgsl    # Mist pass 1
    ├── mist_glass.wgsl    # Mist static glass
    └── mist_composite.wgsl # Mist pass 2
```

---

*[Back to Desktop](README.md) | [Previous: Acceptance](12-acceptance.md)*
