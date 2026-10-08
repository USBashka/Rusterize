fn main() {
    for name in ["wave", "uv", "texture"] {
        rusterize_shader_build::compile_file(format!("shaders/{name}.wgsl"), &format!("{name}.rs"))
            .unwrap();
    }
}
