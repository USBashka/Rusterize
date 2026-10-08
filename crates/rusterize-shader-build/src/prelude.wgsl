struct ElementParams {
    resolution: vec2f,
    time: f32,
    scale: f32,
    data: array<vec4f, 4>,
}
@group(0) @binding(0) var<uniform> params: ElementParams;
@group(0) @binding(1) var source_image: texture_2d<f32>;
@group(0) @binding(2) var source_sampler: sampler;

fn sample_image(uv: vec2f) -> vec4f {
    return textureSample(source_image, source_sampler, uv);
}

struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
}
@vertex
fn rusterize_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let p = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return VertexOutput(vec4f(p * 2.0 - 1.0, 0.0, 1.0), vec2f(p.x, 1.0 - p.y));
}
