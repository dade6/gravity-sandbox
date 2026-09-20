// curve_line.wgsl — fragment shader for Catmull-Rom dashed lines
//
// The vertex pipeline (provided by Bevy's Mesh2d) outputs:
//   @builtin(position) position : vec4<f32>
//   @location(0) uv            : vec2<f32>   // x = arc-length, y = 0|1 (edge)
//   @location(1) color         : vec4<f32>   // per-vertex RGBA
//
// The material uniform block carries dashing parameters.

#define_import_path bevy_sprite::curve_line

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct CurveLineUniforms {
    dash_length: f32,   // world-space length of one dash+gap cycle
    dash_ratio:  f32,   // fraction of the cycle drawn (0.0–1.0)
    _pad0:       f32,
    _pad1:       f32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: CurveLineUniforms;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let arc = in.uv.x;
    let edge = in.uv.y;

    // --- dashing ---
    // fract gives position inside the current cycle (0..1)
    let cycle_pos = fract(arc / max(material.dash_length, 0.001));
    // inside the drawn portion?
    let in_dash = select(0.0, 1.0, cycle_pos <= material.dash_ratio);

    // --- edge anti-aliasing (softens the 0|1 boundary of the quad) ---
    let aa = smoothstep(0.0, 0.15, edge) * smoothstep(1.0, 0.85, edge);

    let alpha = in_dash * aa * in.color.a;
    // Avoid fully-transparent fragments that still write to the depth buffer
    if alpha < 0.001 { discard; }

    return vec4<f32>(in.color.rgb, alpha);
}
