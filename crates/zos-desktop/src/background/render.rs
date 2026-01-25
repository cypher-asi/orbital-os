use super::types::BackgroundType;

/// Determine if we should use the two-pass mist renderer
pub fn should_use_mist_renderer(
    current_background: BackgroundType,
    viewport_zoom: f32,
    transitioning: bool,
) -> bool {
    current_background == BackgroundType::Mist && viewport_zoom >= 0.95 && !transitioning
}

/// Render mist smoke pass to offscreen texture
pub fn render_mist_smoke_pass(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
    scene_texture_view: &wgpu::TextureView,
) {
    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Mist Smoke Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: scene_texture_view,
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

    render_pass.set_pipeline(pipeline);
    render_pass.set_bind_group(0, bind_group, &[]);
    render_pass.draw(0..3, 0..1);
}

/// Render composite pass (smoke + glass)
pub fn render_composite_pass(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    uniform_bind_group: &wgpu::BindGroup,
    composite_bind_group: &wgpu::BindGroup,
    output_view: &wgpu::TextureView,
) {
    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Mist Composite Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: output_view,
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

    render_pass.set_pipeline(pipeline);
    render_pass.set_bind_group(0, uniform_bind_group, &[]);
    render_pass.set_bind_group(1, composite_bind_group, &[]);
    render_pass.draw(0..3, 0..1);
}

/// Render single pass using grain shader
pub fn render_single_pass(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
    output_view: &wgpu::TextureView,
) {
    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Background Render Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: output_view,
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

    render_pass.set_pipeline(pipeline);
    render_pass.set_bind_group(0, bind_group, &[]);
    render_pass.draw(0..3, 0..1);
}

/// Render static glass overlay
pub fn render_glass_static(
    queue: &wgpu::Queue,
    device: &wgpu::Device,
    uniform_buffer: &wgpu::Buffer,
    glass_overlay_view: &wgpu::TextureView,
    glass_static_pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
    uniforms: &super::uniforms::Uniforms,
) {
    queue.write_buffer(uniform_buffer, 0, bytemuck::cast_slice(&[*uniforms]));

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Glass Static Encoder"),
    });

    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Glass Static Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: glass_overlay_view,
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

        render_pass.set_pipeline(glass_static_pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }

    queue.submit(std::iter::once(encoder.finish()));
}
