fn shade(uv: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(params.resolution.x / params.resolution.y, 1.0);
    let wave = sin(p.x * 9.0 + sin(p.y * 7.0 + params.time) + params.time * 1.6);
    let amount = wave * 0.5 + 0.5;
    let color = mix(vec3f(0.08, 0.24, 0.62), vec3f(0.84, 0.32, 0.60), amount);
    return vec4f(color, 1.0);
}
