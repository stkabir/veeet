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

struct FinalParams {
    exposure: f32,
    black_bg: f32,
    grid_enabled: f32,
    grid_rows: f32,
    grid_overlap: f32,
    grid_opacity: f32,
    hue_shift: f32,
    saturation: f32,
    contrast: f32,
    tint_amount: f32,
    vignette: f32,
    mirror: f32,
    scale: f32,
    _pad: f32,
    offset: vec2f,
    tint: vec4f,
    background: vec4f,
}

@group(0) @binding(0) var scene_source: texture_2d<f32>;
@group(0) @binding(1) var scene_sampler: sampler;
@group(0) @binding(2) var<uniform> params: FinalParams;

fn scene_color(uv: vec2f) -> vec3f {
    return textureSample(scene_source, scene_sampler, uv).rgb;
}

// Rotación de tono en YIQ: conserva la luminancia.
fn rotate_hue(color: vec3f, turns: f32) -> vec3f {
    let yiq = mat3x3f(
        vec3f(0.299, 0.596, 0.211),
        vec3f(0.587, -0.274, -0.523),
        vec3f(0.114, -0.322, 0.312),
    ) * color;
    let a = turns * 6.2831853;
    let c = cos(a);
    let s = sin(a);
    let rotated = vec3f(yiq.x, yiq.y * c - yiq.z * s, yiq.y * s + yiq.z * c);
    return mat3x3f(
        vec3f(1.0, 1.0, 1.0),
        vec3f(0.956, -0.272, -1.106),
        vec3f(0.621, -0.647, 1.703),
    ) * rotated;
}

fn grade(color_in: vec3f, uv: vec2f) -> vec3f {
    var color = color_in;
    if (abs(params.hue_shift) > 0.0001) {
        color = max(rotate_hue(color, params.hue_shift), vec3f(0.0));
    }
    let luma = dot(color, vec3f(0.2126, 0.7152, 0.0722));
    color = mix(vec3f(luma), color, params.saturation);
    color = mix(color, luma * params.tint.rgb * 1.5, params.tint_amount);
    color = max((color - vec3f(0.5)) * params.contrast + vec3f(0.5), vec3f(0.0));
    if (params.vignette > 0.0001) {
        let d = length(uv - vec2f(0.5)) * 1.41421356;
        color *= 1.0 - params.vignette * smoothstep(0.35, 1.0, d);
    }
    return color;
}

@fragment
fn fs_final(input_raw: FullscreenOut) -> @location(0) vec4f {
    var input = input_raw;
    // Encuadre: tamaño y posición de la escena; lo que queda fuera es fondo.
    input.uv = (input.uv - 0.5 - params.offset * vec2f(1.0, -1.0)) / max(params.scale, 0.05) + 0.5;
    let outside = any(input.uv < vec2f(0.0)) || any(input.uv > vec2f(1.0));
    let mirror = u32(params.mirror + 0.5);
    if ((mirror & 1u) != 0u) {
        input.uv.x = 0.5 - abs(input.uv.x - 0.5);
    }
    if ((mirror & 2u) != 0u) {
        input.uv.y = 0.5 - abs(input.uv.y - 0.5);
    }
    var original = scene_color(input.uv);
    if (outside) {
        original = vec3f(0.0);
    }
    var color = original;

    if (params.grid_enabled > 0.5) {
        let rows = max(1.0, params.grid_rows);
        let tile_h = 1.0 / rows;
        let overlap = clamp(params.grid_overlap, 0.0, 1.0);
        let home = floor(input.uv.y / tile_h);
        var accumulated = vec3f(0.0);

        for (var iy = -1; iy <= 1; iy = iy + 1) {
            let tile_y = home + f32(iy);
            if (tile_y < 0.0 || tile_y > rows - 1.0) {
                continue;
            }

            let core_start = tile_y * tile_h;
            let core_end = core_start + tile_h;
            let overlap_size = overlap * tile_h;
            let expanded_start = core_start - overlap_size;
            let expanded_end = core_end + overlap_size;

            if (input.uv.y < expanded_start || input.uv.y > expanded_end) {
                continue;
            }

            let dist_outside = max(
                max(core_start - input.uv.y, input.uv.y - core_end),
                0.0,
            ) / max(overlap_size, 0.001);
            let fade = 1.0 - smoothstep(0.0, 1.0, dist_outside);

            // Con una columna y zoom 1, cada fila es una copia desplazada 1/rows.
            let sample_y = input.uv.y + 0.5 - (tile_y + 0.5) * tile_h;
            let tiled = scene_color(vec2f(input.uv.x, clamp(sample_y, 0.0, 1.0)));
            accumulated += tiled * fade;
        }

        let tiled = clamp(accumulated, vec3f(0.0), vec3f(1.0));
        color = mix(original, tiled, params.grid_opacity);
    }

    color *= params.exposure;
    color = grade(color, input_raw.uv);
    // El fondo asoma donde la escena no tiene luz.
    let light = clamp(max(color.r, max(color.g, color.b)), 0.0, 1.0);
    color += params.background.rgb * (1.0 - light);
    let rgb = clamp(color, vec3f(0.0), vec3f(1.0));

    if (params.black_bg > 0.5) {
        return vec4f(rgb, 1.0);
    }

    let alpha = clamp(max(rgb.r, max(rgb.g, rgb.b)) * 1.25, 0.0, 1.0);
    return vec4f(rgb * alpha, alpha);
}
