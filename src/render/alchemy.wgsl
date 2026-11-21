// Alquimia: feedback de la imagen anterior (zoom, rotación, remolino,
// ondulación, caleidoscopio, blur) más la escena del frame actual.

struct FullscreenOut {
    @builtin(position) clip_position: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vs_fullscreen(@builtin(vertex_index) index: u32) -> FullscreenOut {
    var positions = array<vec2f, 3>(
        vec2f(-1.0, -1.0),
        vec2f(3.0, -1.0),
        vec2f(-1.0, 3.0),
    );
    let p = positions[index];
    var out: FullscreenOut;
    out.clip_position = vec4f(p, 0.0, 1.0);
    out.uv = vec2f((p.x + 1.0) * 0.5, 1.0 - (p.y + 1.0) * 0.5);
    return out;
}

// Debe coincidir campo a campo con `FeedbackParams` en alchemy.rs (128 B).
struct FeedbackParams {
    resolution: vec2f,
    time: f32,
    zoom: f32,
    rotation: f32,
    swirl: f32,
    warp_amp: f32,
    warp_freq: f32,
    kaleido: f32,
    blur: f32,
    fade: f32,
    hue_shift: f32,
    inject: f32,
    radial_blur: f32,
    smear: vec2f,
    // Color de fondo hacia el que decae la estela (0 = negro).
    base: vec3f,
    // 1 = la escena puede borrar: píxeles con color negativo limpian la
    // estela (la franja de Windmill).
    erase: f32,
    // Contracción por eje (0 = sin efecto): 0.93 en x junta la imagen hacia
    // el centro en horizontal.
    stretch: vec2f,
    // 1 = modo paleta: la intensidad vive en el alfa y el color sale de
    // `base` → `mid` → `high`, como en las visualizaciones de WMP.
    palette: f32,
    _pad0: f32,
    mid: vec3f,
    _pad1: f32,
    high: vec3f,
    _pad2: f32,
}

@group(0) @binding(0) var prev_tex: texture_2d<f32>;
@group(0) @binding(1) var scene_tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var<uniform> fb: FeedbackParams;

fn rot2(p: vec2f, a: f32) -> vec2f {
    let c = cos(a);
    let s = sin(a);
    return vec2f(c * p.x - s * p.y, s * p.x + c * p.y);
}

// Rotación de tono alrededor del eje gris (1,1,1) (fórmula de Rodrigues).
fn hue_rotate(c: vec3f, a: f32) -> vec3f {
    let k = vec3f(0.57735027);
    let ca = cos(a);
    return c * ca + cross(k, c) * sin(a) + k * dot(k, c) * (1.0 - ca);
}

@fragment
fn fs_feedback(input: FullscreenOut) -> @location(0) vec4f {
    let aspect = fb.resolution.x / max(fb.resolution.y, 1.0);
    var p = (input.uv - 0.5) * vec2f(aspect, 1.0);

    // Caleidoscopio: pliega el ángulo en segmentos espejados.
    if (fb.kaleido > 0.5) {
        let seg = 6.2831853 / fb.kaleido;
        let r = length(p);
        let a = abs(fract(atan2(p.y, p.x) / seg + 0.5) - 0.5) * seg;
        p = vec2f(cos(a), sin(a)) * r;
    }

    // Rotación + remolino (más giro lejos del centro), zoom y ondulación.
    p = rot2(p, fb.rotation + fb.swirl * length(p));
    p = p / max(fb.zoom, 0.01);
    let st = select(vec2f(1.0), fb.stretch, fb.stretch > vec2f(0.0));
    p = p / st;
    if (fb.palette > 0.5) {
        // Modo paleta (nubes de WMP): ondulación en direcciones rotadas que
        // cambian con el tiempo, más un término cruzado. Con ondas alineadas
        // a los ejes las manchas terminaban cuadradas.
        let t = fb.time;
        let d1 = vec2f(cos(t * 0.13), sin(t * 0.13));
        let d2 = vec2f(cos(t * 0.11 + 2.1), sin(t * 0.11 + 2.1));
        let d3 = vec2f(cos(-t * 0.07 + 4.2), sin(-t * 0.07 + 4.2));
        let w1 = sin(dot(p, d1) * fb.warp_freq + t);
        let w2 = sin(dot(p, d2) * fb.warp_freq * 0.73 - t * 1.3);
        let w3 = sin(dot(p, d3) * fb.warp_freq * 1.37 + t * 0.7 + w1);
        p += fb.warp_amp * (vec2f(-d1.y, d1.x) * w1 + vec2f(-d2.y, d2.x) * w2
            + vec2f(-d3.y, d3.x) * w3 * 0.6) * 0.7;
    } else {
        p += fb.warp_amp * sin(p.yx * fb.warp_freq + vec2f(fb.time, fb.time * 1.3));
    }
    p += fb.smear;

    let uv = p / vec2f(aspect, 1.0) + 0.5;
    let px = fb.blur / fb.resolution;

    // Blur de 5 taps + un tap hacia el centro (desenfoque radial). Se toman
    // los cuatro canales: el alfa lleva la intensidad del modo paleta.
    var acc4 = textureSample(prev_tex, samp, uv) * 0.4;
    if (fb.palette > 0.5) {
        // Disco de 8 taps (ángulo dorado) que gira cada cuadro: difunde en
        // todas direcciones por igual y no deja la rejilla de una cruz.
        let spin = fb.time * 2.399;
        for (var i = 0; i < 8; i = i + 1) {
            let fi = f32(i);
            let a = spin + fi * 2.39996;
            let r = sqrt((fi + 0.5) / 8.0) * 1.3;
            acc4 += textureSample(prev_tex, samp, uv + vec2f(cos(a), sin(a)) * r * px) * 0.075;
        }
    } else {
        acc4 += textureSample(prev_tex, samp, uv + vec2f(px.x, 0.0)) * 0.15;
        acc4 += textureSample(prev_tex, samp, uv - vec2f(px.x, 0.0)) * 0.15;
        acc4 += textureSample(prev_tex, samp, uv + vec2f(0.0, px.y)) * 0.15;
        acc4 += textureSample(prev_tex, samp, uv - vec2f(0.0, px.y)) * 0.15;
    }
    let toward_center = textureSample(prev_tex, samp, mix(uv, vec2f(0.5), fb.radial_blur));
    acc4 = mix(acc4, toward_center, min(fb.radial_blur * 8.0, 0.5));
    let acc = acc4.rgb;
    let scene4 = textureSample(scene_tex, samp, input.uv);

    if (fb.palette > 0.5) {
        // Intensidad: decae, la franja borra y la escena (canal rojo) suma.
        let eraser_p = fb.erase * clamp(-min(scene4.r, 0.0), 0.0, 1.0);
        var level = acc4.a * fb.fade * (1.0 - eraser_p);
        level = clamp(level + max(scene4.r, 0.0) * fb.inject, 0.0, 1.0);
        let low = mix(fb.base, fb.mid, clamp(level * 2.0, 0.0, 1.0));
        let col_p = mix(low, fb.high, clamp(level * 2.0 - 1.0, 0.0, 1.0));
        return vec4f(col_p, level);
    }

    // Fuera de la imagen no hay historia: evita que el borde se estire.
    let inside = step(0.0, uv.x) * step(uv.x, 1.0) * step(0.0, uv.y) * step(uv.y, 1.0);
    var col = max(hue_rotate(acc, fb.hue_shift), vec3f(0.0)) * fb.fade * inside
        + fb.base * (1.0 - fb.fade);

    // Borrador: la geometría con color negativo devuelve la zona al fondo.
    // El MSAA promedia el negativo en los bordes, así el corte es suave.
    let scene = scene4.rgb;
    let eraser = fb.erase * clamp(-min(scene.r, 0.0), 0.0, 1.0);
    col = mix(col, fb.base, eraser);
    col = min(col + max(scene, vec3f(0.0)) * fb.inject, vec3f(1.5));
    return vec4f(col, 1.0);
}
