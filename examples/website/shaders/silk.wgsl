fn shade(uv: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(params.resolution.x / params.resolution.y, 1.0);
    let t = params.time * 0.35;
    let bend = sin(p.x * 3.5 + t) * 0.25 + cos(p.x * 6.0 - t) * 0.08;
    let wave = sin((p.y + bend) * (17.0 + params.data[0].x * 16.0) + t);
    let light = pow(wave * 0.5 + 0.5, 2.4);
    let base = mix(vec3f(0.10, 0.16, 0.15), params.data[0].yzw, light);
    let sheen = pow(max(0.0, sin((p.y + bend) * 11.0 - t)), 12.0);
    return vec4f(base + vec3f(0.25, 0.19, 0.10) * sheen, 1.0);
}
