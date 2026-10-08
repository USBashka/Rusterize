use rusterize::{shader::*, *};

pub static WAVE: ShaderSource = include!(concat!(env!("OUT_DIR"), "/wave.rs"));
pub static UV: ShaderSource = include!(concat!(env!("OUT_DIR"), "/uv.rs"));
pub static TEXTURE: ShaderSource = include!(concat!(env!("OUT_DIR"), "/texture.rs"));

pub fn checked_renderer() -> Result<ShaderRenderer, Error> {
    let renderer = ShaderRenderer::new()?;
    #[cfg(feature = "self-check")]
    let renderer = {
        let mut renderer = renderer;
        let image = Image::rgba(
            2,
            2,
            vec![
                255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 64, 255, 255, 255, 0,
            ],
        )?;
        let parameters = ShaderParams {
            data: [[1.0; 4], [0.0; 4], [0.0; 4], [0.0; 4]],
            ..Default::default()
        };
        let output =
            renderer.render(&TEXTURE.shader(parameters).with_image(&image), [2, 2], 1.0)?;
        if output.pixels() != image.pixels() {
            return Err(Error("Shader texture self-check failed".into()));
        }
        let params = ShaderParams {
            data: [[0.25, 0.5, 0.0, 0.0], [0.0; 4], [0.0; 4], [0.0; 4]],
            ..Default::default()
        };
        let output = renderer.render(&UV.shader(params), [2, 2], 1.0)?;
        for (i, p) in output.pixels().chunks_exact(4).enumerate() {
            let expected = [
                if i % 2 == 0 { 64 } else { 191 },
                if i < 2 { 64 } else { 191 },
                64,
                128,
            ];
            if p.iter()
                .zip(expected)
                .any(|(a, b)| (*a as i32 - b).abs() > 2)
            {
                return Err(Error(
                    "Shader coordinates/uniforms self-check failed".into(),
                ));
            }
        }
        renderer
    };
    Ok(renderer)
}

#[derive(Default)]
pub struct App {
    renderer: Option<ShaderRenderer>,
    time: f32,
    paused: bool,
    error: Option<String>,
}
impl Application for App {
    fn window_options(&self) -> WindowOptions {
        WindowOptions::default()
            .with_title("Rusterize · WGSL")
            .with_size(760.0, 540.0)
            .with_title_bar(TitleBarStyle::dark())
    }
    fn event(&mut self, event: Event, c: &mut Context) {
        match event {
            Event::Resume => c.set_animation(!self.paused && self.error.is_none()),
            Event::Suspend => {
                c.set_animation(false);
                self.renderer = None;
            }
            Event::Frame { delta, .. } if !self.paused => {
                if self.error.is_some() {
                    c.set_animation(false);
                } else {
                    self.time += delta as f32;
                }
            }
            Event::Pointer {
                phase: PointerPhase::Up,
                ..
            }
            | Event::Key {
                key: Key::Space,
                pressed: true,
                repeat: false,
                ..
            } => {
                self.paused = !self.paused;
                c.set_animation(!self.paused && self.error.is_none());
                c.request_redraw();
            }
            _ => {}
        }
    }
    fn draw(&mut self, c: &mut Canvas<'_>, viewport: Viewport) {
        c.clear(Color::hex(0x11151f));
        if self.renderer.is_none() && self.error.is_none() {
            match checked_renderer() {
                Ok(r) => self.renderer = Some(r),
                Err(e) => self.error = Some(e.to_string()),
            }
        }
        if let Some(error) = &self.error {
            c.text(error, (24.0, 48.0), TextStyle::new(16.0, Color::WHITE));
            return;
        }
        let width = (viewport.size.width - 48.0).max(1.0);
        let height = (viewport.size.height - 132.0).max(1.0);
        let rect = Rect::new(24.0, 76.0, width, height);
        c.text("Волны", (24.0, 46.0), TextStyle::new(28.0, Color::WHITE));
        c.with_save(|c| {
            c.clip(Shape::RoundedRect(rect, 24.0));
            c.shader(
                self.renderer.as_mut().unwrap(),
                rect,
                viewport.scale,
                &WAVE.shader(ShaderParams {
                    time: self.time,
                    ..Default::default()
                }),
            );
            c.text(
                "WGSL",
                (rect.x + 24.0, rect.y + 48.0),
                TextStyle::new(26.0, Color::WHITE).bold(),
            );
        });
        c.text(
            if self.paused {
                "Продолжить"
            } else {
                "Пауза"
            },
            (24.0, viewport.size.height - 22.0),
            TextStyle::new(16.0, Color::hex(0xbac6da)),
        );
    }
}
rusterize::export_app!(App);
