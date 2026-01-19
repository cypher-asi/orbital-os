# Rendering

**Component:** 08-desktop/05-rendering  
**Status:** Specification

---

## Overview

This document specifies WebGPU rendering requirements for the desktop compositor. The renderer must efficiently composite windows, chrome, effects, and Void mode portals.

---

## Rendering Outputs

The compositor must render:

| Element | Description |
|---------|-------------|
| Desktop background | Solid, gradient, or procedural (see [13-backgrounds.md](13-backgrounds.md)) |
| Window shadows | Soft drop shadows with configurable radius |
| Window chrome | Rounded corners, title bar, border |
| Window content placeholder | Gray rect when content is React DOM |
| Void portals | Desktop previews as textured quads |
| Effects (optional) | Blur, glass effects behind feature flags |

---

## Renderer Structure

```rust
/// Renderer
pub struct Renderer {
    /// Render pipelines
    pipelines: Pipelines,
    
    /// Shared GPU resources
    resources: RenderResources,
    
    /// Vertex/index buffers
    buffers: RenderBuffers,
    
    /// Current DPI scale
    dpi: f32,
    
    /// Screen size
    screen_size: (u32, u32),
}

struct Pipelines {
    /// Background rendering
    background: wgpu::RenderPipeline,
    
    /// Window quad rendering (shadows + chrome)
    window: wgpu::RenderPipeline,
    
    /// Texture rendering (content, previews)
    texture: wgpu::RenderPipeline,
    
    /// Optional blur pipeline
    #[cfg(feature = "postfx")]
    blur: wgpu::RenderPipeline,
}

struct RenderResources {
    /// Sampler for textures
    sampler: wgpu::Sampler,
    
    /// Bind group layouts
    layouts: BindGroupLayouts,
    
    /// Uniform buffer for per-frame data
    uniforms: wgpu::Buffer,
}
```

---

## Window Rendering

### Window Batch

Windows are rendered in a single batched draw call where possible:

```rust
/// Per-window instance data
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct WindowInstance {
    /// Position in view space (after origin rebasing)
    pub position: [f32; 2],
    
    /// Size in pixels
    pub size: [f32; 2],
    
    /// Corner radius
    pub corner_radius: f32,
    
    /// Shadow radius
    pub shadow_radius: f32,
    
    /// Shadow opacity
    pub shadow_opacity: f32,
    
    /// Is focused (affects chrome color)
    pub focused: f32,
    
    /// Title bar height
    pub title_bar_height: f32,
    
    /// Padding
    pub _pad: [f32; 3],
}

impl Renderer {
    /// Render windows on a desktop
    fn render_windows(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        windows: &[&Window],
        camera: &Camera,
        config: &ChromeConfig,
    ) {
        // Build instance data
        let origin = camera.center;
        let instances: Vec<_> = windows.iter()
            .filter(|w| self.is_visible(w, camera))
            .map(|w| self.build_window_instance(w, origin, config))
            .collect();
        
        if instances.is_empty() {
            return;
        }
        
        // Upload instance data
        self.buffers.upload_instances(&instances);
        
        // Render pass
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("windows"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        
        pass.set_pipeline(&self.pipelines.window);
        pass.set_bind_group(0, &self.resources.uniforms_bind_group, &[]);
        pass.set_vertex_buffer(0, self.buffers.quad_vertices.slice(..));
        pass.set_vertex_buffer(1, self.buffers.instances.slice(..));
        pass.set_index_buffer(
            self.buffers.quad_indices.slice(..),
            wgpu::IndexFormat::Uint16,
        );
        pass.draw_indexed(0..6, 0, 0..instances.len() as u32);
    }
    
    fn build_window_instance(
        &self,
        window: &Window,
        origin: (f64, f64),
        config: &ChromeConfig,
    ) -> WindowInstance {
        WindowInstance {
            position: [
                (window.position.0 - origin.0) as f32,
                (window.position.1 - origin.1) as f32,
            ],
            size: [window.size.0, window.size.1],
            corner_radius: config.corner_radius,
            shadow_radius: config.shadow_radius,
            shadow_opacity: config.shadow_opacity,
            focused: if window.focused { 1.0 } else { 0.0 },
            title_bar_height: config.title_bar_height,
            _pad: [0.0; 3],
        }
    }
}
```

---

## Background Rendering

For procedural backgrounds (Grain, Mist), see [13-backgrounds.md](13-backgrounds.md) for full specification.

```rust
impl Renderer {
    fn render_background(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        background: &Background,
    ) {
        match background {
            Background::Solid { color } => {
                // Clear with solid color
                let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("background"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: color.0 as f64,
                                g: color.1 as f64,
                                b: color.2 as f64,
                                a: color.3 as f64,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
            }
            Background::Gradient { stops } => {
                self.render_gradient(encoder, target, stops);
            }
            Background::Procedural(procedural) => {
                // Delegate to BackgroundRenderer for animated procedural backgrounds
                self.background_renderer.render(encoder, target, procedural);
            }
        }
    }
    
    fn render_gradient(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        stops: &[GradientStop],
    ) {
        // Full-screen quad with gradient shader
        // Implementation details omitted for brevity
    }
}
```

---

## Void Mode Rendering

```rust
impl Renderer {
    fn render_void(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        state: &CompositorState,
        portals: &[VoidPortal],
    ) {
        // Clear background
        self.render_background(encoder, target, &Background::default());
        
        // Render portal frames and previews
        for portal in portals {
            self.render_portal(encoder, target, state, portal);
        }
    }
    
    fn render_portal(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        state: &CompositorState,
        portal: &VoidPortal,
    ) {
        let desktop = match state.desktops.get(&portal.desktop_id) {
            Some(d) => d,
            None => return,
        };
        
        // Render portal frame (border, optional selection highlight)
        self.render_portal_frame(encoder, target, portal, state.void_selection);
        
        // Render desktop preview texture (if available)
        if let Some(preview) = &desktop.preview {
            self.render_texture(
                encoder,
                target,
                &preview.view,
                portal.rect,
                &state.void_camera,
            );
        }
    }
}
```

---

## Culling

```rust
impl Renderer {
    /// Check if window is visible in viewport
    fn is_visible(&self, window: &Window, camera: &Camera) -> bool {
        let screen_size = (
            self.screen_size.0 as f32,
            self.screen_size.1 as f32,
        );
        let visible_rect = camera.visible_rect(screen_size);
        let margin = self.config.culling.margin;
        
        // Expand visible rect by margin
        let expanded = WorldRect {
            x: visible_rect.x - margin as f64,
            y: visible_rect.y - margin as f64,
            width: visible_rect.width + margin * 2.0,
            height: visible_rect.height + margin * 2.0,
        };
        
        // Check intersection
        let window_rect = WorldRect {
            x: window.position.0,
            y: window.position.1,
            width: window.size.0,
            height: window.size.1,
        };
        
        expanded.intersects(&window_rect)
    }
}
```

---

## Performance Targets

| Scenario | Target |
|----------|--------|
| 50 visible windows on active desktop | 60 fps |
| 4 desktops in Void with previews | 60 fps |
| Smooth zoom/pan transitions | 60 fps |

### Optimization Strategies

1. **Instanced rendering**: All windows drawn in single draw call
2. **Culling**: Skip windows outside viewport
3. **Lazy preview updates**: Only update dirty desktop previews
4. **Preview budget**: Limit preview renders per frame
5. **Origin rebasing**: Stable f32 coords at any zoom level

---

## Coordinate Precision

### Origin Rebasing

```rust
impl Renderer {
    /// Uniforms for current frame
    fn update_uniforms(&mut self, camera: &Camera) {
        let origin = camera.center;
        
        let uniforms = FrameUniforms {
            // View matrix with rebased origin
            view_proj: self.calculate_view_proj(camera, origin),
            
            // Screen size for coordinate conversion
            screen_size: [
                self.screen_size.0 as f32,
                self.screen_size.1 as f32,
            ],
            
            // Zoom level
            zoom: camera.zoom,
            
            // DPI scale
            dpi: self.dpi,
        };
        
        self.gpu.queue.write_buffer(
            &self.resources.uniforms,
            0,
            bytemuck::bytes_of(&uniforms),
        );
    }
    
    fn calculate_view_proj(
        &self,
        camera: &Camera,
        origin: (f64, f64),
    ) -> [[f32; 4]; 4] {
        // Orthographic projection
        // Origin is at (0, 0) in view space
        // All world positions are offset by -origin before rendering
        let half_w = self.screen_size.0 as f32 / 2.0 / camera.zoom;
        let half_h = self.screen_size.1 as f32 / 2.0 / camera.zoom;
        
        orthographic_projection(-half_w, half_w, -half_h, half_h)
    }
}
```

---

## Shaders

### Window Shader (Conceptual)

```wgsl
struct FrameUniforms {
    view_proj: mat4x4<f32>,
    screen_size: vec2<f32>,
    zoom: f32,
    dpi: f32,
}

struct WindowInstance {
    @location(1) position: vec2<f32>,
    @location(2) size: vec2<f32>,
    @location(3) corner_radius: f32,
    @location(4) shadow_radius: f32,
    @location(5) shadow_opacity: f32,
    @location(6) focused: f32,
    @location(7) title_bar_height: f32,
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // SDF for rounded rectangle
    let d = sdf_rounded_rect(in.local_pos, in.size, in.corner_radius);
    
    // Shadow
    let shadow = smoothstep(in.shadow_radius, 0.0, d) * in.shadow_opacity;
    
    // Window body
    let body = smoothstep(1.0, 0.0, d);
    
    // Title bar
    let in_title = step(in.local_pos.y, in.title_bar_height);
    let title_color = mix(CHROME_INACTIVE, CHROME_ACTIVE, in.focused);
    let body_color = mix(CONTENT_BG, title_color, in_title);
    
    // Combine
    return vec4(body_color.rgb, body + shadow * (1.0 - body));
}
```

---

## Module Structure

```
render/
├── mod.rs          # Renderer struct, public interface
├── pipeline.rs     # Pipeline creation
├── buffers.rs      # Buffer management
├── window.rs       # Window rendering
├── background.rs   # Background rendering
├── void.rs         # Void/portal rendering
└── shaders/
    ├── window.wgsl
    ├── background.wgsl
    └── texture.wgsl
```

---

*[Back to Desktop](README.md) | [Previous: Windows](04-windows.md) | [Next: Input](06-input.md)*
