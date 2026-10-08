//! Build-time WGSL validation and translation. Never add this as a runtime dependency.
use naga::{back::glsl, valid, ShaderStage};
use std::{env, fs, path::Path};

const PRELUDE: &str = include_str!("prelude.wgsl");
const ENTRY: &str = "\n@fragment fn rusterize_fragment(@location(0) uv: vec2f) -> @location(0) vec4f { return shade(uv); }\n";

#[derive(Clone, Copy, Debug)]
pub enum Target {
    Desktop,
    Gles,
}

/// Validated sources and binding names obtained from Naga reflection.
pub struct Compiled {
    pub vertex: String,
    pub fragment: String,
    pub uniform_block: Option<String>,
    pub texture_uniform: Option<String>,
}

/// Compile a `fn shade(uv: vec2f) -> vec4f` module for an element.
/// Additional entry points, bindings, storage buffers and overrides are rejected.
pub fn compile(source: &str, target: Target) -> Result<Compiled, String> {
    let combined = format!("{PRELUDE}\n{source}{ENTRY}");
    let module =
        naga::front::wgsl::parse_str(&combined).map_err(|e| e.emit_to_string(&combined))?;
    if module.entry_points.len() != 2 || !module.overrides.is_empty() {
        return Err(
            "Define shade(uv: vec2f) -> vec4f; custom entry points and overrides are unsupported"
                .into(),
        );
    }
    for (_, global) in module.global_variables.iter() {
        if global.space == naga::AddressSpace::PushConstant
            || matches!(
                global.space,
                naga::AddressSpace::Storage { .. } | naga::AddressSpace::WorkGroup
            )
            || global
                .binding
                .as_ref()
                .is_some_and(|b| b.group != 0 || b.binding > 2)
        {
            return Err("Element shaders support only the supplied params, source_image and source_sampler resources".into());
        }
    }
    let info = valid::Validator::new(valid::ValidationFlags::all(), valid::Capabilities::empty())
        .validate(&module)
        .map_err(|e| e.emit_to_string(&combined))?;
    let options = glsl::Options {
        version: match target {
            Target::Desktop => glsl::Version::Desktop(330),
            Target::Gles => glsl::Version::new_gles(300),
        },
        writer_flags: glsl::WriterFlags::empty(),
        ..Default::default()
    };
    let write = |stage, entry: &str| -> Result<_, String> {
        let mut output = String::new();
        let pipeline = glsl::PipelineOptions {
            shader_stage: stage,
            entry_point: entry.into(),
            multiview: None,
        };
        let reflection = glsl::Writer::new(
            &mut output,
            &module,
            &info,
            &options,
            &pipeline,
            Default::default(),
        )
        .map_err(|e| e.to_string())?
        .write()
        .map_err(|e| e.to_string())?;
        // Pin the CPU/GPU ABI: shared uniform-block layout is implementation-defined.
        let first_line = output.find('\n').ok_or("Missing GLSL version")? + 1;
        output.insert_str(first_line, "layout(std140) uniform;\n");
        Ok((output, reflection))
    };
    let (vertex, _) = write(ShaderStage::Vertex, "rusterize_vertex")?;
    let (fragment, reflection) = write(ShaderStage::Fragment, "rusterize_fragment")?;
    let uniform_block = reflection.uniforms.iter().find_map(|(h, name)| {
        (module.global_variables[*h].name.as_deref() == Some("params")).then(|| name.clone())
    });
    let texture_uniform = reflection.texture_mapping.keys().next().cloned();
    if reflection.texture_mapping.len() > 1 {
        return Err("Only one sampled source image is supported".into());
    }
    Ok(Compiled {
        vertex,
        fragment,
        uniform_block,
        texture_uniform,
    })
}

impl Compiled {
    pub fn rust_expression(&self) -> String {
        format!("::rusterize::shader::ShaderSource {{ vertex: {:?}, fragment: {:?}, uniform_block: {:?}, texture_uniform: {:?} }}",
            self.vertex, self.fragment, self.uniform_block, self.texture_uniform)
    }
}

/// Use in build.rs: `compile_file("shaders/wave.wgsl", "wave.rs")?`.
/// Include OUT_DIR/wave.rs as a static `ShaderSource` in the application.
pub fn compile_file(input: impl AsRef<Path>, output_name: &str) -> Result<(), String> {
    let input = input.as_ref();
    if Path::new(output_name).file_name().and_then(|n| n.to_str()) != Some(output_name) {
        return Err("Output must be a file name inside OUT_DIR".into());
    }
    println!("cargo:rerun-if-changed={}", input.display());
    let target = match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("windows" | "macos") => Target::Desktop,
        Ok("linux" | "android") => Target::Gles,
        _ => return Err("Rusterize shaders support Windows, Linux, macOS and Android".into()),
    };
    let source = fs::read_to_string(input).map_err(|e| format!("{}: {e}", input.display()))?;
    let compiled = compile(&source, target).map_err(|e| format!("{}: {e}", input.display()))?;
    let output =
        env::var_os("OUT_DIR").ok_or("OUT_DIR is not set; call compile_file from build.rs")?;
    fs::write(
        Path::new(&output).join(output_name),
        compiled.rust_expression(),
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_and_translates_math_parameters_and_texture_for_both_profiles() {
        for target in [Target::Desktop, Target::Gles] {
            let result = compile("fn shade(uv: vec2f) -> vec4f { let x = sin(params.time + uv.x); return sample_image(uv) * params.data[0] * x; }", target).unwrap();
            assert!(result.uniform_block.is_some());
            assert!(result.texture_uniform.is_some());
            assert!(result.vertex.contains("void main()"));
        }
    }
    #[test]
    fn rejects_invalid_wgsl_and_unsupported_resources() {
        for source in [
            "fn shade(uv: vec2f) -> vec4f { return uv; }",
            "fn shade(uv: vec2f) -> vec4f { return unknown; }",
            "@group(1) @binding(0) var<uniform> extra: vec4f; fn shade(uv: vec2f) -> vec4f { return extra; }",
            "@compute @workgroup_size(1) fn compute() {} fn shade(uv: vec2f) -> vec4f { return vec4f(1); }",
        ] {
            assert!(compile(source, Target::Desktop).is_err(), "{source}");
        }
    }
}
