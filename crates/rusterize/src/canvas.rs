use crate::*;

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Clear(Color),
    Save,
    Restore,
    Transform(Transform),
    Clip(Shape),
    Fill(Shape, Paint),
    Stroke(Shape, Paint, f32),
    Text {
        text: String,
        baseline: Point,
        style: TextStyle,
    },
    TextLayout {
        layout: TextLayout,
        origin: Point,
    },
    Image {
        image: Image,
        destination: Rect,
        opacity: f32,
    },
}

/// Reusable display list. A failed frame is cleared, never partly submitted.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub(crate) commands: Vec<Command>,
}
impl Scene {
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }
    pub fn capacity(&self) -> usize {
        self.commands.capacity()
    }
}

/// Immediate drawing recorder. Call `finish` before submitting the scene.
pub struct Canvas<'a> {
    scene: &'a mut Scene,
    depth: usize,
    error: Option<Error>,
    finished: bool,
}
impl<'a> Canvas<'a> {
    pub fn new(scene: &'a mut Scene) -> Self {
        scene.commands.clear();
        Self {
            scene,
            depth: 0,
            error: None,
            finished: false,
        }
    }
    /// Replace the entire surface. Only valid as the first command in a frame.
    pub fn clear(&mut self, color: Color) {
        if !self.scene.commands.is_empty() {
            self.fail("clear must be the first command");
            return;
        }
        self.scene.commands.push(Command::Clear(color));
    }
    pub fn save(&mut self) {
        self.depth += 1;
        self.scene.commands.push(Command::Save);
    }
    pub fn restore(&mut self) {
        if self.depth == 0 {
            self.fail("restore without save");
            return;
        }
        self.depth -= 1;
        self.scene.commands.push(Command::Restore);
    }
    /// Balanced local drawing state, including nested clips and transforms.
    pub fn with_save(&mut self, draw: impl FnOnce(&mut Self)) {
        self.save();
        let depth = self.depth;
        draw(self);
        if self.depth != depth {
            self.fail("unbalanced save/restore inside with_save");
        } else {
            self.restore();
        }
    }
    pub fn transform(&mut self, transform: Transform) {
        self.scene.commands.push(Command::Transform(transform));
    }
    pub fn translate(&mut self, x: f32, y: f32) {
        self.transform(Transform::translate(x, y));
    }
    pub fn clip(&mut self, shape: impl Into<Shape>) {
        self.scene.commands.push(Command::Clip(shape.into()));
    }
    pub fn fill(&mut self, shape: impl Into<Shape>, paint: impl Into<Paint>) {
        self.scene
            .commands
            .push(Command::Fill(shape.into(), paint.into()));
    }
    /// Strokes use butt caps, miter joins and miter limit 10 on every backend.
    pub fn stroke(&mut self, shape: impl Into<Shape>, paint: impl Into<Paint>, width: f32) {
        self.scene
            .commands
            .push(Command::Stroke(shape.into(), paint.into(), width));
    }
    pub fn fill_rect(&mut self, rect: Rect, paint: impl Into<Paint>) {
        self.fill(rect, paint);
    }
    pub fn rounded_rect(&mut self, rect: Rect, radius: f32, paint: impl Into<Paint>) {
        self.fill(Shape::RoundedRect(rect, radius), paint);
    }
    pub fn ellipse(&mut self, rect: Rect, paint: impl Into<Paint>) {
        self.fill(Shape::Ellipse(rect), paint);
    }
    pub fn circle(&mut self, center: impl Into<Point>, radius: f32, paint: impl Into<Paint>) {
        let p = center.into();
        self.ellipse(
            Rect::new(p.x - radius, p.y - radius, radius * 2.0, radius * 2.0),
            paint,
        );
    }
    pub fn line(
        &mut self,
        from: impl Into<Point>,
        to: impl Into<Point>,
        paint: impl Into<Paint>,
        width: f32,
    ) {
        self.stroke(
            Path::builder().move_to(from).line_to(to).build(),
            paint,
            width,
        );
    }
    /// Single-line UTF-8 text. Baseline and size are in logical pixels.
    pub fn text(&mut self, text: impl Into<String>, baseline: impl Into<Point>, style: TextStyle) {
        self.scene.commands.push(Command::Text {
            text: text.into(),
            baseline: baseline.into(),
            style,
        });
    }
    pub fn measure_text(&self, text: &str, style: &TextStyle) -> Result<TextMetrics, Error> {
        crate::measure_text(text, style)
    }
    /// Draw the same native layout that supplied the metrics, at its top-left origin.
    pub fn text_layout(&mut self, layout: &TextLayout, origin: impl Into<Point>) {
        self.scene.commands.push(Command::TextLayout {
            layout: layout.clone(),
            origin: origin.into(),
        });
    }
    pub fn image(&mut self, image: &Image, destination: Rect, opacity: f32) {
        self.scene.commands.push(Command::Image {
            image: image.clone(),
            destination,
            opacity,
        });
    }
    pub(crate) fn fail(&mut self, message: &str) {
        if self.error.is_none() {
            self.error = Some(Error(message.into()));
        }
    }
    pub fn finish(mut self) -> Result<(), Error> {
        if self.depth != 0 {
            self.fail("unbalanced save/restore at end of frame");
        }
        if self.scene.commands.len() > 1_000_000 {
            self.fail("too many drawing commands");
        }
        let mut transform = Transform::IDENTITY;
        let mut transforms = Vec::new();
        for command in &self.scene.commands {
            let valid = match command {
                Command::Clear(_) => true,
                Command::Save => {
                    transforms.push(transform);
                    true
                }
                Command::Restore => {
                    if let Some(t) = transforms.pop() {
                        transform = t;
                        true
                    } else {
                        false
                    }
                }
                Command::Transform(t) => {
                    transform = transform.concat(*t);
                    transform.0.iter().all(|v| v.is_finite()) && transform.inverse().is_some()
                }
                Command::Clip(s) => s.valid(),
                Command::Fill(s, p) => s.valid() && p.valid(),
                Command::Stroke(s, p, w) => s.valid() && p.valid() && w.is_finite() && *w > 0.0,
                Command::Text {
                    text,
                    baseline,
                    style,
                } => {
                    baseline.finite()
                        && style.valid()
                        && text.len() <= 1024 * 1024
                        && !text.contains(['\n', '\r', '\0'])
                }
                Command::TextLayout { origin, .. } => origin.finite(),
                Command::Image {
                    destination,
                    opacity,
                    ..
                } => destination.valid() && opacity.is_finite() && (0.0..=1.0).contains(opacity),
            };
            if !valid {
                self.error
                    .get_or_insert_with(|| Error("invalid drawing geometry, paint or text".into()));
                break;
            }
        }
        if let Some(error) = self.error.take() {
            self.scene.commands.clear();
            self.finished = true;
            Err(error)
        } else {
            self.finished = true;
            Ok(())
        }
    }
}
impl Drop for Canvas<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.scene.commands.clear();
        }
    }
}

/// Adapter contract: replay in order, preserve logical coordinates and surface errors.
pub trait Renderer {
    fn render(&mut self, scene: &Scene, viewport: Viewport) -> Result<(), Error>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_frames_are_atomic() {
        let mut s = Scene::default();
        let mut c = Canvas::new(&mut s);
        c.clear(Color::BLACK);
        c.restore();
        assert!(c.finish().is_err());
        assert!(s.commands().is_empty());
        let mut c = Canvas::new(&mut s);
        c.circle((0.0, 0.0), f32::NAN, Color::WHITE);
        assert!(c.finish().is_err());
        assert!(s.commands().is_empty());
    }
    #[test]
    fn scopes_and_abandoned_frames() {
        let mut s = Scene::default();
        let mut c = Canvas::new(&mut s);
        c.with_save(|c| {
            c.translate(4.0, 5.0);
            c.clip(Rect::new(0.0, 0.0, 4.0, 4.0));
        });
        c.finish().unwrap();
        assert_eq!(s.commands().len(), 4);
        {
            let mut c = Canvas::new(&mut s);
            c.clear(Color::BLACK);
        }
        assert!(s.commands().is_empty());
    }
    #[test]
    fn image_length_checked() {
        assert!(Image::rgba(u32::MAX, u32::MAX, Vec::new()).is_err());
    }
    #[test]
    fn composed_transform_overflow_is_rejected() {
        let mut s = Scene::default();
        let mut c = Canvas::new(&mut s);
        c.translate(3.0e38, 0.0);
        c.translate(3.0e38, 0.0);
        assert!(c.finish().is_err());
        assert!(s.commands().is_empty());
    }
}
