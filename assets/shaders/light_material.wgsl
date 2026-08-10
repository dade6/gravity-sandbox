#define_import_path bevy_sprite::light_material

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct LightUniforms {
    light_pos: vec2<f32>,
    light_intensity: f32,
    light_color: vec3<f32>,
    ambient_strength: f32,
    base_color: vec4<f32>,
    body_pos: vec2<f32>,
    body_radius: f32,
    falloff: f32,
    has_normal_map: u32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> light: LightUniforms;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var normal_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var normal_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let frag_pos = in.world_position.xy;

    // Light direction (2D: XY only)
    let L = light.light_pos - frag_pos;
    let dist = length(L);
    let L_dir = L / max(dist, 0.001);

    // Radial direction from the body centre to this fragment: the XY of the
    // surface normal of a sphere seen face-on. With no normal map this alone
    // produces the flat radial gradient (side facing the star brighter); the
    // sampled normal-map relief is added on top when available.
    let rel = frag_pos - light.body_pos;
    let r = max(length(rel), 0.001);
    let radial = rel / r;

    var relief = vec2<f32>(0.0, 0.0);
    if light.has_normal_map == 1u {
        // The Circle mesh carries UVs (disc mapping: circle centre at (0.5,0.5),
        // rim at the texture edges). Fall back to polar coordinates around the
        // body centre when the mesh has no UV attribute, so the relief stays
        // coherent on the sphere.
#ifdef VERTEX_UVS
        let uv = in.uv;
#else
        let uv = vec2<f32>(0.5 + rel.x / (2.0 * max(light.body_radius, 0.001)), 0.5 - rel.y / (2.0 * max(light.body_radius, 0.001)));
#endif
        let encoded = textureSample(normal_map, normal_sampler, uv);
        // Decode tangent-space normal from [0,1] to [-1,1]; keep only the XY
        // perturbation (Z stays near 1 for a front-facing surface). Amplify
        // it so the procedural relief stays visible on small on-screen discs
        // (the 256px texture is filtered down to a few pixels).
        relief = (encoded.xyz * 2.0 - 1.0).xy * 2.0;
    }

    // Surface normal: sphere radial term + normal-map relief, Z front-facing.
    let normal = normalize(vec3<f32>(radial + relief, 1.0));

    // Diffuse: N·L (2D light direction projected on the sphere normal)
    let NdotL = max(dot(normal.xy, L_dir), 0.0);

    // Inverse-square attenuation, scaled by the star's falloff constant so the
    // brightness matches the world scale of the scene (bodies are 100-500 units
    // from their star; a bare 1/(1+d²) would leave them almost black).
    let attenuation = 1.0 / (1.0 + dist * dist * light.falloff);

    // Final colour: base_color * (ambient + diffuse)
    let ambient = light.ambient_strength;
    let diffuse = light.light_color * light.light_intensity * NdotL * attenuation;
    let lit = light.base_color.rgb * (ambient + diffuse);

    return vec4<f32>(lit, light.base_color.a);
}
