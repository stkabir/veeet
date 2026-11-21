// Ambiente: gas de color fluido con fbm y domain warping, moduladas por
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
    morph: f32,
    custom: f32,
    brightness: f32,
    density: f32,
    warp: f32,
    _p0: f32,
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

// fbm de 5 octavas con rotación entre octavas para evitar artefactos de rejilla.
fn fbm(p: vec2f) -> f32 {
    var v = 0.0;
    var a = 0.5;
    var q = p;
    let rot = mat2x2f(0.8, 0.6, -0.6, 0.8);
    for (var i = 0; i < 5; i = i + 1) {
        v += a * noise(q);
        q = rot * q * 2.02 + vec2f(1.7, 9.2);
        a *= 0.5;
    }
    return v;
}

fn rot(p: vec2f, a: f32) -> vec2f {
    let c = cos(a);
    let s = sin(a);
    return vec2f(c * p.x - s * p.y, s * p.x + c * p.y);
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

// Gas: tres niveles de domain warping. `morph` (que salta en cada golpe)
// entra en cada nivel, así la silueta se reorganiza con el beat; el dominio
// además gira despacio y se advecta siguiendo el propio campo.
@fragment
fn fs_ambience(input: FullscreenOut) -> @location(0) vec4f {
    let aspect = amb.resolution.x / max(amb.resolution.y, 1.0);
    let t = amb.time;
    let m = amb.morph;
    // Pulso: el beat acerca la cámara un instante.
    let zoom = 1.0 - 0.10 * amb.beat;
    var p = (input.uv - 0.5) * vec2f(aspect, 1.0) * amb.scale * 2.0 * zoom;
    p = rot(p, t * 0.03 + 0.15 * sin(m * 0.21));

    let q = vec2f(
        fbm(p + vec2f(0.0, t * 0.12) + vec2f(m * 0.31, -m * 0.17)),
        fbm(p + vec2f(5.2, 1.3) - vec2f(t * 0.10, 0.0) + vec2f(-m * 0.23, m * 0.29)),
    );
    let warp = (1.8 + 2.0 * amb.bass + 1.2 * amb.beat) * amb.warp;
    // Advección: el gas se desplaza a lo largo de su propio campo.
    let adv = p + warp * q + (q - 0.5) * 1.5 * sin(m * 0.4);
    let r = vec2f(
        fbm(adv + vec2f(1.7, 9.2) + vec2f(t * 0.15, m * 0.19)),
        fbm(adv + vec2f(8.3, 2.8) - vec2f(m * 0.27, t * 0.126)),
    );
    let s = vec2f(
        fbm(p + warp * r + vec2f(3.1, 7.4) + m * 0.11),
        fbm(p + warp * r + vec2f(6.6, 0.9) - m * 0.13),
    );
    let sparkle = amb.treble * 0.6 * vec2f(sin(t * 1.3), cos(t * 1.1));
    let f = fbm(p + warp * s + sparkle);

    let col = palette(amb.hue + f * 0.6 + length(q) * 0.3 + 0.08 * sin(m * 0.5));
    // Umbral que se mueve con los graves: deja huecos oscuros y jirones
    // entre las nubes en vez de una mancha pareja.
    let lo = 0.32 - 0.12 * amb.bass - 0.06 * amb.beat + (1.0 - amb.density) * 0.3;
    let raw = f * f * 1.7 + length(r) * 0.25 + 0.15 * s.x;
    let density = smoothstep(lo, lo + 0.55, raw);
    // Filamentos: bordes finos más brillantes donde el gas cambia rápido.
    let wisp = smoothstep(0.0, 0.25, abs(s.y - r.x)) * 0.25;
    let brightness =
        (density + wisp * density) * (0.3 + 0.55 * amb.level + 0.6 * amb.beat) * amb.brightness;

    let c = input.uv - 0.5;
    let vignette = clamp(1.0 - dot(c, c) * 1.6, 0.0, 1.0);

    // Color al cuadrado: más saturado en espacio lineal.
    return vec4f(col * col * brightness * vignette, 1.0);
}
