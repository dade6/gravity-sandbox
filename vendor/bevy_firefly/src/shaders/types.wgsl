#define_import_path firefly::types


#import bevy_render::view::View

@group(0) @binding(0) var<uniform> view: View;

@group(0) @binding(1) var dt_lut_texture: texture_3d<f32>;

@group(0) @binding(2) var dt_lut_sampler: sampler;

struct PointLight {
    pos: vec2<f32>,
    intensity: f32,
    radius: f32,

    color: vec4<f32>, 

    core_radius: f32,
    core_boost: f32, 
    // 0 - inverse square, 1 - linear, 2 - none
    core_falloff: u32, 
    core_falloff_intensity: f32,


    // 0 - inverse square, 1 - linear, 2 - none
    falloff: u32,
    falloff_intensity: f32,

    inner_angle: f32,
    outer_angle: f32,

    dir: vec2<f32>, 

    z: f32,
    height: f32,
    // v0.14.195: raggio del disco solare usato SOLO dalle ombre, in `.x` di un
    // `vec4` (0 = usa core_radius). VEC4 per riempire esattamente i 16 byte:
    // un singolo `f32` lascerebbe 12 byte di padding implicito e `NoUninit`
    // rifiuta il tipo (la struct passerebbe da 80 a 84 byte).
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
    // v2 (Shadow System 3.0): 0 = cono v1 (firefly), 1 = cono analitico umbra+penombra
    shadow_algorithm: u32,
    // v0.14.176: esponente del profilo di penombra (1.0 = fisico lineare,
    // <1 più scura, >1 l'alone passa di più nella penombra)
    penumbra_exp: f32,
    // v0.14.178: se > 0 la penombra è PIATTA a questo valore (nessun
    // gradiente dentro la banda); 0 = usa il profilo (1-x)^penumbra_exp
    penumbra_flat: f32,
    // v0.14.179: ammorbidisce i bordi. Frazione dell'emilarghezza del cono
    // (w_cone) usata come rampa sui due bordi; 0 = bordi netti come prima
    penumbra_soft: f32,
    // v0.14.194: "coda" oltre la punta. Distanza (unità mondo, 0 = off) su cui
    // la penombra si dissolve DOPO t_close, così il cono continua a leggersi
    // invece di fermarsi di colpo sulla punta dell'umbra.
    penumbra_tail: f32,
    z_sorting: u32,
    z_sorting_error_margin: f32,
    // 0 - none, 1 - simple, 2 - topdown
    normal_mode: u32, 
    normal_attenuation: f32,
    n_combined_lightmaps: u32,

    // 0 - multiply, 1 - add, 2 - max, 3 - min, 4 - none
    combination_mode: u32,

    texture_scale: vec2<f32>
}

// Should correspond to the value in buffers.rs!
const N_BINS: u32 = 256;

struct BinIndices {
    indices: array<u32, N_BINS + 1>,
}