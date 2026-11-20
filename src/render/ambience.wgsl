// Ambiente: nubes de color fluidas con fbm y domain warping, moduladas por
// el audio (graves deforman, agudos agregan detalle, el beat da un destello).

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

// Debe coincidir campo a campo con `AmbienceParams` en ambience.rs (112 B).
struct AmbienceParams {
    resolution: vec2f,
    time: f32,
    scale: f32,
    bass: f32,
    mid: f32,
    treble: f32,
    level: f32,
    beat: f32,
    hue: f32,
    mode: f32,
    custom: f32,
    brightness: f32,
    _p0: f32,
    _p1: f32,
    _p2: f32,
    color_a: vec4f,
    color_b: vec4f,
    color_c: vec4f,
}

@group(0) @binding(0) var<uniform> amb: AmbienceParams;

// Hash entero (estilo PCG) sobre la celda: estable con coordenadas grandes,
// a diferencia de fract(sin(x)·k), que en f32 produce bloques.
fn hash(cell: vec2f) -> f32 {
    var v = bitcast<vec2u>(vec2i(cell));
    v = v * 1664525u + 1013904223u;
    v.x += v.y * 1664525u;
    v.y += v.x * 1664525u;
    v = v ^ (v >> vec2u(16u));
    v.x += v.y * 1664525u;
    v.y += v.x * 1664525u;
    v = v ^ (v >> vec2u(16u));
    return f32(v.x) * (1.0 / 4294967296.0);
}

fn noise(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash(i);
    let b = hash(i + vec2f(1.0, 0.0));
    let c = hash(i + vec2f(0.0, 1.0));
    let d = hash(i + vec2f(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// fbm de 4 octavas con rotación entre octavas para evitar artefactos de rejilla.
fn fbm(p: vec2f) -> f32 {
    var v = 0.0;
    var a = 0.5;
    var q = p;
    let rot = mat2x2f(0.8, 0.6, -0.6, 0.8);
    for (var i = 0; i < 4; i = i + 1) {
        v += a * noise(q);
        q = rot * q * 2.02 + vec2f(1.7, 9.2);
        a *= 0.5;
    }
    return v;
}

// Paleta coseno (Íñigo Quílez): ciclo de color continuo desplazado por `hue`.
fn palette(t: f32) -> vec3f {
    if (amb.custom > 0.5) {
        // Degradado cíclico a → b → c → a con los colores del preset.
        let x = fract(t) * 3.0;
        if (x < 1.0) {
            return mix(amb.color_a.rgb, amb.color_b.rgb, x);
        } else if (x < 2.0) {
            return mix(amb.color_b.rgb, amb.color_c.rgb, x - 1.0);
        }
        return mix(amb.color_c.rgb, amb.color_a.rgb, x - 2.0);
    }
    let d = vec3f(0.0, 0.33, 0.67);
    return 0.55 + 0.45 * cos(6.2831853 * (t + d));
}

// Huracán: nubes en espiral con ojo oscuro y pared brillante. La coordenada
// radial ln(r) − t hace que las nubes fluyan hacia afuera, como si la cámara
// avanzara hacia el ojo. cos/sin del ángulo evitan la costura en ±π.
fn hurricane_clouds(uv: vec2f, aspect: f32) -> vec3f {
    let p = (uv - 0.5) * vec2f(aspect, 1.0);
    let r = max(length(p), 1e-3);
    let t = amb.time;
    let eye = max(amb.scale, 0.02);
    let lr = log(r / eye);
    let a = atan2(p.y, p.x) + 1.9 * lr - t * 0.35;
    let dir = vec2f(cos(a), sin(a));
    let radial = lr * 2.2 - t * 1.4;
    let f = fbm(dir * 1.6 + vec2f(radial, 0.0) + amb.bass * 0.4 * dir);
    let g = fbm(dir * 3.1 + vec2f(radial * 1.7, 4.3));
    let clouds = smoothstep(0.3, 0.85, f * 0.75 + g * 0.45);

    // Ojo oscuro, pared brillante alrededor y caída hacia los bordes.
    let inside = smoothstep(eye * 0.9, eye * 1.7, r);
    let wall = exp(-pow((r - eye * 1.25) / (eye * 0.35), 2.0));
    let outer = 1.0 - smoothstep(0.45, 1.0, r);
    let density = clouds * inside * outer + wall * (0.4 + 0.6 * f);

    var low = vec3f(0.30, 0.38, 0.62);
    var high = vec3f(0.85, 0.90, 1.0);
    var flash_tint = vec3f(0.6, 0.45, 1.0);
    if (amb.custom > 0.5) {
        low = amb.color_a.rgb;
        high = amb.color_b.rgb;
        flash_tint = amb.color_c.rgb;
    }
    let base = mix(low, high, f);
    // Relámpago dentro de la nube en cada beat.
    let flash = flash_tint * amb.beat * 0.35 * g;
    let brightness = 0.14 + 0.24 * amb.level + 0.2 * amb.mid;
    return base * base * density * brightness + flash * density;
}

@fragment
fn fs_ambience(input: FullscreenOut) -> @location(0) vec4f {
    let aspect = amb.resolution.x / max(amb.resolution.y, 1.0);
    if (amb.mode > 0.5) {
        return vec4f(hurricane_clouds(input.uv, aspect), 1.0);
    }
    // Pulso: el beat acerca la cámara un instante y las nubes "laten".
    let zoom = 1.0 - 0.12 * amb.beat;
    let p = (input.uv - 0.5) * vec2f(aspect, 1.0) * amb.scale * 2.0 * zoom;
    let t = amb.time;

    let q = vec2f(
        fbm(p + vec2f(0.0, t * 0.12)),
        fbm(p + vec2f(5.2, 1.3) - vec2f(t * 0.10, 0.0)),
    );
    let warp = 1.6 + 1.8 * amb.bass + 0.8 * amb.beat;
    let r = vec2f(
        fbm(p + warp * q + vec2f(1.7, 9.2) + vec2f(t * 0.15, 0.0)),
        fbm(p + warp * q + vec2f(8.3, 2.8) - vec2f(0.0, t * 0.126)),
    );
    let sparkle = amb.treble * 0.6 * vec2f(sin(t * 1.3), cos(t * 1.1));
    let f = fbm(p + warp * r + sparkle);

    let col = palette(amb.hue + f * 0.55 + length(q) * 0.35);
    let density = smoothstep(0.15, 0.9, f * f * 1.6 + length(r) * 0.3);
    let brightness = density * (0.3 + 0.55 * amb.level + 0.5 * amb.beat) * amb.brightness;

    let c = input.uv - 0.5;
    let vignette = clamp(1.0 - dot(c, c) * 1.6, 0.0, 1.0);

    // Color al cuadrado: más saturado en espacio lineal.
    return vec4f(col * col * brightness * vignette, 1.0);
}
