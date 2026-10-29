//! Stable little-endian display-list wire format for thin native hosts.
//! See `docs/protocol.md`. This is an in-process trusted transport, not a file parser.
use crate::*;
use std::collections::{BTreeMap, BTreeSet};

pub const MAGIC: &[u8; 4] = b"RZ03";

/// Encode a self-contained resource snapshot and display list, reusing `output`.
/// Use [`Encoder`] across frames to upload each live resource only once.
pub fn encode(scene: &Scene, output: &mut Vec<u8>) {
    Encoder::default().encode(scene, output);
}

/// Resource state for one receiving host. Submit every encoded frame in order.
/// Resource operations precede the full display list; omitted resources remain
/// cached until a release operation or an explicit receiver reset.
#[derive(Default)]
pub struct Encoder {
    images: BTreeSet<u64>,
    #[cfg(feature = "shaders")]
    shaders: BTreeSet<u64>,
}
impl Encoder {
    /// Forget the receiver's resource state. Clear its cache before the next frame.
    /// The next call to `encode` uploads all resources used by that scene again.
    pub fn reset(&mut self) {
        self.images.clear();
        #[cfg(feature = "shaders")]
        self.shaders.clear();
    }

    /// Encode resource changes followed by a complete display list.
    /// If a frame is dropped or the receiver cache is lost, reset both sides.
    pub fn encode(&mut self, scene: &Scene, output: &mut Vec<u8>) {
        output.clear();
        output.extend_from_slice(MAGIC);
        let mut w = Writer(std::mem::take(output));
        let mut images = BTreeMap::new();
        #[cfg(feature = "shaders")]
        let mut shaders = BTreeMap::new();
        for command in scene.commands() {
            match command {
                Command::Image { image, .. } => {
                    images.insert(image.id(), image);
                }
                #[cfg(feature = "shaders")]
                Command::Shader { source, image, .. } => {
                    shaders.insert(*source as *const shader::ShaderSource as u64, *source);
                    if let Some(image) = image {
                        images.insert(image.id(), image);
                    }
                }
                _ => {}
            }
        }
        let resource_count = self
            .images
            .iter()
            .filter(|id| !images.contains_key(id))
            .count()
            + images.keys().filter(|id| !self.images.contains(id)).count();
        #[cfg(feature = "shaders")]
        let resource_count = resource_count
            + self
                .shaders
                .iter()
                .filter(|id| !shaders.contains_key(id))
                .count()
            + shaders
                .keys()
                .filter(|id| !self.shaders.contains(id))
                .count();
        w.u32(resource_count as u32);
        for id in self.images.iter().filter(|id| !images.contains_key(id)) {
            w.u32(2);
            w.u64(*id);
        }
        for (id, image) in &images {
            if !self.images.contains(id) {
                w.u32(1);
                w.u64(*id);
                w.u32(image.width());
                w.u32(image.height());
                w.bytes(image.pixels());
            }
        }
        self.images = images.into_keys().collect();
        #[cfg(feature = "shaders")]
        {
            for id in self.shaders.iter().filter(|id| !shaders.contains_key(id)) {
                w.u32(4);
                w.u64(*id);
            }
            for (id, source) in &shaders {
                if !self.shaders.contains(id) {
                    w.u32(3);
                    w.u64(*id);
                    w.bytes(source.vertex.as_bytes());
                    w.bytes(source.fragment.as_bytes());
                    w.bytes(source.uniform_block.unwrap_or("").as_bytes());
                    w.bytes(source.texture_uniform.unwrap_or("").as_bytes());
                }
            }
            self.shaders = shaders.into_keys().collect();
        }
        w.u32(scene.commands().len() as u32);
        for command in scene.commands() {
            match command {
                Command::Clear(c) => {
                    w.u32(1);
                    w.color(*c);
                }
                Command::Save => w.u32(2),
                Command::Restore => w.u32(3),
                Command::Transform(t) => {
                    w.u32(4);
                    for v in t.0 {
                        w.f32(v);
                    }
                }
                Command::Clip(s) => {
                    w.u32(5);
                    w.shape(s);
                }
                Command::Fill(s, p) => {
                    w.u32(6);
                    w.shape(s);
                    w.paint(p);
                }
                Command::Stroke(s, p, width) => {
                    w.u32(7);
                    w.shape(s);
                    w.paint(p);
                    w.f32(*width);
                }
                Command::Text {
                    text,
                    baseline,
                    style,
                } => {
                    w.u32(8);
                    w.point(*baseline);
                    w.text(text, style, TextOptions::default());
                }
                Command::TextLayout { layout, origin } => {
                    w.u32(10);
                    w.point(*origin);
                    w.text(&layout.text, &layout.style, layout.options);
                }
                Command::Image {
                    image,
                    destination,
                    opacity,
                } => {
                    w.u32(9);
                    w.u64(image.id());
                    w.rect(*destination);
                    w.f32(*opacity);
                }
                #[cfg(feature = "shaders")]
                Command::Shader {
                    source,
                    params,
                    image,
                    destination,
                    size,
                    scale,
                } => {
                    w.u32(11);
                    w.u64(*source as *const shader::ShaderSource as u64);
                    for value in [size[0] as f32, size[1] as f32, params.time, *scale] {
                        w.f32(value);
                    }
                    for value in params.data.iter().flatten() {
                        w.f32(*value);
                    }
                    w.u64(image.as_ref().map_or(0, Image::id));
                    w.rect(*destination);
                }
            }
        }
        *output = w.0;
    }
}
pub(crate) struct Writer(pub Vec<u8>);
impl Writer {
    pub(crate) fn text(&mut self, text: &str, style: &TextStyle, options: TextOptions) {
        self.f32(style.size);
        self.color(style.color);
        self.u32(style.family as u32);
        self.u32(
            u32::from(style.bold)
                | (u32::from(style.italic) << 1)
                | (u32::from(style.underline) << 2)
                | (u32::from(style.strikethrough) << 3),
        );
        self.bytes(style.font_name.as_deref().unwrap_or("").as_bytes());
        self.f32(options.width);
        self.u32(options.wrap as u32);
        self.u32(options.align as u32);
        self.u32(options.direction as u32);
        self.bytes(text.as_bytes());
    }
    pub(crate) fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub(crate) fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn color(&mut self, c: Color) {
        self.0.extend_from_slice(&[c.r, c.g, c.b, c.a]);
    }
    pub(crate) fn point(&mut self, p: Point) {
        self.f32(p.x);
        self.f32(p.y);
    }
    fn rect(&mut self, r: Rect) {
        for v in [r.x, r.y, r.width, r.height] {
            self.f32(v);
        }
    }
    pub(crate) fn bytes(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.0.extend_from_slice(b);
    }
    fn shape(&mut self, s: &Shape) {
        match s {
            Shape::Rect(r) => {
                self.u32(0);
                self.rect(*r);
            }
            Shape::RoundedRect(r, v) => {
                self.u32(1);
                self.rect(*r);
                self.f32((*v).min(r.width / 2.0).min(r.height / 2.0));
            }
            Shape::Ellipse(r) => {
                self.u32(2);
                self.rect(*r);
            }
            Shape::Path(p) => {
                self.u32(3);
                self.u32(p.segments().len() as u32);
                for s in p.segments() {
                    match s {
                        Segment::Move(p) => {
                            self.u32(0);
                            self.point(*p);
                        }
                        Segment::Line(p) => {
                            self.u32(1);
                            self.point(*p);
                        }
                        Segment::Cubic(a, b, c) => {
                            self.u32(2);
                            self.point(*a);
                            self.point(*b);
                            self.point(*c);
                        }
                        Segment::Close => self.u32(3),
                    }
                }
            }
        }
    }
    fn paint(&mut self, p: &Paint) {
        match p {
            Paint::Solid(c) => {
                self.u32(0);
                self.color(*c);
            }
            Paint::Linear {
                from,
                to,
                start,
                end,
            } => {
                self.u32(1);
                self.point(*from);
                self.point(*to);
                self.color(*start);
                self.color(*end);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn golden_frame_and_buffer_reuse() {
        let mut scene = Scene::default();
        let mut c = Canvas::new(&mut scene);
        c.clear(Color::rgba(1, 2, 3, 4));
        c.fill_rect(Rect::new(0.0, 0.0, 1.0, 2.0), Color::WHITE);
        c.finish().unwrap();
        let mut b = Vec::with_capacity(256);
        encode(&scene, &mut b);
        let mut expected =
            b"RZ03\0\0\0\0\x02\0\0\0\x01\0\0\0\x01\x02\x03\x04\x06\0\0\0\0\0\0\0".to_vec();
        for n in [0.0f32, 0.0, 1.0, 2.0] {
            expected.extend_from_slice(&n.to_le_bytes());
        }
        expected.extend_from_slice(&[0, 0, 0, 0, 255, 255, 255, 255]);
        assert_eq!(b, expected);
        let ptr = b.as_ptr();
        encode(&scene, &mut b);
        assert_eq!(ptr, b.as_ptr());
    }

    #[derive(Debug, PartialEq)]
    enum Resource {
        UploadImage(u64, u32, u32, Vec<u8>),
        ReleaseImage(u64),
        UploadShader(u64, [Vec<u8>; 4]),
        ReleaseShader(u64),
    }
    struct Reader<'a>(&'a [u8]);
    impl<'a> Reader<'a> {
        fn take(&mut self, size: usize) -> &'a [u8] {
            let (bytes, rest) = self.0.split_at(size);
            self.0 = rest;
            bytes
        }
        fn u32(&mut self) -> u32 {
            u32::from_le_bytes(self.take(4).try_into().unwrap())
        }
        fn u64(&mut self) -> u64 {
            u64::from_le_bytes(self.take(8).try_into().unwrap())
        }
        fn bytes(&mut self) -> Vec<u8> {
            let size = self.u32() as usize;
            self.take(size).to_vec()
        }
    }
    fn resources(bytes: &[u8]) -> (Vec<Resource>, u32, &[u8]) {
        let mut reader = Reader(bytes);
        assert_eq!(reader.take(4), b"RZ03");
        let count = reader.u32();
        let ops = (0..count)
            .map(|_| match reader.u32() {
                1 => {
                    Resource::UploadImage(reader.u64(), reader.u32(), reader.u32(), reader.bytes())
                }
                2 => Resource::ReleaseImage(reader.u64()),
                3 => Resource::UploadShader(reader.u64(), std::array::from_fn(|_| reader.bytes())),
                4 => Resource::ReleaseShader(reader.u64()),
                op => panic!("unknown resource opcode {op}"),
            })
            .collect();
        (ops, reader.u32(), reader.0)
    }
    fn image_scene(images: &[&Image]) -> Scene {
        let mut scene = Scene::default();
        let mut canvas = Canvas::new(&mut scene);
        for image in images {
            canvas.image(image, Rect::new(1.0, 2.0, 3.0, 4.0), 0.5);
        }
        canvas.finish().unwrap();
        scene
    }
    #[test]
    fn image_upload_is_unique_and_drawing_contains_only_its_id() {
        let image = Image::rgba(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        let clone = image.clone();
        let scene = image_scene(&[&image, &clone]);
        let mut encoder = Encoder::default();
        let mut bytes = Vec::new();
        encoder.encode(&scene, &mut bytes);
        let (ops, count, drawing) = resources(&bytes);
        assert_eq!(
            ops,
            vec![Resource::UploadImage(
                image.id(),
                2,
                1,
                image.pixels().to_vec()
            )]
        );
        assert_eq!(count, 2);
        let mut expected = 9u32.to_le_bytes().to_vec();
        expected.extend_from_slice(&image.id().to_le_bytes());
        for value in [1.0f32, 2.0, 3.0, 4.0, 0.5] {
            expected.extend_from_slice(&value.to_le_bytes());
        }
        assert_eq!(drawing, expected.repeat(2));
        let first_size = bytes.len();
        encoder.encode(&scene, &mut bytes);
        assert_eq!(
            resources(&bytes),
            (vec![], 2, expected.repeat(2).as_slice())
        );
        assert_eq!(first_size - bytes.len(), 24 + image.pixels().len());
    }
    #[test]
    fn absent_images_are_released_and_returning_images_are_uploaded() {
        let first = Image::rgba(1, 1, vec![255, 0, 0, 255]).unwrap();
        let second = Image::rgba(1, 1, vec![0, 255, 0, 255]).unwrap();
        let mut encoder = Encoder::default();
        let mut bytes = Vec::new();
        encoder.encode(&image_scene(&[&first]), &mut bytes);
        encoder.encode(&image_scene(&[&second]), &mut bytes);
        assert_eq!(
            resources(&bytes).0,
            vec![
                Resource::ReleaseImage(first.id()),
                Resource::UploadImage(second.id(), 1, 1, second.pixels().to_vec()),
            ]
        );
        encoder.encode(&Scene::default(), &mut bytes);
        assert_eq!(
            resources(&bytes),
            (vec![Resource::ReleaseImage(second.id())], 0, &[][..])
        );
        encoder.encode(&Scene::default(), &mut bytes);
        assert_eq!(resources(&bytes), (vec![], 0, &[][..]));
        encoder.encode(&image_scene(&[&first]), &mut bytes);
        assert_eq!(
            resources(&bytes).0,
            vec![Resource::UploadImage(
                first.id(),
                1,
                1,
                first.pixels().to_vec()
            )]
        );
    }
    #[test]
    fn reset_and_stateless_encoding_produce_complete_resource_snapshots() {
        let image = Image::rgba(1, 1, vec![0, 0, 255, 128]).unwrap();
        let scene = image_scene(&[&image]);
        let mut encoder = Encoder::default();
        let mut bytes = Vec::new();
        encoder.encode(&scene, &mut bytes);
        let snapshot = bytes.clone();
        encoder.encode(&scene, &mut bytes);
        assert!(resources(&bytes).0.is_empty());
        encoder.reset();
        encoder.encode(&scene, &mut bytes);
        assert_eq!(bytes, snapshot);
        for _ in 0..2 {
            encode(&scene, &mut bytes);
            assert_eq!(bytes, snapshot);
        }
    }
    #[test]
    fn repeated_large_bitmap_frame_is_smaller_than_one_hundred_bytes() {
        let image = Image::rgba(1024, 1024, vec![255; 1024 * 1024 * 4]).unwrap();
        let scene = image_scene(&[&image]);
        let mut encoder = Encoder::default();
        let mut bytes = Vec::new();
        encoder.encode(&scene, &mut bytes);
        assert_eq!(bytes.len(), image.pixels().len() + 68);
        encoder.encode(&scene, &mut bytes);
        assert_eq!(bytes.len(), 44);
        assert!(resources(&bytes).0.is_empty());
    }
    #[cfg(feature = "shaders")]
    #[test]
    fn shader_sources_and_input_images_follow_resource_lifetime() {
        static SOURCE: shader::ShaderSource = shader::ShaderSource {
            vertex: "vertex",
            fragment: "fragment",
            uniform_block: Some("params"),
            texture_uniform: None,
        };
        let id = &SOURCE as *const shader::ShaderSource as u64;
        let image = Image::rgba(1, 1, vec![3, 6, 9, 128]).unwrap();
        let scene = Scene {
            commands: vec![Command::Shader {
                source: &SOURCE,
                params: shader::ShaderParams {
                    time: 3.0,
                    data: [[4.0; 4]; 4],
                },
                image: Some(image.clone()),
                destination: Rect::new(1.0, 2.0, 8.0, 6.0),
                size: [16, 12],
                scale: 2.0,
            }],
        };
        let mut encoder = Encoder::default();
        let mut bytes = Vec::new();
        encoder.encode(&scene, &mut bytes);
        let (ops, count, drawing) = resources(&bytes);
        assert_eq!(
            ops,
            vec![
                Resource::UploadImage(image.id(), 1, 1, image.pixels().to_vec()),
                Resource::UploadShader(
                    id,
                    [
                        b"vertex".to_vec(),
                        b"fragment".to_vec(),
                        b"params".to_vec(),
                        vec![]
                    ]
                ),
            ]
        );
        assert_eq!(count, 1);
        let mut expected = 11u32.to_le_bytes().to_vec();
        expected.extend_from_slice(&id.to_le_bytes());
        for value in [16.0f32, 12.0, 3.0, 2.0].into_iter().chain([4.0; 16]) {
            expected.extend_from_slice(&value.to_le_bytes());
        }
        expected.extend_from_slice(&image.id().to_le_bytes());
        for value in [1.0f32, 2.0, 8.0, 6.0] {
            expected.extend_from_slice(&value.to_le_bytes());
        }
        assert_eq!(drawing, expected);
        let snapshot = bytes.clone();
        encoder.encode(&scene, &mut bytes);
        assert_eq!(resources(&bytes), (vec![], 1, expected.as_slice()));
        // A shader-only input remains live when it moves into an ordinary image draw.
        encoder.encode(&image_scene(&[&image]), &mut bytes);
        assert_eq!(resources(&bytes).0, vec![Resource::ReleaseShader(id)]);
        encoder.encode(&Scene::default(), &mut bytes);
        assert_eq!(
            resources(&bytes).0,
            vec![Resource::ReleaseImage(image.id())]
        );
        encoder.encode(&scene, &mut bytes);
        assert_eq!(bytes, snapshot);
        encoder.reset();
        encoder.encode(&scene, &mut bytes);
        assert_eq!(bytes, snapshot);
    }
}
