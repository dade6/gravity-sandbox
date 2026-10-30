enable f16;

#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping
#endif

#import bevy_render::{
    maths::affine3_to_square,
    view::View,
}

#import firefly::types::SpriteId

#import bevy_sprite::sprite_view_bindings::view

struct VertexInput {
    @builtin(vertex_index) index: u32,
    // NOTE: Instance-rate vertex buffer members prefixed with i_
    // NOTE: i_model_transpose_colN are the 3 columns of a 3x4 matrix that is the transpose of the
    // affine 4x3 model matrix.
    @location(0) i_model_transpose_col0: vec4<f32>,
    @location(1) i_model_transpose_col1: vec4<f32>,
    @location(2) i_model_transpose_col2: vec4<f32>,
    @location(3) i_uv_offset_scale: vec4<f32>,
    @location(4) z: f32,
    @location(5) height: f32,
    @location(6) y: f32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) z: f32,
    @location(2) height: f32,
    @location(3) y: f32,
};

@vertex
fn vertex(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    let vertex_position = vec3<f32>(
        f32(in.index & 0x1u),
        f32((in.index & 0x2u) >> 1u),
        0.0
    );

    out.clip_position = view.clip_from_world * affine3_to_square(mat3x4<f32>(
        in.i_model_transpose_col0,
        in.i_model_transpose_col1,
        in.i_model_transpose_col2,
    )) * vec4<f32>(vertex_position, 1.0);
    out.uv = vec2<f32>(vertex_position.xy) * in.i_uv_offset_scale.zw + in.i_uv_offset_scale.xy;
    out.z = in.z;
    out.height = in.height;
    out.y = in.y;

    return out;
}

@group(1) @binding(0) var sprite_texture: texture_2d<f32>;
@group(1) @binding(1) var normal_texture: texture_2d<f32>;
@group(1) @binding(2) var sprite_sampler: sampler;
@group(1) @binding(3) var<uniform> normal_dummy: u32;

struct FragmentOutput {
    @location(0) stencil: vec4<f32>, 
    @location(1) normal: vec4<f32>,
}

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    var res: FragmentOutput;

    var color = textureSample(sprite_texture, sprite_sampler, in.uv);
    var normal = textureSample(normal_texture, sprite_sampler, in.uv);
    
    // v0.14.137/139: gli sprite dello starfield (SpriteHeight = sentinella
    // NEGATIVA, vedi STARFIELD_HEIGHT in parallax.rs) scrivono lo stencil su
    // TUTTO il proprio quad, anche dove la texture e' semitrasparente: la
    // shader delle ombre riconosce lo starfield dalla sua z (stencil.g ~=
    // -500k) e ne esenta i pixel, e senza stencil l'alone esterno
    // (alpha < 1) restava "spazio vuoto" -> il cono d'ombra lo spegneva.
    // Gli sprite normali (pianeti, glow della stella) mantengono la soglia
    // storica alpha >= 1.0.
    // NB: `select(f, t, cond)` e' WGSL standard — un if-EXPRESSION non
    // esiste in WGSL e non va dato per scontato che naga lo accetti.
    // Questa condizione tocca SOLO lo stencil (le ombre): mai la normal map.
    // v0.14.190 — IL BORDINO CHIARO SUL BORDO DEL PIANETA.
    // La texture del disco ha alpha di copertura antialiasato sull'ultimo
    // texel (`generate_disc_texture`), quindi sul bordo del pianeta `color.a`
    // è fra 0 e 1. Con la soglia storica `alpha >= 1.0` quei texel NON
    // scrivevano stencil: la shader delle ombre li vedeva come "spazio vuoto"
    // e non li scuriva, lasciando una linguetta chiara lungo la circonferenza
    // (segnalata su device).
    // La sentinella `height` distingue già i casi: starfield -999, CORPI 0,
    // stella 1000. Per i corpi (height == 0) basta un alpha minimo, così il
    // bordo antialiasato entra nello stencil e riceve l'ombra.
    // NOTA: questa condizione tocca SOLO lo stencil, mai il ramo della normal
    // map (che resta `color.a >= 1.0`, vedi pitfall 0y).
    let is_starfield = in.height < 0.0;
    let is_body = in.height == 0.0;
    let stencil_min_alpha = select(select(1.0, 0.004, is_starfield), 0.004, is_body);
    let writes_stencil = color.a >= stencil_min_alpha;

    if writes_stencil {
        // stencil come da crate (alpha = 1.0: presenza sprite)
        res.stencil = vec4<f32>(in.y, in.z, in.height, 1.0);
    }
    else {
        res.stencil = vec4<f32>(0, 0, 0, 0);
    }

    // v0.14.139: la scrittura della NORMAL MAP resta IDENTICA allo shader
    // originale del crate (`color.a >= 1.0`), senza nessun riferimento allo
    // starfield. La normal map e' cio' che da' la porzione illuminata ai
    // pianeti (terminatore giorno/notte): legarla a una condizione nuova
    // significa che se quella condizione si comporta male NESSUNA sprite la
    // scrive -> i pianeti perdono il terminatore e ricevono Halo Brightness
    // al posto di Planet Light (bug v0.14.138: "non si vede piu' la porzione
    // illuminata dei pianeti"). Il percorso critico per i pianeti NON deve
    // dipendere da nulla di nostro.
    if color.a >= 1.0 {
        if normal_dummy == 1 {
            res.normal = vec4<f32>(0, 0, f32(f16(0.1)), 1.0);
        }
        else {
            res.normal = normal;
        }
    }
    else {
        res.normal = vec4<f32>(0.0);
    }

    return res; 
}
