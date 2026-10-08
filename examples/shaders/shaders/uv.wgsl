fn shade(uv: vec2f) -> vec4f {
    return vec4f(uv, params.data[0].x, params.data[0].y);
}
