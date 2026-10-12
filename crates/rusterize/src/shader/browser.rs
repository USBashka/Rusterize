use super::*;
use crate::{native::Reader, protocol::Writer};
use std::{marker::PhantomData, rc::Rc};

/// WebGL 2 executor owned by the mounted browser host, with the same RGBA contract.
pub struct ShaderRenderer {
    id: u32,
    max_size: u32,
    name: String,
    last: Option<(CacheKey, Image)>,
    _thread: PhantomData<Rc<()>>,
}
impl ShaderRenderer {
    pub fn new() -> Result<Self, Error> {
        let response = crate::web::query(&30u32.to_le_bytes())?;
        let mut r = Reader::new(&response);
        let id = r.u32()?;
        let max_size = r.u32()?.min(4096);
        let name = r.string()?;
        if id == 0 || max_size == 0 || !r.done() {
            return Err(Error("invalid browser GPU context".into()));
        }
        Ok(Self {
            id,
            max_size,
            name,
            last: None,
            _thread: PhantomData,
        })
    }
    pub fn renderer_name(&self) -> &str {
        &self.name
    }
    /// Return straight-alpha, top-down RGBA8 at physical pixel resolution.
    /// Identical consecutive requests reuse the cached Image and its ID.
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
        if let Some((last, image)) = &self.last {
            if *last == key {
                return Ok(image.clone());
            }
        }
        let mut w = Writer(Vec::new());
        w.u32(31);
        w.u32(self.id);
        w.u32(key.source as u32);
        for s in [
            shader.source.vertex,
            shader.source.fragment,
            shader.source.uniform_block.unwrap_or(""),
            shader.source.texture_uniform.unwrap_or(""),
        ] {
            w.bytes(s.as_bytes());
        }
        for value in values {
            w.f32(value);
        }
        w.0.extend_from_slice(&key.image.to_le_bytes());
        let (width, height, pixels) = shader.image.map_or((1, 1, &[255u8; 4][..]), |i| {
            (i.width(), i.height(), i.pixels())
        });
        w.u32(width);
        w.u32(height);
        w.bytes(pixels);
        let pixels = crate::web::query(&w.0)?;
        let image = Image::rgba(size[0], size[1], pixels)?;
        self.last = Some((key, image.clone()));
        Ok(image)
    }
}
impl Drop for ShaderRenderer {
    fn drop(&mut self) {
        let mut w = Writer(Vec::new());
        w.u32(32);
        w.u32(self.id);
        let _ = crate::web::query(&w.0);
    }
}
