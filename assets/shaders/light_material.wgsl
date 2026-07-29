#define_import_path bevy_sprite::light_material

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct LightUniforms {
    light_pos: vec2<f32>,
    light_intensity: f32,
    light_color: vec3<f32>,
    ambient_strength: f32,
}

@group(1) @binding(0) var<uniform> light: LightUniforms;
@group(1) @binding(1) var normal_map: texture_2d<f32>;
@group(1) @binding(2) var normal_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample normal map (or default to flat normal if texture is degenerate)
    let encoded = textureSample(normal_map, normal_sampler, in.uv);
    // Decode tangent-space normal from [0,1] to [-1,1]
    let normal = normalize(encoded.xyz * 2.0 - 1.0);

    // Light direction (2D: XY only)
    let frag_pos = in.world_position.xy;
    let L = light.light_pos - frag_pos;
    let dist = length(L);
    let L_dir = L / max(dist, 0.001);

    // Diffuse: N·L (use XY of normal; Z adds rim contribution)
    let NdotL = max(dot(normal.xy, L_dir), 0.0);

    // Inverse-square attenuation
    let attenuation = 1.0 / (1.0 + dist * dist);

    // Final colour: ambient + diffuse
    let ambient = light.ambient_strength;
    let diffuse = light.light_color * light.light_intensity * NdotL * attenuation;

    return vec4<f32>(ambient + diffuse, 1.0);
}
