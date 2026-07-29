// Gravity Sandbox — 2D Lighting Shader
// Custom Material2d fragment shader for per-pixel 2D lighting.
// Compatible with WebGL2 backend via naga GLSL translation.
//
// Uniform bindings (from AsBindGroup):
//   @uniform(0) light_pos: vec2<f32>
//   @uniform(1) light_intensity: f32
//   @uniform(2) light_color: vec3<f32>
//   @uniform(3) ambient_strength: f32
//   @texture(4)  normal_map (Option<Handle<Image>>)
//   @sampler(5)  normal_map sampler

#import bevy_sprite::material2d_bindings

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) world_position: vec4<f32>,
    @location(2) clip: vec4<f32>,
};

// Detect whether the normal-map texture is the default 1×1 white fallback
// that Bevy binds when `Option<Handle<Image>>` is `None`.
// When it is the fallback we use a front-facing normal (0.0, 0.0, 1.0)
// to produce a flat radial gradient instead of directional shading.
fn is_default_fallback(sample: vec4<f32>) -> bool {
    return all(sample == vec4<f32>(1.0, 1.0, 1.0, 1.0));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // ── Base colour from sprite texture ──────────────────────────
    var base_color: vec4<f32> = textureSample(base_color_texture, base_color_sampler, in.uv);

    // Discard fully transparent fragments.
    if base_color.a < 0.01 {
        discard;
    }

    // ── Light direction & distance ───────────────────────────────
    let to_light: vec2<f32> = material.light_pos - in.world_position.xy;
    let distance_sq: f32 = dot(to_light, to_light);
    let distance: f32 = sqrt(distance_sq);

    // ── Distance attenuation ─────────────────────────────────────
    // Formula: attenuation = intensity / (1.0 + dist² × 1/falloff)
    // Lower falloff → further reach (less aggressive decay).
    let inv_falloff: f32 = 1.0 / max(material.light_falloff, 0.0001);
    let attenuation: f32 = material.light_intensity / (1.0 + distance_sq * inv_falloff);

    // ── Normal mapping / flat gradient shading ───────────────────
    var diffuse: f32 = 1.0;          // default: flat radial gradient

    // Sample normal-map texture (or Bevy's 1×1 white fallback if None).
    let normal_sample: vec4<f32> = textureSample(normal_map_texture, normal_map_sampler, in.uv);
    if !is_default_fallback(normal_sample) {
        // Decode normal from [0, 1] to [-1, 1] range and normalise.
        let normal: vec3<f32> = normalize(normal_sample.xyz * 2.0 - 1.0);
        let light_dir_3d: vec3<f32> = normalize(vec3<f32>(to_light, 0.0));
        diffuse = max(dot(normal, light_dir_3d), 0.0);
    }

    // ── Ambient light ────────────────────────────────────────────
    // Always at least 12 % of base colour (hard-coded minimum).
    let ambient: vec3<f32> = max(material.ambient_strength, 0.12) * base_color.rgb;

    // ── Final colour ─────────────────────────────────────────────
    let light_contrib: vec3<f32> = material.light_color * diffuse * attenuation;
    let final_color: vec3<f32> = base_color.rgb * (ambient + light_contrib);

    return vec4<f32>(final_color, base_color.a);
}
