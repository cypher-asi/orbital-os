use std::collections::HashMap;
use wgpu::util::DeviceExt;

use super::shaders::SHADER_GLASS_STATIC;
use super::types::BackgroundType;
use super::uniforms::Uniforms;

/// Create wgpu device and adapter
pub async fn create_device(
    instance: &wgpu::Instance,
    surface: &wgpu::Surface<'static>,
) -> Result<(wgpu::Device, wgpu::Queue, wgpu::Adapter), String> {
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(surface),
            force_fallback_adapter: false,
        })
        .await
        .ok_or_else(|| "Failed to find suitable GPU adapter".to_string())?;

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

    Ok((device, queue, adapter))
}

/// Configure the surface
pub fn configure_surface(
    surface: &wgpu::Surface<'static>,
    adapter: &wgpu::Adapter,
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::SurfaceConfiguration, wgpu::TextureFormat) {
    let surface_caps = surface.get_capabilities(adapter);
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
    surface.configure(device, &surface_config);

    (surface_config, surface_format)
}

/// Create uniform buffer and bind group
pub fn create_uniform_resources(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (
    wgpu::Buffer,
    wgpu::BindGroupLayout,
    wgpu::BindGroup,
) {
    let uniforms = Uniforms::default_with_resolution(width, height);
    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Background Uniform Buffer"),
        contents: bytemuck::cast_slice(&[uniforms]),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

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

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Background Bind Group"),
        layout: &bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buffer.as_entire_binding(),
        }],
    });

    (uniform_buffer, bind_group_layout, bind_group)
}

/// Create render pipelines for all background types
pub fn create_pipelines(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    surface_format: wgpu::TextureFormat,
) -> HashMap<BackgroundType, wgpu::RenderPipeline> {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Background Pipeline Layout"),
        bind_group_layouts: &[bind_group_layout],
        push_constant_ranges: &[],
    });

    let mut pipelines = HashMap::new();
    for bg_type in BackgroundType::all() {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&format!("{:?} Shader", bg_type)),
            source: wgpu::ShaderSource::Wgsl(bg_type.shader_source().into()),
        });

        let pipeline = create_single_pipeline(
            device,
            &pipeline_layout,
            &shader,
            surface_format,
            &format!("{:?} Pipeline", bg_type),
        );

        pipelines.insert(*bg_type, pipeline);
    }

    pipelines
}

/// Create a single render pipeline
fn create_single_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    label: &str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
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
    })
}

/// Create smoke texture for mist effect
pub fn create_smoke_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> (wgpu::Texture, wgpu::TextureView, wgpu::Sampler) {
    let scene_width = (width / 4).clamp(1, 480);
    let scene_height = (height / 4).clamp(1, 270);
    
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Smoke Texture"),
        size: wgpu::Extent3d {
            width: scene_width,
            height: scene_height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("Smoke Sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });

    (texture, view, sampler)
}

/// Create glass overlay texture
pub fn create_glass_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Glass Overlay Texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    
    (texture, view)
}

/// Create glass static rendering pipeline
pub fn create_glass_static_pipeline(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Glass Static Pipeline Layout"),
        bind_group_layouts: &[bind_group_layout],
        push_constant_ranges: &[],
    });

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Glass Static Shader"),
        source: wgpu::ShaderSource::Wgsl(SHADER_GLASS_STATIC.into()),
    });

    create_single_pipeline(
        device,
        &pipeline_layout,
        &shader,
        format,
        "Glass Static Pipeline",
    )
}

/// Create composite bind group layout
pub fn create_composite_bind_group_layout(
    device: &wgpu::Device,
) -> wgpu::BindGroupLayout {
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
    })
}

/// Create composite bind group
pub fn create_composite_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    smoke_view: &wgpu::TextureView,
    smoke_sampler: &wgpu::Sampler,
    glass_view: &wgpu::TextureView,
) -> wgpu::BindGroup {
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

    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Composite Bind Group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(smoke_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(smoke_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(glass_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&glass_sampler),
            },
        ],
    })
}

/// Create composite pipeline
pub fn create_composite_pipeline(
    device: &wgpu::Device,
    uniform_layout: &wgpu::BindGroupLayout,
    composite_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    use super::shaders::SHADER_MIST_COMPOSITE;
    
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Composite Pipeline Layout"),
        bind_group_layouts: &[uniform_layout, composite_layout],
        push_constant_ranges: &[],
    });

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Composite Shader"),
        source: wgpu::ShaderSource::Wgsl(SHADER_MIST_COMPOSITE.into()),
    });

    create_single_pipeline(
        device,
        &pipeline_layout,
        &shader,
        format,
        "Composite Pipeline",
    )
}
