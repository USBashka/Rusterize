fn shade(uv: vec2f) -> vec4f {
    return sample_image(uv) * params.data[0];
}
