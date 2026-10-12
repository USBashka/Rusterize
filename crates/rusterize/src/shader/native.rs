use super::*;
use glow::HasContext;
use std::{marker::PhantomData, rc::Rc};
#[path = "platform/mod.rs"]
mod platform;

/// Keep one renderer per UI thread. GPU contexts, programs and surfaces are reused.
/// This type is deliberately neither Send nor Sync. Construction can fail when
/// the system has no usable OpenGL 3.3 / OpenGL ES 3.0 implementation.
pub struct ShaderRenderer {
    gl: glow::Context,
    native: platform::NativeContext,
    programs: Vec<(usize, glow::Program)>,
    vertex_array: Option<glow::VertexArray>,
    uniforms: Option<glow::Buffer>,
    framebuffer: Option<glow::Framebuffer>,
    output: Option<glow::Texture>,
    input: Option<glow::Texture>,
    size: [u32; 2],
    input_id: Option<u64>,
    max_size: u32,
    last: Option<(CacheKey, Image)>,
    renderer_name: String,
    _thread: PhantomData<Rc<()>>,
}

impl ShaderRenderer {
    pub fn new() -> Result<Self, Error> {
        let native = platform::NativeContext::new()?;
        let _current = native.activate()?;
        // SAFETY: the owned native context is current and outlives all GL calls.
        let gl = unsafe { glow::Context::from_loader_function(|s| native.get_proc(s)) };
        let mut renderer = Self {
            gl,
            native,
            programs: Vec::new(),
            vertex_array: None,
            uniforms: None,
            framebuffer: None,
            output: None,
            input: None,
            size: [0, 0],
            input_id: None,
            max_size: 0,
            last: None,
            renderer_name: String::new(),
            _thread: PhantomData,
        };
        unsafe {
            let g = &renderer.gl;
            let version = g.version();
            if (version.is_embedded && version.major < 3)
                || (!version.is_embedded && (version.major, version.minor) < (3, 3))
            {
                return Err(Error(format!(
                    "Shaders require OpenGL 3.3 / ES 3.0; found {version:?}"
                )));
            }
            renderer.renderer_name = format!(
                "{} / {}",
                g.get_parameter_string(glow::RENDERER),
                g.get_parameter_string(glow::VERSION)
            );
            renderer.max_size = (g.get_parameter_i32(glow::MAX_TEXTURE_SIZE) as u32).min(4096);
            renderer.vertex_array = Some(g.create_vertex_array().map_err(Error)?);
            renderer.uniforms = Some(g.create_buffer().map_err(Error)?);
            renderer.framebuffer = Some(g.create_framebuffer().map_err(Error)?);
            renderer.output = Some(g.create_texture().map_err(Error)?);
            renderer.input = Some(g.create_texture().map_err(Error)?);
            for texture in [renderer.input, renderer.output] {
                g.bind_texture(glow::TEXTURE_2D, texture);
                g.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MIN_FILTER,
                    glow::LINEAR as i32,
                );
                g.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MAG_FILTER,
                    glow::LINEAR as i32,
                );
                g.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_S,
                    glow::CLAMP_TO_EDGE as i32,
                );
                g.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_T,
                    glow::CLAMP_TO_EDGE as i32,
                );
            }
            renderer.check_error()?;
        }
        Ok(renderer)
    }

    /// Actual driver, including software implementations such as Mesa llvmpipe.
    pub fn renderer_name(&self) -> &str {
        &self.renderer_name
    }

    /// Execute at physical pixel resolution, returning straight-alpha, top-down RGBA8.
    /// `scale` is exposed to WGSL; UV always spans the element from (0,0) to (1,1).
    /// Identical consecutive requests return the same cached Image (including its ID).
    pub fn render(
        &mut self,
        shader: &Shader<'_>,
        size: [u32; 2],
        scale: f32,
    ) -> Result<Image, Error> {
        validate_request(shader, size, scale, self.max_size)?;
        let mut values = [0.0f32; 20];
        values[..4].copy_from_slice(&[size[0] as f32, size[1] as f32, shader.params.time, scale]);
        for (out, value) in values[4..]
            .iter_mut()
            .zip(shader.params.data.iter().flatten())
        {
            *out = *value;
        }
        let key = CacheKey {
            source: shader.source as *const _ as usize,
            size,
            values: values.map(f32::to_bits),
            image: shader.image.map_or(0, Image::id),
        };
        if let Some((last_key, image)) = &self.last {
            if last_key == &key {
                return Ok(image.clone());
            }
        }
        let _current = self.native.activate()?;
        let program = self.program(shader.source)?;
        unsafe {
            let g = &self.gl;
            g.use_program(Some(program));
            g.bind_vertex_array(self.vertex_array);
            g.bind_buffer(glow::UNIFORM_BUFFER, self.uniforms);
            let bytes: Vec<_> = values.iter().flat_map(|n| n.to_ne_bytes()).collect();
            g.buffer_data_u8_slice(glow::UNIFORM_BUFFER, &bytes, glow::DYNAMIC_DRAW);
            g.bind_buffer_base(glow::UNIFORM_BUFFER, 0, self.uniforms);
            if size != self.size {
                g.bind_texture(glow::TEXTURE_2D, self.output);
                g.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    size[0] as i32,
                    size[1] as i32,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(None),
                );
                self.check_error()?;
                self.size = size;
            }
            g.bind_framebuffer(glow::FRAMEBUFFER, self.framebuffer);
            g.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                self.output,
                0,
            );
            if g.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                return Err(Error("Shader framebuffer is incomplete".into()));
            }
            g.active_texture(glow::TEXTURE0);
            g.bind_texture(glow::TEXTURE_2D, self.input);
            if self.input_id != Some(key.image) {
                let (width, height, pixels) = shader.image.map_or((1, 1, &[255u8; 4][..]), |i| {
                    (i.width(), i.height(), i.pixels())
                });
                g.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
                g.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    width as i32,
                    height as i32,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(pixels)),
                );
                self.check_error()?;
                self.input_id = Some(key.image);
            }
            g.viewport(0, 0, size[0] as i32, size[1] as i32);
            g.disable(glow::BLEND);
            g.disable(glow::DEPTH_TEST);
            g.disable(glow::SCISSOR_TEST);
            g.disable(glow::DITHER);
            g.clear_color(0.0, 0.0, 0.0, 0.0);
            g.clear(glow::COLOR_BUFFER_BIT);
            g.draw_arrays(glow::TRIANGLES, 0, 3);
            let stride = size[0] as usize * 4;
            let mut pixels = vec![0; stride * size[1] as usize];
            g.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            g.read_pixels(
                0,
                0,
                size[0] as i32,
                size[1] as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
            self.check_error()?;
            // GL readback starts at the bottom; Canvas image rows start at the top.
            for y in 0..size[1] as usize / 2 {
                let opposite = (size[1] as usize - 1 - y) * stride;
                let (first, second) = pixels.split_at_mut(opposite);
                first[y * stride..(y + 1) * stride].swap_with_slice(&mut second[..stride]);
            }
            let image = Image::rgba(size[0], size[1], pixels)?;
            self.last = Some((key, image.clone()));
            Ok(image)
        }
    }

    fn program(&mut self, source: &'static ShaderSource) -> Result<glow::Program, Error> {
        let id = source as *const _ as usize;
        if let Some(index) = self.programs.iter().position(|(key, _)| *key == id) {
            let entry = self.programs.remove(index);
            self.programs.push(entry);
            return Ok(entry.1);
        }
        unsafe {
            let g = &self.gl;
            let program = g.create_program().map_err(Error)?;
            let mut stages = Vec::new();
            let result = (|| {
                for (kind, text) in [
                    (glow::VERTEX_SHADER, source.vertex),
                    (glow::FRAGMENT_SHADER, source.fragment),
                ] {
                    let stage = g.create_shader(kind).map_err(Error)?;
                    stages.push(stage);
                    g.shader_source(stage, text);
                    g.compile_shader(stage);
                    if !g.get_shader_compile_status(stage) {
                        return Err(Error(format!(
                            "Native shader compilation: {}",
                            g.get_shader_info_log(stage)
                        )));
                    }
                    g.attach_shader(program, stage);
                }
                g.link_program(program);
                if !g.get_program_link_status(program) {
                    return Err(Error(format!(
                        "Native shader link: {}",
                        g.get_program_info_log(program)
                    )));
                }
                g.use_program(Some(program));
                if let Some(name) = source.uniform_block {
                    if let Some(index) = g.get_uniform_block_index(program, name) {
                        g.uniform_block_binding(program, index, 0);
                    }
                }
                if let Some(name) = source.texture_uniform {
                    g.uniform_1_i32(g.get_uniform_location(program, name).as_ref(), 0);
                }
                self.check_error()
            })();
            for stage in stages {
                g.delete_shader(stage);
            }
            if let Err(error) = result {
                g.delete_program(program);
                return Err(error);
            }
            if self.programs.len() == 32 {
                let (_, old) = self.programs.remove(0);
                g.delete_program(old);
            }
            self.programs.push((id, program));
            Ok(program)
        }
    }

    fn check_error(&self) -> Result<(), Error> {
        let error = unsafe { self.gl.get_error() };
        if error == glow::NO_ERROR {
            Ok(())
        } else {
            Err(Error(format!("Shader OpenGL error: 0x{error:04x}")))
        }
    }
}

impl Drop for ShaderRenderer {
    fn drop(&mut self) {
        if let Ok(_current) = self.native.activate() {
            unsafe {
                for (_, program) in self.programs.drain(..) {
                    self.gl.delete_program(program);
                }
                if let Some(v) = self.vertex_array {
                    self.gl.delete_vertex_array(v);
                }
                if let Some(v) = self.uniforms {
                    self.gl.delete_buffer(v);
                }
                if let Some(v) = self.framebuffer {
                    self.gl.delete_framebuffer(v);
                }
                if let Some(v) = self.output {
                    self.gl.delete_texture(v);
                }
                if let Some(v) = self.input {
                    self.gl.delete_texture(v);
                }
            }
        }
    }
}
