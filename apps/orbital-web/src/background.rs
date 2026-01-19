//! Desktop Background Renderer
//!
//! WebGPU-based fullscreen backgrounds with multiple procedural shaders.
//! Backgrounds can be switched at runtime without recompiling.
//!
//! ## Available Backgrounds
//!
//! - **Grain**: Subtle film grain on near-black (default)
//! - **Mist**: Two-pass animated smoke with glass overlay effect
//!   - Pass 1: Multi-layer smoke with parallax depth and volume lighting
//!   - Pass 2: Glass refraction, fresnel, specular highlights, dust/grain
//!
//! ## Design
//!
//! - Full-screen triangle rendered via vertex shader (no geometry needed)
//! - All procedural - no textures required
//! - Shared uniform buffer and bind group across all backgrounds
//! - Hot-swappable pipelines for instant background changes

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use wgpu::util::DeviceExt;

/// Available background types
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackgroundType {
    /// Subtle film grain on dark background
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

    /// Get the shader source for this background
    fn shader_source(&self) -> &'static str {
        match self {
            BackgroundType::Grain => SHADER_GRAIN,
            BackgroundType::Mist => SHADER_MIST_SMOKE, // Pass 1 for pipeline creation
        }
    }
}

impl Default for BackgroundType {
    fn default() -> Self {
        BackgroundType::Grain
    }
}

impl BackgroundType {
    /// Parse from string ID (e.g., "grain", "mist")
    pub fn from_id(id: &str) -> Option<Self> {
        match id.to_lowercase().as_str() {
            "grain" => Some(BackgroundType::Grain),
            "mist" => Some(BackgroundType::Mist),
            _ => None,
        }
    }

    /// Get the string ID for this background
    pub fn id(&self) -> &'static str {
        match self {
            BackgroundType::Grain => "grain",
            BackgroundType::Mist => "mist",
        }
    }
}

// =============================================================================
// Shader Sources
// =============================================================================

/// Shared vertex shader for fullscreen triangle
const VERTEX_SHADER: &str = r#"
struct Uniforms {
    time: f32,
    _pad0: f32,
    resolution: vec2<f32>,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

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
"#;

/// Film grain background shader with zoom support and multi-workspace rendering
const SHADER_GRAIN: &str = r#"
struct Uniforms {
    time: f32,
    zoom: f32,
    resolution: vec2<f32>,
    viewport_center: vec2<f32>,
    workspace_count: f32,
    active_workspace: f32,
    workspace_backgrounds: vec4<f32>,
    transitioning: f32,
    workspace_width: f32,
    workspace_height: f32,
    workspace_gap: f32,
    _pad: vec4<f32>,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
    var out: VsOut;
    let x = f32(i32(vertex_index & 1u) * 4 - 1);
    let y = f32(i32(vertex_index >> 1u) * 4 - 1);
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

fn hash12(p: vec2<f32>) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453);
}

fn hash21(p: vec2<f32>) -> f32 {
    let p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    let q = p3 + dot(p3, p3.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(hash21(i), hash21(i + vec2<f32>(1.0, 0.0)), u.x),
        mix(hash21(i + vec2<f32>(0.0, 1.0)), hash21(i + vec2<f32>(1.0, 1.0)), u.x),
        u.y
    );
}

// Render grain-style background
fn render_grain(uv: vec2<f32>, time: f32) -> vec3<f32> {
    let scaled_px = floor(uv * uniforms.resolution);
    let base = vec3<f32>(0.055, 0.055, 0.065);
    let n0 = hash12(scaled_px);
    let n1 = hash12(scaled_px + vec2<f32>(time * 60.0, time * 37.0));
    let n = mix(n0, n1, 0.08);
    let grain = (n - 0.5) * 0.012;
    return clamp(base + vec3<f32>(grain), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Render mist-style background (simplified for overview)
fn render_mist(uv: vec2<f32>, time: f32) -> vec3<f32> {
    let asp = uniforms.resolution.x / uniforms.resolution.y;
    var p = vec2<f32>((uv.x - 0.5) * asp, uv.y - 0.5);
    let t = time * 0.030;
    p += vec2<f32>(t * 0.5, t * 0.15);
    let n = noise(p * 1.2) * 0.6 + noise(p * 2.4 + vec2<f32>(5.2, 1.3)) * 0.4;
    let mist = smoothstep(0.25, 0.70, n);
    let r = length(vec2<f32>((uv.x - 0.5) * asp, uv.y - 0.5));
    let vig = smoothstep(0.85, 0.25, r);
    let base = vec3<f32>(0.05, 0.07, 0.08);
    let smoke_color = vec3<f32>(0.12, 0.16, 0.17) * mist;
    var col = base + smoke_color;
    col *= (0.65 + 0.35 * vig);
    return col;
}

// Get workspace background type (0=grain, 1=mist)
fn get_workspace_bg(index: i32) -> f32 {
    if (index == 0) { return uniforms.workspace_backgrounds.x; }
    if (index == 1) { return uniforms.workspace_backgrounds.y; }
    if (index == 2) { return uniforms.workspace_backgrounds.z; }
    if (index == 3) { return uniforms.workspace_backgrounds.w; }
    return 0.0;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Workspace layout from uniforms (must match Rust desktop engine)
    let workspace_width = uniforms.workspace_width;
    let workspace_height = uniforms.workspace_height;
    let workspace_gap = uniforms.workspace_gap;
    let total_cell = workspace_width + workspace_gap;
    
    // Calculate world position based on viewport
    let screen_offset = (in.uv - 0.5) * uniforms.resolution;
    let world_pos = uniforms.viewport_center + screen_offset / uniforms.zoom;
    
    // transitioning > 0.5 means we're in void mode or transitioning between workspaces
    // When false (in workspace mode), always render full-screen regardless of zoom
    let in_void_or_transitioning = uniforms.transitioning > 0.5;
    
    // Close-up slide: zoom ~1.0 during transition = workspace-to-workspace slide
    // In this mode, we hide gaps/borders but keep the same position calculations
    let is_closeup_slide = in_void_or_transitioning && uniforms.zoom > 0.85;
    
    // Determine which workspace this pixel belongs to (always use real gap for math)
    let grid_x = world_pos.x + workspace_width * 0.5;
    let workspace_index = i32(floor(grid_x / total_cell));
    let cell_x = fract(grid_x / total_cell) * total_cell;
    
    // Check if we're in the gap between workspaces
    let in_gap = cell_x > workspace_width;
    
    // For close-up slides in the gap, figure out which workspace to extend
    var visual_workspace_index = workspace_index;
    if (is_closeup_slide && in_gap) {
        // In gap during slide - extend the workspace we're closer to
        let gap_pos = (cell_x - workspace_width) / workspace_gap;
        if (gap_pos > 0.5) {
            visual_workspace_index = workspace_index + 1;
        }
    }
    
    let valid_workspace = visual_workspace_index >= 0 && visual_workspace_index < i32(uniforms.workspace_count);
    let in_vertical_bounds = abs(world_pos.y) < workspace_height * 0.5;
    let is_active_workspace = visual_workspace_index == i32(uniforms.active_workspace);
    
    var color: vec3<f32>;
    
    if (!in_void_or_transitioning) {
        // WORKSPACE MODE: Always render full-screen active workspace background
        // The background fills the entire viewport regardless of zoom level
        let active_bg = get_workspace_bg(i32(uniforms.active_workspace));
        if (active_bg > 0.5) {
            color = render_mist(in.uv, uniforms.time);
        } else {
            color = render_grain(in.uv, uniforms.time);
        }
    } else if (is_closeup_slide) {
        // CLOSE-UP SLIDE: Render workspaces sliding across screen
        // The boundary between workspaces moves across the screen as viewport pans
        // Each side of the boundary renders its workspace's background using screen UV
        // This creates a visible "wipe" transition effect
        
        // Determine which workspace this screen pixel shows based on world position
        var pixel_workspace = workspace_index;
        if (in_gap) {
            // In gap - extend the closer workspace
            let gap_pos = (cell_x - workspace_width) / workspace_gap;
            if (gap_pos > 0.5) {
                pixel_workspace = workspace_index + 1;
            }
        }
        
        let clamped_index = clamp(pixel_workspace, 0, i32(uniforms.workspace_count) - 1);
        let bg_type = get_workspace_bg(clamped_index);
        
        // Use screen UV so the pattern stays stable on each side
        // The sliding effect comes from the BOUNDARY moving across the screen
        if (bg_type > 0.5) {
            color = render_mist(in.uv, uniforms.time);
        } else {
            color = render_grain(in.uv, uniforms.time);
        }
        
        // Add a subtle vertical line at workspace boundaries to make the wipe visible
        // Calculate where the boundary appears in screen space
        let boundary_world_x = f32(workspace_index) * total_cell + workspace_width * 0.5;
        let boundary_screen_x = (boundary_world_x - uniforms.viewport_center.x) * uniforms.zoom / uniforms.resolution.x + 0.5;
        let dist_to_boundary = abs(in.uv.x - boundary_screen_x);
        
        // Draw a subtle edge effect near the boundary (only if boundary is on screen)
        if (boundary_screen_x > 0.0 && boundary_screen_x < 1.0 && dist_to_boundary < 0.02) {
            let edge_fade = smoothstep(0.0, 0.02, dist_to_boundary);
            color = color * (0.7 + 0.3 * edge_fade);
        }
    } else if (valid_workspace && in_vertical_bounds && !in_gap) {
        // VOID MODE: Show grid of all workspaces with gaps and borders
        let bg_type = get_workspace_bg(workspace_index);
        
        let local_x = cell_x / workspace_width;
        let local_y = (world_pos.y + workspace_height * 0.5) / workspace_height;
        let local_uv = vec2<f32>(local_x, 1.0 - local_y);
        
        if (bg_type > 0.5) {
            color = render_mist(local_uv, uniforms.time);
        } else {
            color = render_grain(local_uv, uniforms.time);
        }
        
        // Add border around workspaces
        let edge_x = min(local_x, 1.0 - local_x);
        let edge_y = min(local_y, 1.0 - local_y);
        let edge_dist = min(edge_x, edge_y);
        let border_width = 0.01;
        let border = smoothstep(0.0, border_width, edge_dist);
        color = mix(vec3<f32>(0.2, 0.22, 0.25), color, border * 0.7 + 0.3);
        
        // Highlight active workspace slightly
        if (is_active_workspace) {
            color = color * 1.05;
        }
    } else {
        // THE VOID - pure darkness (outside workspace bounds or in gap)
        color = vec3<f32>(0.02, 0.02, 0.03);
    }
    
    // Add subtle vignette when in void mode and zoomed out
    if (in_void_or_transitioning && uniforms.zoom < 0.9) {
        let vignette_uv = (in.uv - 0.5) * 2.0;
        let vignette_strength = 1.0 - smoothstep(0.3, 0.9, uniforms.zoom);
        let vignette = 1.0 - dot(vignette_uv, vignette_uv) * 0.25 * vignette_strength;
        color *= vignette;
    }
    
    return vec4<f32>(color, 1.0);
}
"#;

/// Mist Pass 1: Ultra-optimized smoke effect (renders at quarter resolution)
/// - Single 2-octave noise (no FBM function call overhead)
/// - Single smoke layer
/// - No domain warp
/// - Renders at 1/4 resolution for ~16x fewer pixels than full res
const SHADER_MIST_SMOKE: &str = r#"
struct Uniforms {
    time: f32,
    zoom: f32,
    resolution: vec2<f32>,
    viewport_center: vec2<f32>,
    workspace_count: f32,
    active_workspace: f32,
    workspace_backgrounds: vec4<f32>,
    transitioning: f32,
    workspace_width: f32,
    workspace_height: f32,
    workspace_gap: f32,
    _pad: vec4<f32>,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
    var out: VsOut;
    let x = f32(i32(vertex_index & 1u) * 4 - 1);
    let y = f32(i32(vertex_index >> 1u) * 4 - 1);
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

// Fast hash
fn hash21(p: vec2<f32>) -> f32 {
    let p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    let q = p3 + dot(p3, p3.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
}

// Value noise
fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(hash21(i), hash21(i + vec2<f32>(1.0, 0.0)), u.x),
        mix(hash21(i + vec2<f32>(0.0, 1.0)), hash21(i + vec2<f32>(1.0, 1.0)), u.x),
        u.y
    );
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let asp = uniforms.resolution.x / uniforms.resolution.y;
    var p = vec2<f32>((in.uv.x - 0.5) * asp, in.uv.y - 0.5);

    // Slow drift
    let t = uniforms.time * 0.030;
    p += vec2<f32>(t * 0.5, t * 0.15);

    // Simple 2-octave noise inline (no function call overhead)
    let n = noise(p * 1.2) * 0.6 + noise(p * 2.4 + vec2<f32>(5.2, 1.3)) * 0.4;
    let mist = smoothstep(0.25, 0.70, n);

    // Vignette
    let r = length(vec2<f32>((in.uv.x - 0.5) * asp, in.uv.y - 0.5));
    let vig = smoothstep(0.85, 0.25, r);

    // Output
    let base = vec3<f32>(0.05, 0.07, 0.08);
    let smoke_color = vec3<f32>(0.12, 0.16, 0.17) * mist;
    var col = base + smoke_color;
    col *= (0.65 + 0.35 * vig);

    return vec4<f32>(col, 1.0);
}
"#;

/// Static glass overlay shader - rendered ONCE on init/resize
/// Outputs: RGB = additive color, A = UV distortion strength
const SHADER_GLASS_STATIC: &str = r#"
struct Uniforms {
    time: f32,
    zoom: f32,
    resolution: vec2<f32>,
    viewport_center: vec2<f32>,
    workspace_count: f32,
    active_workspace: f32,
    workspace_backgrounds: vec4<f32>,
    transitioning: f32,
    workspace_width: f32,
    workspace_height: f32,
    workspace_gap: f32,
    _pad: vec4<f32>,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
    var out: VsOut;
    let x = f32(i32(vertex_index & 1u) * 4 - 1);
    let y = f32(i32(vertex_index >> 1u) * 4 - 1);
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

fn hash21(p: vec2<f32>) -> f32 {
    let p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    let q = p3 + dot(p3, p3.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let asp = uniforms.resolution.x / uniforms.resolution.y;

    // Fresnel edge glow (static)
    let p = vec2<f32>((uv.x - 0.5) * asp, uv.y - 0.5);
    let r = length(p);
    let fres = smoothstep(0.35, 0.95, r);
    var overlay = vec3<f32>(0.03, 0.04, 0.05) * fres;

    // Static specular highlight (diagonal streak)
    let highlight_pos = 0.3; // Fixed position
    let sweep = smoothstep(0.04, 0.0, abs((uv.x + uv.y * 0.2) - highlight_pos));
    overlay += vec3<f32>(0.06, 0.07, 0.08) * sweep * 0.3;

    // Dust/grain (static)
    let dust = (hash21(floor(uv * uniforms.resolution * 0.4)) - 0.5) * 0.008;
    overlay += vec3<f32>(dust);

    // Store UV distortion in alpha (for refraction effect)
    let px = uv * uniforms.resolution;
    let distort = (hash21(floor(px * 0.5)) - 0.5) * 0.004;

    return vec4<f32>(overlay, distort);
}
"#;

/// Mist Pass 2: Composite smoke + static glass overlay
const SHADER_MIST_COMPOSITE: &str = r#"
struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(1) @binding(0) var smoke_tex: texture_2d<f32>;
@group(1) @binding(1) var smoke_samp: sampler;
@group(1) @binding(2) var glass_tex: texture_2d<f32>;
@group(1) @binding(3) var glass_samp: sampler;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
    var out: VsOut;
    let x = f32(i32(vertex_index & 1u) * 4 - 1);
    let y = f32(i32(vertex_index >> 1u) * 4 - 1);
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Sample static glass overlay (RGB = color, A = distortion)
    let glass = textureSample(glass_tex, glass_samp, in.uv);
    
    // Apply static UV distortion from glass alpha
    let distorted_uv = in.uv + vec2<f32>(glass.a, glass.a * 0.7);
    
    // Sample smoke with distortion
    let smoke = textureSample(smoke_tex, smoke_samp, distorted_uv).rgb;
    
    // Composite: smoke + glass overlay (additive)
    let final_color = smoke + glass.rgb;
    
    return vec4<f32>(clamp(final_color, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
"#;

// =============================================================================
// Renderer Implementation
// =============================================================================

/// Uniform data sent to shaders
/// NOTE: This struct must match WGSL alignment requirements!
/// Total struct size must be 80 bytes (padded to 16-byte boundary).
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    time: f32,                        // offset 0
    zoom: f32,                        // offset 4
    resolution: [f32; 2],             // offset 8
    viewport_center: [f32; 2],        // offset 16
    workspace_count: f32,             // offset 24
    active_workspace: f32,            // offset 28
    workspace_backgrounds: [f32; 4],  // offset 32
    transitioning: f32,               // offset 48
    workspace_width: f32,             // offset 52
    workspace_height: f32,            // offset 56
    workspace_gap: f32,               // offset 60
    _pad: [f32; 4],                   // offset 64 - padding to 80 bytes
}

/// Background renderer with multiple switchable shaders
pub struct BackgroundRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    surface_format: wgpu::TextureFormat,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    pipelines: HashMap<BackgroundType, wgpu::RenderPipeline>,
    current_background: BackgroundType,
    start_time: f64,
    // Viewport state for zoom effects
    viewport_zoom: f32,
    viewport_center: [f32; 2],
    // Workspace info for multi-workspace rendering
    workspace_count: f32,
    active_workspace: f32,
    workspace_backgrounds: [f32; 4],
    // Workspace layout dimensions (match Rust desktop engine)
    workspace_width: f32,
    workspace_height: f32,
    workspace_gap: f32,
    // Whether we're transitioning between workspaces
    transitioning: bool,
    // Smoke texture (half-res, animated)
    scene_texture: wgpu::Texture,
    scene_texture_view: wgpu::TextureView,
    scene_sampler: wgpu::Sampler,
    // Static glass overlay (full-res, rendered once)
    glass_overlay_texture: wgpu::Texture,
    glass_overlay_view: wgpu::TextureView,
    glass_static_pipeline: wgpu::RenderPipeline,
    // Composite pass resources
    composite_bind_group_layout: wgpu::BindGroupLayout,
    composite_bind_group: wgpu::BindGroup,
    composite_pipeline: wgpu::RenderPipeline,
}

impl BackgroundRenderer {
    /// Create a new background renderer
    pub async fn new(canvas: web_sys::HtmlCanvasElement) -> Result<Self, String> {
        let width = canvas.width();
        let height = canvas.height();

        // Create wgpu instance
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
            ..Default::default()
        });

        // Create surface from canvas
        #[cfg(target_arch = "wasm32")]
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| format!("Failed to create surface: {}", e))?;

        #[cfg(not(target_arch = "wasm32"))]
        let surface: wgpu::Surface<'static> = {
            return Err("BackgroundRenderer only supports WASM targets".to_string());
        };

        // Request adapter
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| "Failed to find suitable GPU adapter".to_string())?;

        // Request device
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Background Renderer Device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None,
            )
            .await
            .map_err(|e| format!("Failed to create device: {}", e))?;

        // Configure surface
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        // Create shared uniform buffer
        let uniforms = Uniforms {
            time: 0.0,
            zoom: 1.0,
            resolution: [width as f32, height as f32],
            viewport_center: [0.0, 0.0],
            workspace_count: 2.0,
            active_workspace: 0.0,
            workspace_backgrounds: [0.0, 0.0, 0.0, 0.0], // Default all to grain (0)
            transitioning: 0.0,
            workspace_width: 1920.0,
            workspace_height: 1080.0,
            workspace_gap: 100.0,
            _pad: [0.0, 0.0, 0.0, 0.0],
        };
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Background Uniform Buffer"),
            contents: bytemuck::cast_slice(&[uniforms]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // Create shared bind group layout (for uniforms)
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Background Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        // Create shared bind group
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Background Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // Create pipeline layout (shared for simple backgrounds)
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Background Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        // Create pipelines for all background types
        let mut pipelines = HashMap::new();
        for bg_type in BackgroundType::all() {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&format!("{:?} Shader", bg_type)),
                source: wgpu::ShaderSource::Wgsl(bg_type.shader_source().into()),
            });

            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(&format!("{:?} Pipeline", bg_type)),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

            pipelines.insert(*bg_type, pipeline);
        }

        // =====================================================================
        // Mist background resources
        // =====================================================================

        // Smoke texture at quarter resolution, capped at 480p max
        let scene_width = ((width / 4).max(1)).min(480);
        let scene_height = ((height / 4).max(1)).min(270);
        let scene_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Smoke Texture"),
            size: wgpu::Extent3d {
                width: scene_width,
                height: scene_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: surface_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let scene_texture_view = scene_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let scene_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Smoke Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Static glass overlay texture (full resolution, rendered once)
        let glass_overlay_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Glass Overlay Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: surface_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let glass_overlay_view =
            glass_overlay_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Pipeline for rendering static glass overlay (once)
        let glass_static_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Glass Static Shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_GLASS_STATIC.into()),
        });
        let glass_static_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Glass Static Pipeline"),
            layout: Some(&pipeline_layout), // Uses uniforms only
            vertex: wgpu::VertexState {
                module: &glass_static_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &glass_static_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Composite bind group layout (smoke + glass textures)
        let composite_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Composite Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let glass_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Glass Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let composite_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Composite Bind Group"),
            layout: &composite_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&scene_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&scene_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&glass_overlay_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&glass_sampler),
                },
            ],
        });

        // Composite pipeline
        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Composite Pipeline Layout"),
                bind_group_layouts: &[&bind_group_layout, &composite_bind_group_layout],
                push_constant_ranges: &[],
            });

        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Composite Shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_MIST_COMPOSITE.into()),
        });

        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Composite Pipeline"),
            layout: Some(&composite_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &composite_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &composite_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let start_time = js_sys::Date::now();

        let mut renderer = Self {
            device,
            queue,
            surface,
            surface_config,
            surface_format,
            bind_group_layout,
            bind_group,
            uniform_buffer,
            pipelines,
            current_background: BackgroundType::default(),
            start_time,
            viewport_zoom: 1.0,
            viewport_center: [0.0, 0.0],
            workspace_count: 2.0,
            active_workspace: 0.0,
            workspace_backgrounds: [0.0, 0.0, 0.0, 0.0],
            workspace_width: 1920.0,
            workspace_height: 1080.0,
            workspace_gap: 100.0,
            transitioning: false,
            scene_texture,
            scene_texture_view,
            scene_sampler,
            glass_overlay_texture,
            glass_overlay_view,
            glass_static_pipeline,
            composite_bind_group_layout,
            composite_bind_group,
            composite_pipeline,
        };

        // Render static glass overlay once
        renderer.render_static_glass();

        Ok(renderer)
    }

    /// Render the static glass overlay texture (called once on init and resize)
    fn render_static_glass(&mut self) {
        // Update uniforms with current resolution
        let uniforms = Uniforms {
            time: 0.0,
            zoom: 1.0,
            resolution: [
                self.surface_config.width as f32,
                self.surface_config.height as f32,
            ],
            viewport_center: [0.0, 0.0],
            workspace_count: self.workspace_count,
            active_workspace: self.active_workspace,
            workspace_backgrounds: self.workspace_backgrounds,
            transitioning: 0.0,
            workspace_width: self.workspace_width,
            workspace_height: self.workspace_height,
            workspace_gap: self.workspace_gap,
            _pad: [0.0, 0.0, 0.0, 0.0],
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Glass Static Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Glass Static Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.glass_overlay_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.glass_static_pipeline);
            render_pass.set_bind_group(0, &self.bind_group, &[]);
            render_pass.draw(0..3, 0..1);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
    }

    /// Get the current background type
    pub fn current_background(&self) -> BackgroundType {
        self.current_background
    }

    /// Set the background type (instant switch, no recompile needed)
    pub fn set_background(&mut self, bg_type: BackgroundType) {
        self.current_background = bg_type;
    }

    /// Resize the renderer
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.surface_config.width = width;
            self.surface_config.height = height;
            self.surface.configure(&self.device, &self.surface_config);

            // Recreate smoke texture at quarter resolution, capped at 480p max
            let scene_width = ((width / 4).max(1)).min(480);
            let scene_height = ((height / 4).max(1)).min(270);
            self.scene_texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Smoke Texture"),
                size: wgpu::Extent3d {
                    width: scene_width,
                    height: scene_height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.surface_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.scene_texture_view = self
                .scene_texture
                .create_view(&wgpu::TextureViewDescriptor::default());

            // Recreate glass overlay texture at full resolution
            self.glass_overlay_texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Glass Overlay Texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.surface_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.glass_overlay_view = self
                .glass_overlay_texture
                .create_view(&wgpu::TextureViewDescriptor::default());

            // Recreate composite bind group with new texture views
            let glass_sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Glass Sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Nearest,
                ..Default::default()
            });

            self.composite_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Composite Bind Group"),
                layout: &self.composite_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&self.scene_texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.scene_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&self.glass_overlay_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&glass_sampler),
                    },
                ],
            });

            // Re-render static glass overlay
            self.render_static_glass();
        }
    }

    /// Set viewport state for zoom effects
    pub fn set_viewport(&mut self, zoom: f32, center_x: f32, center_y: f32) {
        self.viewport_zoom = zoom;
        self.viewport_center = [center_x, center_y];
    }
    
    /// Set workspace layout dimensions (must match Rust desktop engine)
    pub fn set_workspace_dimensions(&mut self, width: f32, height: f32, gap: f32) {
        self.workspace_width = width;
        self.workspace_height = height;
        self.workspace_gap = gap;
    }
    
    /// Set workspace info for multi-workspace rendering when zoomed out
    pub fn set_workspace_info(&mut self, count: usize, active: usize, backgrounds: &[BackgroundType]) {
        self.workspace_count = count as f32;
        self.active_workspace = active as f32;
        // Convert background types to floats (0=grain, 1=mist)
        for (i, bg) in backgrounds.iter().take(4).enumerate() {
            self.workspace_backgrounds[i] = match bg {
                BackgroundType::Grain => 0.0,
                BackgroundType::Mist => 1.0,
            };
        }
    }
    
    /// Set whether we're transitioning between workspaces
    /// Only during transitions can you see other workspaces
    pub fn set_transitioning(&mut self, transitioning: bool) {
        self.transitioning = transitioning;
    }
    
    /// Set view mode for multi-workspace rendering
    /// 
    /// This controls when other workspaces are visible:
    /// - **Workspace mode**: Only show current workspace (no other workspaces visible)
    /// - **Void mode**: Show all workspaces
    /// - **Transitioning**: Show all workspaces (during animation)
    ///
    /// # Arguments
    /// * `in_void_or_transitioning` - True if in void mode or transitioning, false if in workspace
    pub fn set_view_mode(&mut self, in_void_or_transitioning: bool) {
        self.transitioning = in_void_or_transitioning;
    }
    
    /// Render a frame with the current background
    pub fn render(&mut self) -> Result<(), String> {
        // Calculate elapsed time
        let now = js_sys::Date::now();
        let elapsed = ((now - self.start_time) / 1000.0) as f32;

        // Update uniforms with viewport state
        let uniforms = Uniforms {
            time: elapsed,
            zoom: self.viewport_zoom,
            resolution: [
                self.surface_config.width as f32,
                self.surface_config.height as f32,
            ],
            viewport_center: self.viewport_center,
            workspace_count: self.workspace_count,
            active_workspace: self.active_workspace,
            workspace_backgrounds: self.workspace_backgrounds,
            transitioning: if self.transitioning { 1.0 } else { 0.0 },
            workspace_width: self.workspace_width,
            workspace_height: self.workspace_height,
            workspace_gap: self.workspace_gap,
            _pad: [0.0, 0.0, 0.0, 0.0],
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));

        // Get current texture, handling surface errors gracefully
        let output = match self.surface.get_current_texture() {
            Ok(texture) => texture,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                // Surface was lost or outdated (common during resize) - reconfigure and skip this frame
                self.surface.configure(&self.device, &self.surface_config);
                return Ok(()); // Skip this frame, next frame will work
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                return Err("Out of GPU memory".to_string());
            }
            Err(wgpu::SurfaceError::Timeout) => {
                // GPU is busy, skip this frame
                return Ok(());
            }
            Err(e) => {
                return Err(format!("Failed to get surface texture: {}", e));
            }
        };
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Create command encoder
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Background Encoder"),
            });

        // Use the fancy two-pass Mist renderer only when:
        // 1. Active workspace has Mist background
        // 2. Fully zoomed in (zoom >= 0.95)
        // 3. NOT transitioning between workspaces
        // During transitions, we need the Grain shader's multi-workspace rendering
        // which can display both grain and mist patterns for different workspaces.
        let use_mist_renderer = self.current_background == BackgroundType::Mist 
            && self.viewport_zoom >= 0.95
            && !self.transitioning;
        
        if use_mist_renderer {
            // ===============================================================
            // Two-pass rendering for Mist background (only when fully zoomed in)
            // ===============================================================

            // Pass 1: Render smoke to offscreen texture
            {
                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Mist Smoke Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.scene_texture_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });

                if let Some(pipeline) = self.pipelines.get(&BackgroundType::Mist) {
                    render_pass.set_pipeline(pipeline);
                    render_pass.set_bind_group(0, &self.bind_group, &[]);
                    render_pass.draw(0..3, 0..1);
                }
            }

            // Pass 2: Composite smoke + static glass overlay to screen
            {
                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Mist Composite Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });

                render_pass.set_pipeline(&self.composite_pipeline);
                render_pass.set_bind_group(0, &self.bind_group, &[]);
                render_pass.set_bind_group(1, &self.composite_bind_group, &[]);
                render_pass.draw(0..3, 0..1);
            }
        } else {
            // ===============================================================
            // Single-pass rendering using Grain shader
            // This shader handles multi-workspace rendering with both grain
            // and mist patterns, so it works correctly during transitions.
            // ===============================================================
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Background Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Always use Grain pipeline here - it has multi-workspace rendering
            // that can display both grain and mist background patterns
            if let Some(pipeline) = self.pipelines.get(&BackgroundType::Grain) {
                render_pass.set_pipeline(pipeline);
                render_pass.set_bind_group(0, &self.bind_group, &[]);
                render_pass.draw(0..3, 0..1);
            }
        }

        // Submit
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}
