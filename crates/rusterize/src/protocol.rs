//! Stable little-endian display-list wire format for thin native hosts.
//! See `docs/protocol.md`. This is an in-process trusted transport, not a file parser.
use crate::*;

pub const MAGIC: &[u8; 4] = b"RZ01";

/// Encode a complete frame, reusing the destination allocation.
pub fn encode(scene: &Scene, output: &mut Vec<u8>) {
    output.clear();
    output.extend_from_slice(MAGIC);
    let mut w = Writer(output);
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
                w.f32(style.size);
                w.color(style.color);
                w.u32(match style.family {
                    FontFamily::Sans => 0,
                    FontFamily::Serif => 1,
                    FontFamily::Monospace => 2,
                });
                w.u32(u32::from(style.bold));
                w.bytes(text.as_bytes());
            }
            Command::Image {
                image,
                destination,
                opacity,
            } => {
                w.u32(9);
                w.0.extend_from_slice(&image.id().to_le_bytes());
                w.u32(image.width());
                w.u32(image.height());
                w.rect(*destination);
                w.f32(*opacity);
                w.bytes(image.pixels());
            }
        }
    }
}
struct Writer<'a>(&'a mut Vec<u8>);
impl Writer<'_> {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn color(&mut self, c: Color) {
        self.0.extend_from_slice(&[c.r, c.g, c.b, c.a]);
    }
    fn point(&mut self, p: Point) {
        self.f32(p.x);
        self.f32(p.y);
    }
    fn rect(&mut self, r: Rect) {
        for v in [r.x, r.y, r.width, r.height] {
            self.f32(v);
        }
    }
    fn bytes(&mut self, b: &[u8]) {
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
        let mut expected = b"RZ01\x02\0\0\0\x01\0\0\0\x01\x02\x03\x04\x06\0\0\0\0\0\0\0".to_vec();
        for n in [0.0f32, 0.0, 1.0, 2.0] {
            expected.extend_from_slice(&n.to_le_bytes());
        }
        expected.extend_from_slice(&[0, 0, 0, 0, 255, 255, 255, 255]);
        assert_eq!(b, expected);
        let ptr = b.as_ptr();
        encode(&scene, &mut b);
        assert_eq!(ptr, b.as_ptr());
    }
}
