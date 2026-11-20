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

struct BlurParams {
    inv_size: vec2f,
    direction: vec2f,
    kernel_radius: u32,
    mip: u32,
    _pad: vec2f,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> params: BlurParams;
@group(0) @binding(3) var<storage, read> coefficients: array<f32>;

@fragment
fn fs_blur(input: FullscreenOut) -> @location(0) vec4f {
    let radius = params.kernel_radius;
    let base = params.mip * 11u;

    var weight_sum = coefficients[base];
    var diffuse_sum = textureSample(source, source_sampler, input.uv).rgb * weight_sum;

    for (var i: u32 = 1u; i < radius; i = i + 1u) {
        let x = f32(i);
        let w = coefficients[base + i];
        let offset = params.direction * params.inv_size * x;
        let sample1 = textureSample(source, source_sampler, input.uv + offset).rgb;
        let sample2 = textureSample(source, source_sampler, input.uv - offset).rgb;
        diffuse_sum += (sample1 + sample2) * w;
        weight_sum += 2.0 * w;
    }

    return vec4f(diffuse_sum / weight_sum, 1.0);
}
