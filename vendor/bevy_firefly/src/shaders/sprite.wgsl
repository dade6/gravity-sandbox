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
    // v0.14.207 — IL BORDO DEL PIANETA (strada C): la NORMAL MAP deve coprire
    // gli stessi texel dello stencil, non solo quelli pieni.
    //
    // Storia: dal .190 lo stencil dei corpi si scrive già con soglia 0.004,
    // quindi il bordo antialiasato ENTRA nello stencil e riceve `res *= shadow`.
    // Ma la normal map restava a `color.a >= 1.0`: sui texel di bordo
    // (`color.a` = copertura, 0.04..0.43) `res.normal` era `vec4(0)`, cioè
    // `normal.a == 0` e `normal.b == 0`.
    //
    // Nella lightmap quel pixel prende il ramo `surface = halo` (pixel SENZA
    // superficie) e `normal_multi = 0`: la lightmap vale ~0 invece di
    // `intensity`. Con `Planet Light = 0` il valore giusto è **0**, ma il pixel
    // composito conserva una parte di sprite NON moltiplicata, perché l'alpha
    // del bordo è parziale: il colore della texture (a spicchi!) resta visibile
    // come un filo luminoso che segue gli spicchi. Misurato: bordo 144 tu 19
    // di fondo, con il cono nero a 1 alle spalle.
    //
    // Dato decisivo su device: con Halo = 10 e Planet Light = 0 il bordo è
    // ancora colorato come gli spicchi, quindi non è la stella (la stella è
    // spenta su quel percorso) ed è la texture che torna visibile dove la
    // normal map manca.
    //
    // Fix: sugli stessi texel che scrivono lo stencil si scrive la normal map.
    // Per i CORPI (`height == 0`) la soglia diventa `stencil_min_alpha` (0.004)
    // invece di 1.0. Per tutto il resto (stella, aloni, starfield) resta
    // esattamente `color.a >= 1.0`, cioè il comportamento originale del crate:
    // il percorso critico degli altri sprite NON cambia.
    //
    // ⚠️ Il ramo `normal_dummy` resta invariato: la stella e gli sprite senza
    // normal map continuano a scrivere il dummy (b == 0.1), che la shader
    // tratta come "superficie piena senza rilievo".
    let normal_min_alpha = select(1.0, stencil_min_alpha, is_body);
    if color.a >= normal_min_alpha {
        if normal_dummy == 1 {
            res.normal = vec4<f32>(0, 0, f32(f16(0.1)), 1.0);
        }
        else {
            // v0.14.209 — LA COPERTURA VIAGGIA NEL CANALE ALPHA DELLA NORMAL MAP.
            //
            // Ora che il bordo ha una normal map (v0.208), la lightmap gli
            // applica l'ombra come a una superficie piena: su un pixel quasi
            // trasparente (copertura 0.04..0.43) il risultato è che si vede la
            // LIGHTMAP al posto del pianeta, cioè un ANELLO NERO di ~9 px fuori
            // dalla silhouette (misurato: lum 3-12 tu 46 di fondo).
            //
            // La shader delle ombre ha bisogno di sapere QUANTO quel texel
            // copre il pixel, per riportare l'ombra verso il fondo in
            // proporzione: è l'antialiasing corretto. Il canale non usato della
            // normal map (`a`) è libero — la shader legge solo `.rgb` per il
            // rilievo e `.b` per la sentinella — e c'è spazio.
            //
            // `a` = copertura del disco in quel texel. Per la stella e per gli
            // sprite senza normal map resta 1.0 (comportamento invariato).
            // `a` = copertura del disco per i CORPI. Per ogni altro sprite con
            // normal map resta 1.0: l'alpha di un alone è la trasparenza del
            // glow, non la copertura di una silhouette, e usarla come copertura
            // smorzerebbe la luce dell'alone.
            res.normal = vec4<f32>(normal.rgb, select(1.0, color.a, is_body));
        }
    }
    else {
        res.normal = vec4<f32>(0.0);
    }

    return res; 
}
