/// Shared vertex shader for fullscreen triangle
#[allow(dead_code)]
pub const VERTEX_SHADER: &str = r#"
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
