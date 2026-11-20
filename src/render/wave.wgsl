struct Globals {
    resolution: vec2f,
    _pad: vec2f,
}

@group(0) @binding(0) var<uniform> globals: Globals;

struct LineIn {
    @location(0) position: vec2f,
    @location(1) color: vec4f,
}

struct LineOut {
    @builtin(position) clip_position: vec4f,
    @location(0) color: vec4f,
}

@vertex
fn vs_line(input: LineIn) -> LineOut {
    var out: LineOut;
    let x = input.position.x / globals.resolution.x * 2.0 - 1.0;
    let y = 1.0 - input.position.y / globals.resolution.y * 2.0;
    out.clip_position = vec4f(x, y, 0.0, 1.0);
    out.color = input.color;
    return out;
}

@fragment
fn fs_line(input: LineOut) -> @location(0) vec4f {
    return input.color;
}
