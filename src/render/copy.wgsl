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

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;

@fragment
fn fs_copy(input: FullscreenOut) -> @location(0) vec4f {
    let texel = textureSample(source, source_sampler, input.uv);
    // El blend material de Vizz usa AdditiveBlending con el factor alfa
    // efectivamente recortado a 1; el RGB del bloom se suma completo.
    return vec4f(texel.rgb, 1.0);
}
