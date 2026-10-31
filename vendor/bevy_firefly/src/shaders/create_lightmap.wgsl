enable f16;

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import bevy_render::view::View

#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping
#endif

#import firefly::types::{
    view, PointLight, LightingData, PolyOccluder, RoundOccluder, OccluderPointer, 
    FireflyConfig, BinIndices, N_BINS,
}

#import firefly::utils::{
    ndc_to_world, frag_coord_to_ndc, orientation, same_orientation, intersect, blend, 
    shadow_blend, intersects_arc, rotate, rotate_arctan, between_arctan, distance_point_to_line,
    intersection_point, rect_intersection, rect_line_intersection, intersects_axis_edge, intersects_corner_arc,
    rotate_90, rotate_90_cc, intersects_half, falloff
}

@group(1) @binding(0)
var texture_sampler: sampler;

@group(1) @binding(1)
var<storage> lights: array<PointLight>;

@group(1) @binding(2)
var<storage> light_index: u32; 

@group(1) @binding(3)
var<storage> round_occluders: array<RoundOccluder>;

@group(1) @binding(4)
var<storage> poly_occluders: array<PolyOccluder>;

@group(1) @binding(5)
var<storage> vertices: array<vec2f>;

@group(1) @binding(6)
var<storage> occluders: array<OccluderPointer>;

@group(1) @binding(7)
var<storage> bin_indices: BinIndices;

@group(1) @binding(8)
var sprite_stencil: texture_2d<f32>;

@group(1) @binding(9)
var normal_map: texture_2d<f32>;

@group(1) @binding(10)
var<uniform> config: FireflyConfig;

const PI2: f32 = 6.28318530717958647692528676655900577;
const PI: f32 = 3.14159265358979323846264338327950288;
const PIDIV2: f32 = 1.57079632679489661923132169163975144; 

// ============================================================
// v2 "Shadow System 3.0" — cono analitico umbra + penombra
// ============================================================
// Riferimento: calcolatore Desmos "Shadow System 3.0" (pql2tb3dhv).
//
// Tre zone, come i poligoni del calcolatore:
//   cono (primary shadows):   w_c(t) = r_body + t·r_body/d
//   umbra (secondo poligono): w_u(t) = r_body - t·(r_sun - r_body)/d
//   penombra: la banda fra w_u e w_c, scurimento decrescente verso w_c.
// `t` = distanza lungo l'asse stella->corpo misurata dal CENTRO del corpo.
// Il bordo esterno è la SILHOUETTE del corpo vista dal centro del sole (un
// punto): il raggio solare entra SOLO nell'umbra. Se r_sun > r_body l'umbra
// converge a t = r_body·d/(r_sun - r_body), esattamente il `P_intersect` del
// calcolatore; se r_sun < r_body diverge e il cono resta pieno più a lungo.
// Mettere il disco solare anche sul bordo esterno (v0.14.167) allargava il
// cono con pendenza (r_sun + r)/d: con r_sun = 450 era ~2× troppo largo.
//
// I marker @@SHADOW_V2_BEGIN/END delimitano il blocco che il test
// `shadow_v2_cone_is_valid_wgsl` estrae e valida con naga in isolamento.
//
// @@SHADOW_V2_BEGIN
// Profilo della penombra: scurimento = (1 - x)^esponente, con x = 0 sul bordo
// dell'umbra e x = 1 sul bordo del cono. L'esponente arriva dall'uniform
// `config.penumbra_exp` (regolabile a caldo da Impostazioni → OMBRE):
//   1.0  = rampa lineare (fisica)
//   0.25 = penombra ~2× più scura della rampa lineare nella banda centrale
//   >1   = l'alone della stella passa di più: penombra più distinguibile
// Il valore arriva a 0 ESATTAMENTE sul bordo del cono: nessuno scalino. La
// v0.14.167 usava una soglia minima (0.3) e il velo finiva con un bordo netto
// dove il cono si interrompeva, ben visibile sui corpi che lo attraversavano.
const PENUMBRA_EXP: f32 = 0.25;

// Oltre il punto di incrocio dei bordi dell'umbra (`P_intersect`) il corpo non
// copre più l'intero disco solare: è l'antumbra. Il calcolatore di riferimento
// (Desmos "Shadow System 3.0") lì NON disegna niente di più scuro: il secondo
// poligono vale solo per `r_sun >= r_body` e si ferma proprio a P_intersect,
// mentre il cono primario continua sempre uguale fino al limite di render.
// Quindi qui NON c'è nessun fattore di copertura: il `peak = (r/dist :
// r_sun/dist)²` della v0.14.170 era arbitrario (non esiste nel riferimento) e
// spegneva i coni dei corpi piccoli: Rocky r=50 contro un disco solare di 450
// aveva la penombra quasi invisibile già a ~420 unità.
// La penombra dipende SOLO dalla distanza dall'asse: nessun valore arbitrario.

fn desmos_round_check(pos: vec2f, index: u32) -> f32 {
    let light = lights[light_index];
    let occ = round_occluders[index];
    let r_body = occ.radius;
    if r_body <= 0.0 {
        return 0.0;
    }
    let to_body = occ.pos - light.pos;
    let d = length(to_body);
    if d <= r_body {
        // La stella e' dentro (o sulla superficie del) corpo: nessun cono.
        return 0.0;
    }
    let axis = to_body / d;
    let perp = vec2f(-axis.y, axis.x);
    let q = pos - light.pos;
    let t = dot(q, axis) - d;    // distanza oltre il centro del corpo
    if t < 0.0 {
        return 0.0;              // davanti al corpo: illuminato
    }
    let b = abs(dot(q, perp));   // distanza dall'asse del cono
    // v0.14.195: raggio del disco solare usato dalle OMBRE. Se
    // `shadow_sun_radius > 0` è un valore dedicato (manopola "Shadow Sun"
    // del pannello Luce), altrimenti resta il core della luce come prima.
    // Serve perché `core_radius` è anche il raggio del NUCLEO LUMINOSO:
    // abbassarlo per allungare l'umbra cambierebbe l'aspetto della stella.
    let r_sun = select(
        max(light.core_radius, 0.0),
        max(light.shadow_sun.x, 0.0),
        light.shadow_sun.x > 0.0,
    );
    let w_umbra = max(r_body - t * (r_sun - r_body) / d, 0.0);
    // Bordo del cono = silhouette del corpo vista dal centro del sole.
    let w_cone = r_body + t * r_body / d;
    // v0.14.186: bordi morbidi calcolati PRIMA dei rami secchi, altrimenti
    // l'umbra uscirebbe con `return 1.0` e la rampa interna non avrebbe mai
    // effetto (bug v0.14.179: `soft` non cambiava NIENTE a schermo).
    //
    // Geometria della sfumatura, in unità mondo:
    //   soft_w = penumbra_soft * w_cone   -> semi-larghezza della rampa
    // Il profilo della penombra e' `p` (piatto) o `(1-x)^exp`, ma comunque
    // VIENE MOLTIPLICATO per due rampe che lo portano a 0 sul bordo esterno
    // e lo lasciano pieno nel tratto centrale:
    //   outer: 1 dentro, 0 fuori, rampa centrata su w_cone
    //   inner: 0 dentro l'umbra, 1 fuori, rampa centrata su w_umbra
    // Con soft = 0 le rampe degenerano in scalini (bordi netti, come prima).
    let soft_w = max(config.penumbra_soft, 0.0) * w_cone;
    let band = max(w_cone - w_umbra, 0.0001);
    let x = clamp((b - w_umbra) / band, 0.0, 1.0);
    let p = config.penumbra_flat;
    let flat_on = step(1e-5, p);
    let shaped = pow(1.0 - x, config.penumbra_exp);
    var v = select(shaped, clamp(p, 0.0, 1.0), flat_on > 0.5);

    // v0.14.195 — NERO PIENO FINO ALLA PUNTA, SFUMATURA SOLO DOPO.
    // Prima (0.192-0.194) il peso `p_umbra` faceva schiarire l'umbra GIÀ PRIMA
    // di `t_close`: a t=1000 il valore era 0.53 e a t=1050 0.47, quindi il
    // nero finiva molto prima della punta. Per questo il parametro "Coda cono"
    // sembrava inerte: agiva su un valore già chiaro.
    // Ora:
    //   - fino a `t_close` (la punta, dove le linee rosse si incontrano) il
    //     valore resta 1.0 = nero pieno;
    //   - oltre `t_close` la penombra si dissolve in `penumbra_tail` unità,
    //     così la punta non ha un bordo netto.
    // `t_close = r·d/(r_sun − r)` (infinito se il sole è più piccolo del corpo).
    let closed = r_sun > r_body;
    let t_close = select(1.0e9, r_body * d / max(r_sun - r_body, 0.0001), closed);
    let v_pen = select(shaped, clamp(p, 0.0, 1.0), flat_on > 0.5);
    // Umbria: nero pieno, senza alcun peso in t.
    let v_umbra = 1.0;

    if soft_w > 0.0001 {
        // v0.14.194 — CODA OLTRE LA PUNTA. Oltre `t_close` l'umbra è 0 per
        // geometria: senza coda la penombra resta costante a `flat` e il cono
        // sembra finire di colpo sulla punta (segnalato: "il cono è più corto").
        // Con `penumbra_tail > 0` il velo si dissolve su quella distanza.
        // Non cambia la fisica (t_close resta dove è): cambia solo per quanto
        // il cono continua a LEGGERSI dopo la punta.
        if config.penumbra_tail > 0.0001 && t > t_close {
            // q va da 0 (sulla punta) a 1 (fine coda)
            let q = clamp((t - t_close) / config.penumbra_tail, 0.0, 1.0);
            // v0.14.195: sulla punta il valore deve valere `v_pen` (penombra
            // piena, 0.45) e scendere a 0 a fine coda. Prima moltiplicava `v`
            // che era già chiaro, quindi non cambiava la lunghezza percepita.
            v = v_pen * (1.0 - smoothstep(0.0, 1.0, q));
        }
        // Bordo esterno: da `v` a 0 in `soft_w` unità, centrato su w_cone.
        // La rampa sta DENTRO il cono: inizia a `wipe` unità dal bordo e
        // finisce esattamente su w_cone (oltre, valore 0). Se la rampa
        // partisse FUORI dal cono (soft_w > wipe) allargherebbe il cono:
        // osservato a soft=1 il velo arrivava a b=250 con w_cone=240.
        // Il clamp finale a `band*0.5` impedisce che le due rampe si
        // sovrappongano oltre metà banda anche a soft=1.
        let wipe = min(soft_w, band * 0.5);
        let outer = 1.0 - smoothstep(w_cone - wipe, w_cone, b);
        v = v * outer;
        // Bordo interno: dall'umbra alla penombra in `wipe` unità.
        // Usa `v_umbra` (1.0 già pesato dalla distanza alla punta) e un
        // rientro laterale la cui larghezza NON si azzera dove serve: prima
        // era `min(wipe, w_umbra)`, che alla punta diventava 0 e lasciava il
        // bordo netto proprio lì.
        let w_umbra_eff = max(w_umbra, 0.0);
        let inner = smoothstep(w_umbra_eff, w_umbra_eff + max(wipe, 0.0001), b);
        v = v_umbra * (1.0 - inner) + v * inner;
    }

    // Umbria: la zona NERA. v0.14.192 — qui c'era `return 1.0` secco, e questo
    // ANNULLAVA tutta la sfumatura: lo screenshot a bordi attivi mostrava la
    // parte nera arrivare netta fino alla punta (croce rossa) mentre la
    // sfocatura si fermava prima, perché il peso `p_umbra` calcolato sopra
    // veniva buttato via da questo ramo.
    // Ora l'umbra usa `v_umbra` (1.0 pesato, che decade verso la penombra
    // quando ci si avvicina a `t_close`) e il rientro laterale `inner`.
    // Fuori dalla zona umbra resta il valore `v` della penombra.
    if w_umbra > 0.0 && b <= w_umbra {
        // v0.14.195: nero pieno fino alla punta, senza peso in `t`.
        return v_umbra;
    }
    if b >= w_cone {
        return 0.0;
    }
    return v;
}
// @@SHADOW_V2_END

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4f {
    // return vec4f(0.5);
    let light = lights[light_index];

    var res = vec4f(0);
    
    let pos = ndc_to_world(frag_coord_to_ndc(in.position.xy * config.texture_scale));
    let normal = textureLoad(normal_map, vec2<i32>(in.uv * vec2<f32>(textureDimensions(normal_map))), 0);
    let stencil = textureSample(sprite_stencil, texture_sampler, in.uv);

    let dist = distance(pos, light.pos);
    
    let a = pos - light.pos;
    let b = light.dir;
    let dot_a_b = clamp(dot(normalize(a), normalize(b)), -1.0, 1.0); 

    let angle = acos(dot_a_b);

    var light_color = light.color;
    // Patch v0.14.79 (Ticket 20): il canale ALPHA del colore della luce
    // trasporta `Halo Brightness` (StarGlow.brightness). I pixel SENZA
    // normal map (sfondo/starfield, glow semi-trasparenti) ricevono questo
    // valore invece di `intensity`, così l'alone sullo sfondo è controllato
    // da Halo Brightness e Planet Light tocca SOLO i pianeti.
    // Estratto PRIMA del tonemapping (che opererebbe anche sull'alpha).
    let halo = light.color.a;

#ifdef TONEMAP_IN_SHADER
    light_color = tonemapping::tone_mapping(light_color, view.color_grading);
#endif

    // light_color = pow(light_color, vec4<f32>(2.2));

    // TICKET 19 (vendored): la soglia storica era `dist < light.radius` →
    // spegnimento netto dei pianeti appena oltre il raggio (con Falloff::NONE
    // la luce è costante fino al bordo). Con Falloff::NONE il campo
    // falloff_intensity trasporta `fade_width` (vedi buffers.rs): l'area arriva
    // fino a radius + fade_width e il fade finale (sotto) sfuma a 0 in quella
    // banda. Per gli altri falloff falloff_intensity resta l'intensità reale
    // (fade non applicato, comportamento storico).
    if (dist < light.radius + light.falloff_intensity && angle <= light.outer_angle / 2.) {

        var angle_multi = 1.0; 

        if angle > light.inner_angle / 2. {
            // return vec4<f32>(1.0, 0.0, 0.0, 1.0);
            angle_multi = 1.0 - (angle - light.inner_angle / 2.) / (light.outer_angle / 2. - light.inner_angle / 2.);
        }

        var normal_multi = 1.0;
    
        if config.normal_mode != 0 && normal.a > 0 && normal.b != 0.1 {
            let normal_dir = mix(normalize(normal.xyz * 2f - 1f), vec3f(0f), config.normal_attenuation);

            if normal.b == 0.0 {
                normal_multi = 0.0;
            }
            else if normal.b == 0.1 {
                normal_multi = 1.0;
            }
            else if config.normal_mode == 1 {
                let light_dir = normalize(vec3f(light.pos.x - pos.x, light.pos.y - pos.y, light.z - stencil.g));
                normal_multi = max(0f, dot(normal_dir, light_dir));
            }
            else if config.normal_mode == 2 {
                // 2D PURO (scelta Davide v0.14.56; segno corretto v0.14.57):
                // direzione luce SOLO nel piano (dx, dy, 0). Il test .56 ha
                // mostrato che con (dx, light.y - pos.y, 0) la falce è
                // invertita verticalmente: la ny EFFETTIVA nel target normal
                // è ribaltata rispetto allo schermo, quindi la componente Y
                // va col segno stencil.r - light.pos.y (= pos.y - light.y),
                // come nella .55 che funzionava. Risultato: SMA metà disco
                // illuminata verso la stella, mai "spalle".
                let light_dir = normalize(vec3f(light.pos.x - pos.x, stencil.r - light.pos.y, 0.0));
                normal_multi = max(0f, dot(normal_dir, light_dir));
            }
            else if config.normal_mode == 3 {
                let light_dir = normalize(vec3f(light.pos.x - pos.x, light.height - stencil.b, light.z - stencil.g));
                normal_multi = max(0f, dot(normal_dir, light_dir));
            }
        }; 

        if normal.b == f32(f16(0.1)) {
            normal_multi = 1.0;
        }

        // Patch v0.14.165 (anello nero attorno alla stella, segnalato da Davide):
        // `surface` va calcolato PRIMA dei due rami, perché serve anche dentro
        // il core come PAVIMENTO. Il core usa `LightCore::falloff` di default
        // (InverseSquare) che vale 1 al centro e **0 esattamente a
        // core_radius**: la lightmap scendeva a zero sul bordo del core, e
        // siccome `apply_lightmap` MOLTIPLICA la scena per la lightmap, lo
        // sfondo lì veniva moltiplicato per ~0 → anello NERO e opaco tra il
        // disco della stella (fine del gradiente) e core_radius (1.5× raggio),
        // con salto netto al livello dell'alone subito fuori. Non era
        // controllabile dal pannello (core_radius_factor e il falloff del core
        // non sono esposti) e alzando Halo Brightness il salto peggiorava.
        // - Pixel CON normal map (pianeti, normal.a > 0) -> light.intensity
        // - Pixel SENZA normal map (alone, spazio vuoto) -> `halo`
        // - Sprite opachi SENZA normal map (normal dummy b == 0.1: nuclei
        //   delle stelle di sfondo, dischi) -> `halo` (emissivi).
        let surface = select(light.intensity, halo, normal.a == 0.0 || normal.b == f32(f16(0.1)));

        if dist <= light.core_radius {
            // Patch v0.14.77 (Ticket 20): disco stella separato da intensity.
            // Solo core_boost contribuisce al disco: intensity pilota SOLO
            // la luce sui pianeti (path else, * light.intensity).
            let core_term = light.core_boost
                * falloff(dist / light.core_radius, light.core_falloff, light.core_falloff_intensity);
            // PATCH v0.14.165: mai sotto `surface`. Il core resta più chiaro
            // al centro (boost) e si raccorda al livello dell'alone sul bordo,
            // invece di cadere a 0 e disegnare l'anello nero.
            res = vec4f(light_color.xyz, 0) * angle_multi * normal_multi * max(core_term, surface);
        }
        else {
            let x = (dist - light.core_radius) / (light.radius - light.core_radius);
            // Patch v0.14.79 (Ticket 20): SEPARAZIONE alone/luce pianeti.
            // Vedi sopra: `surface` sceglie intensity (pianeti) o halo (tutto
            // il resto). Così il disco di luce attorno alla stella risponde a
            // Halo Brightness e MAI a Planet Light.
            res = vec4f(light_color.xyz, 0) * surface * angle_multi * normal_multi * falloff(x, light.falloff, light.falloff_intensity);
        }

        // TEST 5 RIMOSSO (v0.14.50): shader pulita, il problema era
        // l'auto-ombra dei pianeti -> risolto con z_sorting + z=-y (vedi
        // firefly_bridge.rs).

        if dot(res, res) < 0.0001 {
            return res;
        }

        var round_index = 0u;
        var start_vertex = 0u;
        var sequence_index = 0u;

        var shadow = vec3f(1); 

        var bin = u32(floor(((atan2(pos.y - light.pos.y, pos.x - light.pos.x) + PI) / PI2) * f32(N_BINS)));
        bin = clamp(bin, 0, N_BINS-1);

        let left = bin_indices.indices[bin]; 
        let right = bin_indices.indices[bin + 1];

        // if left >= right {
            // return vec4f(1.0, 0.0, 0.0, 1.0);
        // }

        var prev_index = 0u; 
        var accumulated_occlusion = 0.0;

        // if left >= right {
        //     return vec4<f32>(1.0, 0.0, 0.0, 1.0);
        // }

        for (var pointer_index = left; pointer_index < right; pointer_index += 1) {
            let pointer = occluders[pointer_index];
            
            if pointer.distance > dist { break; }
            
            // return vec4<f32>(1.0, 0.0, 0.0, 1.0);
            let occluder_type = pointer.index & 2147483648u;
            let occluder_index = pointer.index & 2147483647u;

            // round occluder
            if occluder_type == 0 {
                if stencil.a > 0.1 {
                    // TORNO AL CHECK ORIGINALE DEL CRATE (>= - margin):
                    // con z = -y e margin default gli skip coprono
                    // l'auto-ombra (setup v0.14.57 funzionante). Le
                    // eclissi TRA pianeti sono gestite dal sistema CPU
                    // eclipse_occlusion (v0.14.63) perché il confronto
                    // esatto su stencil/occ non era affidabile nel render.
                    if config.z_sorting == 1 && round_occluders[occluder_index].z_sorting == 1 && stencil.g >= round_occluders[occluder_index].z - config.z_sorting_error_margin {
                        continue;
                    }
                }

                var result: f32;
                if config.shadow_algorithm == 1
                    && round_occluders[occluder_index].radius > 0.0
                    && round_occluders[occluder_index].half_width <= 0.0
                    && round_occluders[occluder_index].half_height <= 0.0 {
                    // v2: cono analitico dal disco solare (solo cerchi; i
                    // rettangoli arrotondati restano sulla v1).
                    result = desmos_round_check(pos, occluder_index);
                } else {
                    result = round_check(pos, occluder_index); 
                }


                if result > 0.0 {
                    shadow = shadow_blend(shadow, round_occluders[occluder_index].color.rgb, round_occluders[occluder_index].opacity * result);
                }            
            }
            // poly occluder
            else {
                if stencil.a > 0.1 {
                    if config.z_sorting == 1 && poly_occluders[occluder_index].z_sorting == 1 && stencil.g >= poly_occluders[occluder_index].z - config.z_sorting_error_margin {
                        continue;
                    }
                }

                if prev_index != occluder_index {
                    if prev_index != 0u && accumulated_occlusion > 0.0 {
                        shadow = shadow_blend(shadow, poly_occluders[prev_index].color.rgb, poly_occluders[prev_index].opacity * accumulated_occlusion);
                    }
                    accumulated_occlusion = 0.0;
                    prev_index = occluder_index;
                }

                let term = (pointer.min_v & 3221225472u) >> 30u;

                let rev = (pointer.min_v & 536870912u) >> 29u;

                let min_v = pointer.min_v & 536870911u;
                let split = pointer.split;
                let length = pointer.length & 1073741823u;

                let result = poly_check(pos, occluder_index, term, rev, min_v, split, length); 
                accumulated_occlusion = max(accumulated_occlusion, result);
            }

            if dot(shadow, shadow) < 0.001 {
                break;
            }
        }
            
        if prev_index != 0u && accumulated_occlusion > 0.0 {
            shadow = shadow_blend(shadow, poly_occluders[prev_index].color.rgb, poly_occluders[prev_index].opacity * accumulated_occlusion);
        }

        // v0.14.134 (supersede v0.14.132): le ombre dei pianeti oscurano
        // l'alone e lo spazio vuoto — il cono RESTA visibile — ma MAI le
        // stelle di sfondo (nucleo + alone). Lo stencil pass di firefly
        // (`sprite.wgsl`) campiona la texture GREZZA, ignorando la tinta:
        // solo il nucleo saturo scrive stencil.a = 1, l'alone esterno
        // resta stencil 0 come lo spazio vuoto — per questo il gate
        // v0.14.132 (solo nuclei) scuriva comunque un po' le stelle.
        // Discriminante robusto: la z dello sprite (stencil.g). Lo
        // starfield vive a z ≈ -500k (parallax.rs), i pianeti a
        // z = -distanza e la stella a z = 0. Quindi:
        // - stencil.g < -100000 = stella di sfondo (tutto il quad) ->
        //   salta l'ombra del tutto;
        // - sprite opaco + normal dummy = disco stella emissivo ->
        //   salta l'ombra, resta luminoso;
        // - tutto il resto (pianeti, alone stella, spazio vuoto) ->
        //   ombra applicata, cono visibile.
        var is_background_star = stencil.g < -100000.0;
        var is_emissive_core = stencil.a > 0.1 && normal.b == f32(f16(0.1));
        if (!is_background_star && !is_emissive_core) {
            res *= vec4f(shadow, 1);
        }
    }

    // return pow(res, vec4<f32>(1.0/2.2));
    // TICKET 19 (vendored): soft edge finale. Con Falloff::NONE il campo
    // falloff_intensity trasporta fade_width (vedi buffers.rs): la luce sfuma
    // in modo continuo da piena (dist <= radius) a 0 (dist = radius + fade),
    // eliminando il taglio secco. Quando vale 0 (o falloff non-None) il
    // fattore è 1 ovunque → comportamento storico identico.
    if (light.falloff_intensity > 0.0) {
        let fade_t = clamp((dist - light.radius) / light.falloff_intensity, 0.0, 1.0);
        let fade = 1.0 - fade_t * fade_t * (3.0 - 2.0 * fade_t); // smoothstep 1→0
        res *= fade;
    }
    return res;
}

fn poly_check(pos: vec2f, index: u32, term: u32, rev: u32, min_v: u32, split: u32, length: u32) -> f32 {
    let light = lights[light_index];
    let occluder = poly_occluders[index];

    let angle = atan2(pos.y - light.pos.y, pos.x - light.pos.x);

    var maybe_prev = 0; 

    var start = min_v; 
    var len = length; 

    if rev == 0 {

        if term == 1 {
            len = split + 1;
        }
        else if term == 2 {
            start = min_v + split - 1;
            len = length - split + 1;
        }

        maybe_prev = bs_vertex_forward(angle, start, len, term, occluder.start_vertex, occluder.n_vertices);
    }
    else {
        if term == 1 {
            len = split + 1;
        }
        else if term == 2 {
            start = min_v - split + 1;
            len = length - split + 1;
        }

        maybe_prev = bs_vertex_reverse(angle, start, len, term, occluder.start_vertex, occluder.n_vertices);
    }

    var is_occluded = false;

    let out_of_bounds = maybe_prev < 0 || maybe_prev + 1 >= i32(len);

    if !out_of_bounds {
        if rev == 0 {
            let v1 = vertices[start + u32(maybe_prev) - select(0, occluder.n_vertices, start + u32(maybe_prev) >= occluder.start_vertex + occluder.n_vertices)];
            let v2 = vertices[start + u32(maybe_prev) + 1 - select(0, occluder.n_vertices, start + u32(maybe_prev) + 1 >= occluder.start_vertex + occluder.n_vertices)];

            is_occluded = !same_orientation(v1, v2, pos, light.pos);
        }
        else {
            let v1 = vertices[i32(start) - maybe_prev + select(0, i32(occluder.n_vertices), i32(start) - maybe_prev < i32(occluder.start_vertex))];
            let v2 = vertices[i32(start) - maybe_prev - 1 + select(0, i32(occluder.n_vertices), i32(start) - maybe_prev - 1 < i32(occluder.start_vertex))];

            is_occluded = !same_orientation(v1, v2, pos, light.pos);
        }
    }

    if config.soft_shadows > 0 && light.core_radius > 0.0 && out_of_bounds {
        if rev == 0 {
            let loops = min_v + length - 1 >= occluder.start_vertex + occluder.n_vertices;
            let last = min_v + length - 1 - select(0, occluder.n_vertices, loops);
    
            return get_softness_multi(light.core_radius, light.pos, pos, vertices[min_v], vertices[last]);
        }
        else {
            let loops = i32(min_v) - i32(length) + 1 < i32(occluder.start_vertex);
            let last = u32(i32(min_v) - i32(length) + 1 + select(0, i32(occluder.n_vertices), loops));
            
            return get_softness_multi(light.core_radius, light.pos, pos, vertices[min_v], vertices[last]);
        }
    }

    if is_occluded {
        return 1.0;
    }

    return 0.0;
}

fn get_softness_multi(light_range: f32, light_pos: vec2<f32>, pos: vec2<f32>, extreme_left: vec2<f32>, extreme_right: vec2<f32>) -> f32 {
    // if distance(pos, extreme_right) < 30.0 {
    //     return 1.0;
    // }

    // let left_range = light_range;
    
    let left_range = min(light_range, distance(extreme_left, light_pos)); 
 
    var left_t1 = light_pos + rotate_90(normalize(extreme_left - light_pos)) * left_range;
    var left_t2 = light_pos;

    // let right_range = light_range;
    let right_range = min(light_range, distance(extreme_right, light_pos));

    var right_t1 = light_pos;
    var right_t2 = light_pos + rotate_90_cc(normalize(extreme_right - light_pos)) * right_range;

    let above_left = orientation(left_t1, extreme_left, pos) < -0.99;
    let under_left = orientation(left_t2, extreme_left, pos) > 0.01;

    let inside_left = !above_left && !under_left;

    let under_right = orientation(right_t2, extreme_right, pos) > 0.01;
    let above_right = orientation(right_t1, extreme_right, pos) < -0.99;

    let inside_right = !above_right && !under_right;    

    var left = 0.0;
    var right = 0.0;

    if inside_left {
        let left2 = normalize(extreme_left - left_t2);
        var d1 = dot(normalize(pos - extreme_left), left2);
        var d2 = dot(normalize(extreme_left - left_t1), left2);

        d1 = clamp(d1, -1.0, 1.0);
        d2 = clamp(d2, -1.0, 1.0);

        left = 1.0 - acos(d1) / acos(d2);
    }
    
    if inside_right {
        let right1 = normalize(extreme_right - right_t1);
        var d1 = dot(normalize(pos - extreme_right), right1);
        var d2 = dot(normalize(extreme_right - right_t2), right1);

        d1 = clamp(d1, -1.0, 1.0);
        d2 = clamp(d2, -1.0, 1.0);

        right = 1.0 - acos(d1) / acos(d2);
    }

    return max(left, right);
}

fn angle_term(p: vec2f, i: u32, length: u32, term: u32) -> f32 {
    let light = lights[light_index];
    var angle = atan2(p.y - light.pos.y, p.x - light.pos.x);
    
    if i == length - 1 && term == 1 {
        angle += PI2;
    }
    else if i == 0 && term == 2 {
        angle -= PI2; 
    }

    return angle;
}

fn vertex_forward(start: u32, index: u32, start_vertex: u32, n_vertices: u32) -> vec2<f32> {
    if start + index >= start_vertex + n_vertices {
        return vertices[start + index - n_vertices];
    }
    return vertices[start + index];
}

fn vertex_reverse(start: u32, index: i32, start_vertex: u32, n_vertices: u32) -> vec2<f32> {
    if i32(start) - i32(index) < i32(start_vertex) {
        return vertices[u32(i32(start) - i32(index) + i32(n_vertices))];
    }
    return vertices[u32(i32(start) - i32(index))];
} 

fn bs_vertex_forward(angle: f32, start: u32, length: u32, term: u32, start_vertex: u32, n_vertices: u32) -> i32 {
    let light = lights[light_index];

    var ans = -1;
    
    var low = 0i; 
    var high = i32(length) - 1; 

    if angle < angle_term(vertex_forward(start, u32(low), start_vertex, n_vertices), u32(low), length, term) {
        return -1;
    }

    if angle > angle_term(vertex_forward(start, u32(high), start_vertex, n_vertices), u32(high), length, term) {
        return high + 1;
    }

    while (low <= high) {
        let mid = low + (high - low + 1) / 2;
        let val = angle_term(vertex_forward(start, u32(mid), start_vertex, n_vertices), u32(mid), length, term);

        if (val < angle) {
            ans = i32(mid);
            low = mid + 1;
        }
        else {
            high = mid - 1;
        }
    }

    return ans;
}

fn bs_vertex_reverse(angle: f32, start: u32, length: u32, term: u32, start_vertex: u32, n_vertices: u32) -> i32 {
    let light = lights[light_index];

    var ans = -1;
    
    var low = 0i; 
    var high = i32(length) - 1;

    if angle <= angle_term(vertex_reverse(start, low, start_vertex, n_vertices), u32(low), length, term) {
        return -1;
    }

    if angle >= angle_term(vertex_reverse(start, high, start_vertex, n_vertices), u32(high), length, term) {
        return high + 1;
    }

    while (low <= high) {
        let mid = low + (high - low + 1) / 2;
        let val = angle_term(vertex_reverse(start, mid, start_vertex, n_vertices), u32(mid), length, term);

        if (val < angle) {
            ans = i32(mid);
            low = mid + 1;
        }
        else {
            high = mid - 1;
        }
    }

    return ans;
}

// checks if pixel is blocked by round occluder
fn round_check(pos: vec2f, occluder: u32) -> f32 {
    let light = lights[light_index];

    let occ = round_occluders[occluder];
    let half_w = occ.half_width;
    let half_h = occ.half_height;
    let radius = occ.radius;

    let relative_pos = pos - occ.pos; 
    let relative_light = light.pos - occ.pos; 

    let c = cos(occ.rot);
    let s = sin(occ.rot);

    // let c = 1.0; 
    // let s = 0.0;

    let p_local = vec2f(relative_pos.x * c + relative_pos.y * s, -relative_pos.x * s + relative_pos.y * c);
    let l_local = vec2f(relative_light.x * c + relative_light.y * s, -relative_light.x * s + relative_light.y * c);
    
    var half_intersection = false; 
    
    let rect = vec4f(-(half_w + radius), -(half_h + radius), half_w + radius, half_h + radius);

    if !rect_line_intersection(p_local, l_local, rect) {

        if config.soft_shadows > 0 && light.core_radius > 0.0 {
            return get_round_extreme_angle(half_w, half_h, p_local, l_local, light.core_radius, radius);
        }

        return 0.0;
    }

    if (half_w > 0) {
        let top_edge = intersects_axis_edge(p_local, l_local, half_h + radius, -half_w, half_w, false);

        if top_edge.full_intersection {
            return 1.0;
        }
        
        half_intersection |= top_edge.half_intersection;

        let bottom_edge = intersects_axis_edge(p_local, l_local, -(half_h + radius), -half_w, half_w, false);

        if bottom_edge.full_intersection {
            return 1.0;
        }

        half_intersection |= bottom_edge.half_intersection;
    }

    if (half_h > 0) {
        let right_edge = intersects_axis_edge(p_local, l_local, half_w + radius, -half_h, half_h, true);

        if right_edge.full_intersection {
            return 1.0;
        }

        half_intersection |= right_edge.half_intersection;

        let left_edge = intersects_axis_edge(p_local, l_local, -(half_w + radius), -half_h, half_h, true);

        if left_edge.full_intersection {
            return 1.0;
        }

        half_intersection |= left_edge.half_intersection;
    }

    if (radius > 0) {
        let arc1 = intersects_corner_arc(p_local, l_local, vec2f(half_w, half_h), radius, vec2f(1,1)); 
        if arc1.full_intersection { 
            return 1.0;
        }
        half_intersection |= arc1.half_intersection;

        let arc2 = intersects_corner_arc(p_local, l_local, vec2f(-half_w, half_h), radius, vec2f(-1,1)); 
        if arc2.full_intersection { 
            return 1.0;
        }
        half_intersection |= arc2.half_intersection;

        let arc3 = intersects_corner_arc(p_local, l_local, vec2f(half_w, -half_h), radius, vec2f(1,-1)); 
        if arc3.full_intersection { 
            return 1.0;
        }
        half_intersection |= arc3.half_intersection;

        let arc4 = intersects_corner_arc(p_local, l_local, vec2f(-half_w, -half_h), radius, vec2f(-1,-1)); 
        if arc4.full_intersection { 
            return 1.0;
        }
        half_intersection |= arc4.half_intersection;
    }

    if config.soft_shadows > 0 && light.core_radius > 0.0 && !half_intersection {
        return get_round_extreme_angle(half_w, half_h, p_local, l_local, light.core_radius, radius);
    }

    return 0.0;
}

fn get_round_extreme_angle(half_w: f32, half_h: f32, p_local: vec2f, l_local: vec2f, light_radius: f32, radius: f32) -> f32 {
    var left_right = vec4<f32>(half_w + radius, half_h, half_w + radius, half_h);

    if radius == 0.0 {
        if half_h > 0 {
            left_right = update_left_right(l_local, left_right, vec2f(half_w + radius, -half_h));
            left_right = update_left_right(l_local, left_right, vec2f(-(half_w + radius), half_h));
            left_right = update_left_right(l_local, left_right, vec2f(-(half_w + radius), -half_h));
        }

        if half_w > 0 {
            left_right = update_left_right(l_local, left_right, vec2f(half_w, half_h + radius));
            left_right = update_left_right(l_local, left_right, vec2f(-half_w, half_h + radius));
            left_right = update_left_right(l_local, left_right, vec2f(half_w, -(half_h + radius)));
            left_right = update_left_right(l_local, left_right, vec2f(-half_w, -(half_h + radius)));
        }
    }
    else {
        let arc_tangents1 = get_arc_extremes(l_local, vec2f(half_w, half_h), radius, 0.0, PIDIV2);
        let arc_tangents2 = get_arc_extremes(l_local, vec2f(-half_w, half_h), radius, PIDIV2, PI);
        let arc_tangents3 = get_arc_extremes(l_local, vec2f(half_w, -half_h), radius, -PIDIV2, 0.0);
        let arc_tangents4 = get_arc_extremes(l_local, vec2f(-half_w, -half_h), radius, -PI, -PIDIV2);

        if arc_tangents1.is_a {
            left_right = update_left_right(l_local, left_right, arc_tangents1.a);
        }
        if arc_tangents1.is_b {
            left_right = update_left_right(l_local, left_right, arc_tangents1.b);
        }

        if arc_tangents2.is_a {
            left_right = update_left_right(l_local, left_right, arc_tangents2.a);
        }
        if arc_tangents2.is_b {
            left_right = update_left_right(l_local, left_right, arc_tangents2.b);
        }

        if arc_tangents3.is_a {
            left_right = update_left_right(l_local, left_right, arc_tangents3.a);
        }
        if arc_tangents3.is_b {
            left_right = update_left_right(l_local, left_right, arc_tangents3.b);
        }

        if arc_tangents4.is_a {
            left_right = update_left_right(l_local, left_right, arc_tangents4.a);
        }
        if arc_tangents4.is_b {
            left_right = update_left_right(l_local, left_right, arc_tangents4.b);
        }
    }

    return get_softness_multi(light_radius, l_local, p_local, left_right.xy, left_right.zw);
}

fn update_left_right(light_pos: vec2<f32>, left_right: vec4<f32>, p: vec2<f32>) -> vec4<f32> {
    var res = left_right;
    if orientation(light_pos, left_right.xy, p) < 0 {
        res.x = p.x;
        res.y = p.y;
    }

    if orientation(light_pos, left_right.zw, p) > 0 {
        res.z = p.x; 
        res.w = p.y; 
    }

    return res; 
}

fn get_arc_extremes(l_local: vec2f, c: vec2f, r: f32, start_angle: f32, end_angle: f32) -> ArcTangents {
    var res: ArcTangents;

    let diff = l_local - c;
    let dist_sq = dot(diff, diff);
    
    // Pixel is inside the corner radius
    // if (dist_sq <= r * r) { return 10.0; } 

    let dist = sqrt(dist_sq);
    let th = acos(r / dist);
    let d = atan2(diff.y, diff.x);
    
    let d1 = d + th;
    let d2 = d - th;

    // Tangent points on the circle
    let t1 = vec2f(c.x + r * cos(d1), c.y + r * sin(d1));
    let t2 = vec2f(c.x + r * cos(d2), c.y + r * sin(d2));

    // Angles of tangent points relative to center 'c'
    let a1 = atan2(t1.y - c.y, t1.x - c.x);
    let a2 = atan2(t2.y - c.y, t2.x - c.x);

    if (between_arctan(a1, start_angle, end_angle)) {
        res.is_a = true; 
        res.a = t1; 
    }

    if (between_arctan(a2, start_angle, end_angle)) {
        res.is_b = true; 
        res.b = t2;
    }

    return res;
}

struct ArcTangents {
    is_a: bool, 
    a: vec2<f32>, 
    is_b: bool,
    b: vec2<f32>
}
