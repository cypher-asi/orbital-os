use std::collections::HashMap;

use super::init::*;
use super::render::*;
use super::types::BackgroundType;
use super::uniforms::Uniforms;

/// Intermediate struct for GPU resources during initialization
struct GpuResources {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,
    surface_format: wgpu::TextureFormat,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
}

/// Intermediate struct for render resources during initialization
struct RenderResources {
    pipelines: HashMap<BackgroundType, wgpu::RenderPipeline>,
    scene_texture: wgpu::Texture,
    scene_texture_view: wgpu::TextureView,
    scene_sampler: wgpu::Sampler,
    glass_overlay_texture: wgpu::Texture,
    glass_overlay_view: wgpu::TextureView,
    glass_static_pipeline: wgpu::RenderPipeline,
    composite_bind_group_layout: wgpu::BindGroupLayout,
    composite_bind_group: wgpu::BindGroup,
    composite_pipeline: wgpu::RenderPipeline,
}

/// Background renderer with multiple switchable shaders
pub struct BackgroundRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    surface_format: wgpu::TextureFormat,
    #[allow(dead_code)]
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    pipelines: HashMap<BackgroundType, wgpu::RenderPipeline>,
    current_background: BackgroundType,
    start_time: f64,
    viewport_zoom: f32,
    viewport_center: [f32; 2],
    workspace_count: f32,
    active_workspace: f32,
    workspace_backgrounds: [f32; 4],
    workspace_width: f32,
    workspace_height: f32,
    workspace_gap: f32,
    transitioning: bool,
    scene_texture: wgpu::Texture,
    scene_texture_view: wgpu::TextureView,
    scene_sampler: wgpu::Sampler,
    glass_overlay_texture: wgpu::Texture,
    glass_overlay_view: wgpu::TextureView,
    glass_static_pipeline: wgpu::RenderPipeline,
    composite_bind_group_layout: wgpu::BindGroupLayout,
    composite_bind_group: wgpu::BindGroup,
    composite_pipeline: wgpu::RenderPipeline,
}

impl BackgroundRenderer {
    /// Create a new background renderer
    pub async fn new(canvas: web_sys::HtmlCanvasElement) -> Result<Self, String> {
        let (instance, surface, width, height) = Self::create_surface(canvas)?;
        let gpu = Self::setup_gpu(&instance, &surface, width, height).await?;
        let resources = Self::setup_render_resources(&gpu, width, height);
        
        let mut renderer = Self::assemble(surface, gpu, resources);
        renderer.render_static_glass();
        
        Ok(renderer)
    }

    /// Create the wgpu instance and surface from canvas
    #[cfg(target_arch = "wasm32")]
    fn create_surface(canvas: web_sys::HtmlCanvasElement) -> Result<(wgpu::Instance, wgpu::Surface<'static>, u32, u32), String> {
        let width = canvas.width();
        let height = canvas.height();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
            ..Default::default()
        });

        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| format!("Failed to create surface: {}", e))?;

        Ok((instance, surface, width, height))
    }

    /// Create the wgpu instance and surface from canvas (non-WASM stub)
    #[cfg(not(target_arch = "wasm32"))]
    fn create_surface(_canvas: web_sys::HtmlCanvasElement) -> Result<(wgpu::Instance, wgpu::Surface<'static>, u32, u32), String> {
        Err("BackgroundRenderer only supports WASM targets".to_string())
    }

    /// Setup GPU device, queue, and surface configuration
    async fn setup_gpu(
        instance: &wgpu::Instance,
        surface: &wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> Result<GpuResources, String> {
        let (device, queue, adapter) = create_device(instance, surface).await?;
        let (surface_config, surface_format) = configure_surface(surface, &adapter, &device, width, height);
        let (uniform_buffer, bind_group_layout, bind_group) = create_uniform_resources(&device, width, height);
        
        Ok(GpuResources {
            device,
            queue,
            surface_config,
            surface_format,
            bind_group_layout,
            bind_group,
            uniform_buffer,
        })
    }

    /// Setup rendering resources (pipelines, textures)
    fn setup_render_resources(gpu: &GpuResources, width: u32, height: u32) -> RenderResources {
        let pipelines = create_pipelines(&gpu.device, &gpu.bind_group_layout, gpu.surface_format);
        
        let (scene_texture, scene_texture_view, scene_sampler) =
            create_smoke_texture(&gpu.device, width, height, gpu.surface_format);
        
        let (glass_overlay_texture, glass_overlay_view) =
            create_glass_texture(&gpu.device, width, height, gpu.surface_format);
        
        let glass_static_pipeline =
            create_glass_static_pipeline(&gpu.device, &gpu.bind_group_layout, gpu.surface_format);
        
        let composite_bind_group_layout = create_composite_bind_group_layout(&gpu.device);
        
        let composite_bind_group = create_composite_bind_group(
            &gpu.device,
            &composite_bind_group_layout,
            &scene_texture_view,
            &scene_sampler,
            &glass_overlay_view,
        );
        
        let composite_pipeline = create_composite_pipeline(
            &gpu.device,
            &gpu.bind_group_layout,
            &composite_bind_group_layout,
            gpu.surface_format,
        );

        RenderResources {
            pipelines,
            scene_texture,
            scene_texture_view,
            scene_sampler,
            glass_overlay_texture,
            glass_overlay_view,
            glass_static_pipeline,
            composite_bind_group_layout,
            composite_bind_group,
            composite_pipeline,
        }
    }

    /// Assemble the final renderer from surface, GPU and render resources
    fn assemble(
        surface: wgpu::Surface<'static>,
        gpu: GpuResources,
        resources: RenderResources,
    ) -> Self {
        Self {
            device: gpu.device,
            queue: gpu.queue,
            surface,
            surface_config: gpu.surface_config,
            surface_format: gpu.surface_format,
            bind_group_layout: gpu.bind_group_layout,
            bind_group: gpu.bind_group,
            uniform_buffer: gpu.uniform_buffer,
            pipelines: resources.pipelines,
            current_background: BackgroundType::default(),
            start_time: js_sys::Date::now(),
            viewport_zoom: 1.0,
            viewport_center: [0.0, 0.0],
            workspace_count: 2.0,
            active_workspace: 0.0,
            workspace_backgrounds: [0.0, 0.0, 0.0, 0.0],
            workspace_width: 1920.0,
            workspace_height: 1080.0,
            workspace_gap: 100.0,
            transitioning: false,
            scene_texture: resources.scene_texture,
            scene_texture_view: resources.scene_texture_view,
            scene_sampler: resources.scene_sampler,
            glass_overlay_texture: resources.glass_overlay_texture,
            glass_overlay_view: resources.glass_overlay_view,
            glass_static_pipeline: resources.glass_static_pipeline,
            composite_bind_group_layout: resources.composite_bind_group_layout,
            composite_bind_group: resources.composite_bind_group,
            composite_pipeline: resources.composite_pipeline,
        }
    }

    /// Render the static glass overlay texture
    fn render_static_glass(&mut self) {
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
        
        render_glass_static(
            &self.queue,
            &self.device,
            &self.uniform_buffer,
            &self.glass_overlay_view,
            &self.glass_static_pipeline,
            &self.bind_group,
            &uniforms,
        );
    }

    /// Get the current background type
    pub fn current_background(&self) -> BackgroundType {
        self.current_background
    }

    /// Set the background type
    pub fn set_background(&mut self, bg_type: BackgroundType) {
        self.current_background = bg_type;
    }

    /// Resize the renderer
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }

        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);

        self.recreate_smoke_texture(width, height);
        self.recreate_glass_texture(width, height);
        self.recreate_composite_bind_group();
        self.render_static_glass();
    }

    /// Recreate smoke texture
    fn recreate_smoke_texture(&mut self, width: u32, height: u32) {
        let (texture, view, sampler) =
            create_smoke_texture(&self.device, width, height, self.surface_format);
        
        self.scene_texture = texture;
        self.scene_texture_view = view;
        self.scene_sampler = sampler;
    }

    /// Recreate glass texture
    fn recreate_glass_texture(&mut self, width: u32, height: u32) {
        let (texture, view) =
            create_glass_texture(&self.device, width, height, self.surface_format);
        
        self.glass_overlay_texture = texture;
        self.glass_overlay_view = view;
    }

    /// Recreate composite bind group
    fn recreate_composite_bind_group(&mut self) {
        self.composite_bind_group = create_composite_bind_group(
            &self.device,
            &self.composite_bind_group_layout,
            &self.scene_texture_view,
            &self.scene_sampler,
            &self.glass_overlay_view,
        );
    }

    /// Set viewport state for zoom effects
    pub fn set_viewport(&mut self, zoom: f32, center_x: f32, center_y: f32) {
        self.viewport_zoom = zoom;
        self.viewport_center = [center_x, center_y];
    }

    /// Set workspace layout dimensions
    pub fn set_workspace_dimensions(&mut self, width: f32, height: f32, gap: f32) {
        self.workspace_width = width;
        self.workspace_height = height;
        self.workspace_gap = gap;
    }

    /// Set workspace info for multi-workspace rendering
    pub fn set_workspace_info(
        &mut self,
        count: usize,
        active: usize,
        backgrounds: &[BackgroundType],
    ) {
        self.workspace_count = count as f32;
        self.active_workspace = active as f32;
        
        for (i, bg) in backgrounds.iter().take(4).enumerate() {
            self.workspace_backgrounds[i] = match bg {
                BackgroundType::Grain => 0.0,
                BackgroundType::Mist => 1.0,
            };
        }
    }

    /// Set whether we're transitioning between workspaces
    pub fn set_transitioning(&mut self, transitioning: bool) {
        self.transitioning = transitioning;
    }

    /// Set view mode for multi-workspace rendering
    pub fn set_view_mode(&mut self, in_void_or_transitioning: bool) {
        self.transitioning = in_void_or_transitioning;
    }

    /// Render a frame with the current background
    pub fn render(&mut self) -> Result<(), String> {
        let now = js_sys::Date::now();
        let elapsed = ((now - self.start_time) / 1000.0) as f32;

        let uniforms = self.build_uniforms(elapsed);
        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[uniforms]),
        );

        let output = self.get_surface_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Background Encoder"),
            });

        let use_mist = should_use_mist_renderer(
            self.current_background,
            self.viewport_zoom,
            self.transitioning,
        );

        if use_mist {
            self.render_mist_two_pass(&mut encoder, &view);
        } else {
            self.render_grain_single_pass(&mut encoder, &view);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    /// Build uniforms from current state
    fn build_uniforms(&self, elapsed: f32) -> Uniforms {
        Uniforms {
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
        }
    }

    /// Get surface texture with error handling
    fn get_surface_texture(&mut self) -> Result<wgpu::SurfaceTexture, String> {
        match self.surface.get_current_texture() {
            Ok(texture) => Ok(texture),
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.surface_config);
                Err("Surface reconfigured, skip frame".to_string())
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                Err("Out of GPU memory".to_string())
            }
            Err(wgpu::SurfaceError::Timeout) => {
                Err("GPU timeout, skip frame".to_string())
            }
        }
    }

    /// Render mist using two-pass approach
    fn render_mist_two_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        output_view: &wgpu::TextureView,
    ) {
        if let Some(pipeline) = self.pipelines.get(&BackgroundType::Mist) {
            render_mist_smoke_pass(
                encoder,
                pipeline,
                &self.bind_group,
                &self.scene_texture_view,
            );
        }

        render_composite_pass(
            encoder,
            &self.composite_pipeline,
            &self.bind_group,
            &self.composite_bind_group,
            output_view,
        );
    }

    /// Render using grain single-pass approach
    fn render_grain_single_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        output_view: &wgpu::TextureView,
    ) {
        if let Some(pipeline) = self.pipelines.get(&BackgroundType::Grain) {
            render_single_pass(encoder, pipeline, &self.bind_group, output_view);
        }
    }
}
