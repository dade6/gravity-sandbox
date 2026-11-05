// Stub per il gate `create_lightmap_shader_body_is_valid_wgsl`.
//
// Il test estrae la funzione `fragment` da
// `vendor/bevy_firefly/src/shaders/create_lightmap.wgsl`, toglie i blocchi
// `#ifdef` (che risolve il preprocessore di Bevy, non naga) e la incolla in
// coda a questo file, poi la valida con naga.
//
// Serve a intercettare gli errori WGSL che `cargo check` NON vede (la shader è
// compilata a RUNTIME dalla GPU) e che a schermo si manifestano come SCHERMO
// NERO + UI ASSENTE — il caso della v0.14.207, dove `normal.rgb != vec3f(0.0)`
// non è WGSL valido (serve `any(a != b)`).
//
// REGOLA: le struct qui sotto sono COPIA di `vendor/bevy_firefly/src/shaders/
// types.wgsl`. Se la shader vera cambia un campo, va cambiato anche qui,
// altrimenti il test fallisce con "no definition in scope" o "invalid field
// accessor" e ci ricorda di riallinearlo.

enable f16;

const PI2: f32 = 6.28318530717958647692528676655900577;
const PI: f32 = 3.14159265358979323846264338327950288;
const PIDIV2: f32 = 1.57079632679489661923132169163975144;

const N_BINS: u32 = 256;

struct PointLight {
    pos: vec2<f32>,
    intensity: f32,
    radius: f32,
    color: vec4<f32>,
    core_radius: f32,
    core_boost: f32,
    core_falloff: u32,
    core_falloff_intensity: f32,
    falloff: u32,
    falloff_intensity: f32,
    inner_angle: f32,
    outer_angle: f32,
    dir: vec2<f32>,
    z: f32,
    height: f32,
    shadow_sun: vec4<f32>,
}

struct PolyOccluder {
    start_vertex: u32,
    n_vertices: u32,
    z: f32,
    opacity: f32,
    color: vec4<f32>,
    z_sorting: u32,
}

struct OccluderPointer {
    index: u32,
    min_v: u32,
    split: u32,
    length: u32,
    distance: f32,
}

struct RoundOccluder {
    pos: vec2<f32>,
    rot: f32,
    half_width: f32,
    half_height: f32,
    radius: f32,
    z: f32,
    opacity: f32,
    color: vec4f,
    z_sorting: u32,
}

struct FireflyConfig {
    ambient_color: vec3<f32>,
    ambient_brightness: f32,
    light_bands: f32,
    soft_shadows: u32,
    shadow_algorithm: u32,
    penumbra_exp: f32,
    penumbra_flat: f32,
    penumbra_soft: f32,
    render_end: f32,
    z_sorting: u32,
    z_sorting_error_margin: f32,
    normal_mode: u32,
    normal_attenuation: f32,
    n_combined_lightmaps: u32,
    combination_mode: u32,
    texture_scale: vec2<f32>,
}

struct BinIndices {
    indices: array<u32, N_BINS + 1>,
}

struct View {
    clip_from_world: mat4x4<f32>,
    world_from_clip: mat4x4<f32>,
    viewport: vec4<f32>,
}

struct FullscreenVertexOutput {
    position: vec4<f32>,
    uv: vec2<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage> lights: array<PointLight>;
@group(0) @binding(2) var<storage> light_index: u32;
@group(0) @binding(3) var<storage> round_occluders: array<RoundOccluder>;
@group(1) @binding(4) var<storage> poly_occluders: array<PolyOccluder>;
@group(1) @binding(5) var<storage> vertices: array<vec2<f32>>;
@group(1) @binding(6) var<storage> occluders: array<OccluderPointer>;
@group(1) @binding(7) var<storage> bin_indices: BinIndices;
@group(1) @binding(8) var sprite_stencil: texture_2d<f32>;
@group(1) @binding(9) var normal_map: texture_2d<f32>;
@group(1) @binding(10) var<uniform> config: FireflyConfig;
@group(0) @binding(4) var texture_sampler: sampler;

fn ndc_to_world(p: vec2<f32>) -> vec2<f32> {
    return (view.world_from_clip * vec4<f32>(p, 0.0, 1.0)).xy;
}

fn frag_coord_to_ndc(p: vec2<f32>) -> vec2<f32> {
    return p * 0.001;
}

fn falloff(x: f32, mode: u32, intensity: f32) -> f32 {
    if mode == 0 {
        let x2 = x * x;
        return ((1.0 - x2) * (1.0 - x2) / (1.0 + intensity * x2));
    }
    if mode == 1 {
        return ((1.0 - x) / (1.0 + intensity * x));
    }
    return 1.0;
}

fn shadow_blend(bg: vec3<f32>, fg: vec3<f32>, opacity: f32) -> vec3<f32> {
    return bg * min(vec3<f32>(1.0), (vec3<f32>(2.0) - (vec3<f32>(1.0) - fg)) * (1.0 - opacity));
}

fn sgn(x: f32) -> f32 {
    return select(1.0, -1.0, x < 0.0);
}

fn rotate_90(p: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(-p.y, p.x);
}

fn orientation(a: vec2<f32>, b: vec2<f32>, p: vec2<f32>) -> f32 {
    return (b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y);
}

fn same_orientation(a: vec2<f32>, b: vec2<f32>, p: vec2<f32>, q: vec2<f32>) -> bool {
    let ori_p = orientation(a, b, p);
    let ori_q = orientation(a, b, q);
    return (ori_p == 0. && ori_q == 0.) || ori_p * ori_q > 0.;
}

fn ccw(a: vec2<f32>, b: vec2<f32>, c: vec2<f32>) -> bool {
    return (c.y - a.y) * (b.x - a.x) > (b.y - a.y) * (c.x - a.x);
}

fn intersect(a: vec2<f32>, b: vec2<f32>, c: vec2<f32>, d: vec2<f32>) -> bool {
    return ccw(a, c, d) != ccw(b, c, d) && ccw(a, b, c) != ccw(a, b, d);
}

fn rect_intersection(r1: vec4<f32>, r2: vec4<f32>) -> bool {
    return !(r2.x > r1.z || r2.z < r1.x || r2.w > r1.y || r2.y < r1.w);
}

fn rect_line_intersection(a: vec2<f32>, b: vec2<f32>, rect: vec4<f32>) -> bool {
    return true;
}

fn distance_point_to_line(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    return abs((b.y - a.y) * p.x - (b.x - a.x) * p.y + b.x * a.y - b.y * a.x) / distance(a, b);
}

fn between_arctan(x: f32, a: f32, b: f32) -> bool {
    return (x > a && x < b) || (x > PIDIV2 && b < -PIDIV2 && x > a && x - PI2 < b) || (x < -PIDIV2 && a > PIDIV2 && x + PI2 > a && x < b);
}

fn intersects_arc(a: vec2<f32>, b: vec2<f32>, c: vec2<f32>, r: f32, start_angle: f32, end_angle: f32) -> bool {
    return true;
}

struct IntersectsEdge {
    full_intersection: bool,
    half_intersection: bool,
}

fn intersects_axis_edge(p: vec2<f32>, l: vec2<f32>, coord: f32, min_v: f32, max_v: f32, is_vertical: bool) -> IntersectsEdge {
    var res: IntersectsEdge;
    return res;
}

fn intersects_corner_arc(p1: vec2<f32>, p2: vec2<f32>, c: vec2<f32>, r: f32, quadrant: vec2<f32>) -> IntersectsEdge {
    var res: IntersectsEdge;
    return res;
}

fn intersects_half(a: vec2<f32>, b: vec2<f32>, c: vec2<f32>, d: vec2<f32>) -> bool {
    return true;
}

// Le due funzioni di cono: il loro corpo è validato dal gate dedicato
// `shadow_v2_cone_is_valid_wgsl`, qui servono solo le firme.
fn desmos_round_check(pos: vec2<f32>, index: u32) -> f32 {
    return 0.0;
}

fn round_check(pos: vec2<f32>, index: u32) -> f32 {
    return 0.0;
}

fn poly_check(pos: vec2<f32>, index: u32, term: u32, rev: u32, min_v: u32, split: u32, length: u32) -> f32 {
    return 0.0;
}

fn bs_vertex_forward(angle: f32, start: u32, len: u32, term: u32, base: u32, n: u32) -> u32 {
    return 0u;
}

fn bs_vertex_reverse(angle: f32, start: u32, len: u32, term: u32, base: u32, n: u32) -> u32 {
    return 0u;
}
