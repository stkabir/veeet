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

struct HighPassParams {
    threshold: f32,
    smooth_width: f32,
    _pad: vec2f,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> params: HighPassParams;

fn luminance(rgb: vec3f) -> f32 {
    return dot(rgb, vec3f(0.2126, 0.7152, 0.0722));
}

@fragment
fn fs_highpass(input: FullscreenOut) -> @location(0) vec4f {
    let texel = textureSample(source, source_sampler, input.uv);
    let luma = luminance(texel.rgb);
    let alpha = smoothstep(
        params.threshold,
        params.threshold + params.smooth_width,
        luma,
    );
    return vec4f(texel.rgb * alpha, 1.0);
}
