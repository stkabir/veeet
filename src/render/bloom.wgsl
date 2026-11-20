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

struct BloomParams {
    strength: f32,
    radius: f32,
    _pad: vec2f,
}

@group(0) @binding(0) var blur1: texture_2d<f32>;
@group(0) @binding(1) var blur2: texture_2d<f32>;
@group(0) @binding(2) var blur3: texture_2d<f32>;
@group(0) @binding(3) var blur4: texture_2d<f32>;
@group(0) @binding(4) var blur5: texture_2d<f32>;
@group(0) @binding(5) var bloom_sampler: sampler;
@group(0) @binding(6) var<uniform> params: BloomParams;

fn lerp_bloom_factor(factor: f32) -> f32 {
    let mirror_factor = 1.2 - factor;
    return mix(factor, mirror_factor, params.radius);
}

@fragment
fn fs_bloom_composite(input: FullscreenOut) -> @location(0) vec4f {
    let factors = array<f32, 5>(1.0, 0.8, 0.6, 0.4, 0.2);
    let w0 = lerp_bloom_factor(factors[0]);
    let w1 = lerp_bloom_factor(factors[1]);
    let w2 = lerp_bloom_factor(factors[2]);
    let w3 = lerp_bloom_factor(factors[3]);
    let w4 = lerp_bloom_factor(factors[4]);

    let s0 = textureSample(blur1, bloom_sampler, input.uv);
    let s1 = textureSample(blur2, bloom_sampler, input.uv);
    let s2 = textureSample(blur3, bloom_sampler, input.uv);
    let s3 = textureSample(blur4, bloom_sampler, input.uv);
    let s4 = textureSample(blur5, bloom_sampler, input.uv);

    let bloom = params.strength * (
        w0 * s0.rgb + w1 * s1.rgb + w2 * s2.rgb + w3 * s3.rgb + w4 * s4.rgb
    );
    let alpha = params.strength * (
        w0 * s0.a + w1 * s1.a + w2 * s2.a + w3 * s3.a + w4 * s4.a
    );

    return vec4f(bloom, alpha);
}
