use rusterize::*;

const BG: Color = Color::hex(0x0c121c);
const PANEL: Color = Color::hex(0x172231);
const INK: Color = Color::hex(0xe8edf5);
const MUTED: Color = Color::hex(0x94a5bc);
const ACCENTS: [Color; 3] = [
    Color::hex(0x79e2c0),
    Color::hex(0xaf9bff),
    Color::hex(0xffb578),
];

pub struct Gallery {
    count: u32,
    accent: usize,
    radius: f32,
    animate: bool,
    angle: f32,
    drag: Option<(u64, u8)>,
    pressed: Option<(u64, u8)>,
    position: Point,
    viewport: Viewport,
    image: Image,
    focus: u8,
}
impl Default for Gallery {
    fn default() -> Self {
        let mut pixels = Vec::new();
        for y in 0..32 {
            for x in 0..32 {
                let c = if ((x / 4) + (y / 4)) % 2 == 0 {
                    ACCENTS[0]
                } else {
                    Color::hex(0x31577b)
                };
                pixels.extend_from_slice(&[c.r, c.g, c.b, 255]);
            }
        }
        Self {
            count: 0,
            accent: 0,
            radius: 38.0,
            animate: false,
            angle: 0.0,
            drag: None,
            pressed: None,
            position: Point::new(0.5, 0.5),
            viewport: Viewport::default(),
            image: Image::rgba(32, 32, pixels).unwrap(),
            focus: 0,
        }
    }
}
struct Layout {
    stage: Rect,
    controls: Rect,
    toggle: Rect,
    reset: Rect,
    plus: Rect,
    slider: Rect,
    swatches: [Rect; 3],
    bottom: f32,
}
impl Layout {
    fn new(v: Viewport) -> Self {
        let w = v.size.width;
        let h = v.size.height;
        let pad = 24.0;
        let compact = w < 720.0;
        let content = (w - pad * 2.0).max(200.0);
        let stage = if compact {
            Rect::new(pad, 132.0, content, (h - 420.0).clamp(150.0, 320.0))
        } else {
            Rect::new(
                pad,
                132.0,
                (content - 240.0 - 20.0).max(200.0),
                (h - 300.0).max(200.0),
            )
        };
        let controls = if compact {
            Rect::new(pad, stage.y + stage.height + 16.0, content, 170.0)
        } else {
            Rect::new(stage.x + stage.width + 20.0, 132.0, 240.0, stage.height)
        };
        let toggle = Rect::new(pad, 82.0, 142.0, 32.0);
        let reset = Rect::new(pad + 152.0, 82.0, 100.0, 32.0);
        let plus = if compact {
            Rect::new(
                controls.x + controls.width - 100.0,
                controls.y + 110.0,
                80.0,
                38.0,
            )
        } else {
            Rect::new(
                controls.x + 20.0,
                controls.y + controls.height - 58.0,
                controls.width - 40.0,
                38.0,
            )
        };
        let slider = Rect::new(
            controls.x + 20.0,
            controls.y + 91.0,
            controls.width - 40.0,
            24.0,
        );
        let swatches = std::array::from_fn(|i| {
            Rect::new(
                controls.x + 20.0 + i as f32 * 48.0,
                controls.y + 40.0,
                34.0,
                28.0,
            )
        });
        let bottom = controls.y + controls.height + 18.0;
        Self {
            stage,
            controls,
            toggle,
            reset,
            plus,
            slider,
            swatches,
            bottom,
        }
    }
    fn button(&self, p: Point) -> Option<u8> {
        if self.toggle.contains(p) {
            Some(0)
        } else if self.reset.contains(p) {
            Some(1)
        } else if self.plus.contains(p) {
            Some(2)
        } else {
            self.swatches
                .iter()
                .position(|r| r.contains(p))
                .map(|n| n as u8 + 3)
        }
    }
}
impl Gallery {
    fn action(&mut self, button: u8, c: &mut Context) {
        match button {
            0 => {
                self.animate = !self.animate;
                c.set_animation(self.animate);
            }
            1 => {
                self.position = Point::new(0.5, 0.5);
                self.radius = 38.0;
                self.angle = 0.0;
                self.count = 0;
            }
            2 => self.count = self.count.saturating_add(1),
            3..=5 => self.accent = (button - 3) as usize,
            _ => {}
        }
        c.request_redraw();
    }
    fn drag_to(&mut self, kind: u8, p: Point) {
        let l = Layout::new(self.viewport);
        if kind == 0 {
            self.position = Point::new(
                ((p.x - l.stage.x) / l.stage.width).clamp(0.12, 0.88),
                ((p.y - l.stage.y) / l.stage.height).clamp(0.2, 0.8),
            );
        } else {
            self.radius = 20.0 + ((p.x - l.slider.x) / l.slider.width).clamp(0.0, 1.0) * 44.0;
        }
    }
}
impl Application for Gallery {
    fn window_options(&self) -> WindowOptions {
        WindowOptions {
            min_size: Size::new(360.0, 560.0),
            ..WindowOptions::default()
        }
        .with_title("Rusterize — Студия форм")
        .with_title_bar(TitleBarStyle::colors(BG, INK).with_border(Color::hex(0x283a4c)))
    }
    fn event(&mut self, event: Event, c: &mut Context) {
        match event {
            Event::Resize(v) => self.viewport = v,
            Event::Frame { delta, .. } => {
                self.angle = (self.angle + delta as f32 * 0.7) % std::f32::consts::TAU;
            }
            Event::Pointer {
                id,
                phase,
                position,
                ..
            } => {
                let l = Layout::new(self.viewport);
                match phase {
                    PointerPhase::Down => {
                        if self.drag.is_none() && self.pressed.is_none() {
                            if l.slider.contains(position) {
                                self.drag = Some((id, 1));
                                self.drag_to(1, position);
                            } else if let Some(button) = l.button(position) {
                                self.pressed = Some((id, button));
                                self.focus = button;
                            } else if l.stage.contains(position) {
                                self.drag = Some((id, 0));
                                self.drag_to(0, position);
                            }
                            c.request_redraw();
                        }
                    }
                    PointerPhase::Move => {
                        if let Some((active, kind)) = self.drag {
                            if active == id {
                                self.drag_to(kind, position);
                                c.request_redraw();
                            }
                        }
                    }
                    PointerPhase::Up | PointerPhase::Cancel => {
                        if self.drag.is_some_and(|(active, _)| active == id) {
                            self.drag = None;
                        }
                        if let Some((active, button)) = self.pressed {
                            if active == id {
                                self.pressed = None;
                                if phase == PointerPhase::Up && l.button(position) == Some(button) {
                                    self.action(button, c);
                                }
                            }
                        }
                        c.request_redraw();
                    }
                }
            }
            Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
            } => match key {
                Key::Tab => {
                    self.focus = if modifiers.shift {
                        (self.focus + 5) % 6
                    } else {
                        (self.focus + 1) % 6
                    };
                    c.request_redraw();
                }
                Key::Space | Key::Enter => self.action(self.focus, c),
                Key::Left => {
                    self.radius = (self.radius - 2.0).max(20.0);
                    c.request_redraw();
                }
                Key::Right => {
                    self.radius = (self.radius + 2.0).min(64.0);
                    c.request_redraw();
                }
                Key::Escape => c.exit(),
                _ => {}
            },
            Event::Focus(false) | Event::Suspend => {
                self.drag = None;
                self.pressed = None;
            }
            _ => {}
        }
    }
    fn draw(&mut self, c: &mut Canvas<'_>, v: Viewport) {
        self.viewport = v;
        let l = Layout::new(v);
        let accent = ACCENTS[self.accent];
        c.clear(BG);
        c.circle((30.0, 36.0), 5.0, accent);
        c.text("Rusterize", (46.0, 44.0), TextStyle::new(26.0, INK).bold());
        if v.size.width > 560.0 {
            c.text(
                "Студия форм",
                (v.size.width - 176.0, 42.0),
                TextStyle::new(18.0, MUTED),
            );
        }
        for (i, r, label) in [
            (
                0,
                l.toggle,
                if self.animate {
                    "Приостановить"
                } else {
                    "Анимация"
                },
            ),
            (1, l.reset, "Сброс"),
        ] {
            c.rounded_rect(
                r,
                8.0,
                if self.pressed.is_some_and(|(_, b)| b == i) {
                    Color::hex(0x34465d)
                } else {
                    PANEL
                },
            );
            c.text(label, (r.x + 14.0, r.y + 21.0), TextStyle::new(14.0, INK));
            if self.focus == i {
                c.stroke(
                    Shape::RoundedRect(r.inset(1.0), 7.0),
                    accent.with_alpha(170),
                    1.0,
                );
            }
        }
        c.rounded_rect(
            l.stage,
            18.0,
            Paint::Linear {
                from: (l.stage.x, l.stage.y).into(),
                to: (l.stage.x + l.stage.width, l.stage.y + l.stage.height).into(),
                start: Color::hex(0x1e3447),
                end: Color::hex(0x16212f),
            },
        );
        c.with_save(|c| {
            c.clip(Shape::RoundedRect(l.stage, 18.0));
            for row in 0..((l.stage.height / 28.0) as i32) {
                for col in 0..((l.stage.width / 28.0) as i32) {
                    c.circle(
                        (
                            l.stage.x + 20.0 + col as f32 * 28.0,
                            l.stage.y + 20.0 + row as f32 * 28.0,
                        ),
                        1.0,
                        Color::rgba(158, 190, 211, 35),
                    );
                }
            }
            let center = Point::new(
                l.stage.x + l.stage.width * self.position.x,
                l.stage.y + l.stage.height * self.position.y,
            );
            c.with_save(|c| {
                c.translate(center.x, center.y);
                c.transform(Transform::rotate(self.angle));
                c.stroke(
                    Shape::Ellipse(Rect::new(-100.0, -56.0, 200.0, 112.0)),
                    accent.with_alpha(95),
                    1.5,
                );
                c.stroke(
                    Shape::Ellipse(Rect::new(-68.0, -92.0, 136.0, 184.0)),
                    accent.with_alpha(50),
                    1.0,
                );
                c.circle((92.0, -22.0), 8.0, accent);
                c.rounded_rect(
                    Rect::new(-116.0, 12.0, 25.0, 25.0),
                    7.0,
                    Color::hex(0xffb578),
                );
                c.circle((0.0, 0.0), self.radius + 10.0, accent.with_alpha(14));
                c.circle(
                    (0.0, 0.0),
                    self.radius,
                    Paint::Linear {
                        from: (-self.radius, -self.radius).into(),
                        to: (self.radius, self.radius).into(),
                        start: accent,
                        end: Color::hex(0x337c9b),
                    },
                );
                c.circle(
                    (-self.radius * 0.28, -self.radius * 0.34),
                    self.radius * 0.16,
                    Color::WHITE.with_alpha(170),
                );
            });
            c.text(
                "Перемещайте фигуру",
                (l.stage.x + 18.0, l.stage.y + l.stage.height - 18.0),
                TextStyle::new(13.0, MUTED),
            );
        });
        c.rounded_rect(l.controls, 16.0, PANEL);
        c.text(
            "Цвет",
            (l.controls.x + 20.0, l.controls.y + 26.0),
            TextStyle::new(14.0, MUTED),
        );
        for (i, r) in l.swatches.iter().enumerate() {
            c.rounded_rect(*r, 8.0, ACCENTS[i]);
            if i == self.accent {
                c.stroke(Shape::RoundedRect(r.inset(-3.0), 10.0), INK, 1.5);
            }
        }
        let track = Rect::new(l.slider.x, l.slider.y + 10.0, l.slider.width, 4.0);
        c.rounded_rect(track, 2.0, Color::hex(0x34485f));
        let progress = (self.radius - 20.0) / 44.0;
        c.rounded_rect(
            Rect::new(track.x, track.y, track.width * progress, track.height),
            2.0,
            accent,
        );
        c.circle((track.x + track.width * progress, track.y + 2.0), 7.0, INK);
        let count_y = if v.size.width < 720.0 {
            l.controls.y + 138.0
        } else {
            l.controls.y + 160.0
        };
        c.text(
            format!("Нажатия: {}", self.count),
            (l.controls.x + 20.0, count_y),
            TextStyle::new(17.0, INK),
        );
        c.rounded_rect(l.plus, 9.0, accent);
        c.text(
            "+1",
            (l.plus.x + l.plus.width / 2.0 - 9.0, l.plus.y + 25.0),
            TextStyle::new(17.0, BG).bold(),
        );
        if self.focus == 2 {
            c.stroke(Shape::RoundedRect(l.plus.inset(-3.0), 11.0), INK, 1.0);
        }
        let y = l.bottom;
        let tile_w = (v.size.width - 64.0) / 3.0;
        if y + 116.0 <= v.size.height {
            for i in 0..3 {
                c.rounded_rect(
                    Rect::new(24.0 + i as f32 * (tile_w + 8.0), y, tile_w, 104.0),
                    12.0,
                    PANEL,
                );
            }
            let wave = Path::builder()
                .move_to((40.0, y + 66.0))
                .cubic_to(
                    (70.0, y + 10.0),
                    (104.0, y + 108.0),
                    (tile_w + 4.0, y + 45.0),
                )
                .build();
            c.stroke(wave, accent, 3.0);
            c.text("Кривые", (40.0, y + 88.0), TextStyle::new(12.0, MUTED));
            let tx = 24.0 + tile_w + 8.0;
            c.text(
                "Аа Ёё",
                (tx + 16.0, y + 50.0),
                TextStyle::new(28.0, INK).family(FontFamily::Serif),
            );
            c.text("Текст", (tx + 16.0, y + 88.0), TextStyle::new(12.0, MUTED));
            let ix = 24.0 + 2.0 * (tile_w + 8.0);
            c.with_save(|c| {
                c.clip(Shape::RoundedRect(
                    Rect::new(ix + 16.0, y + 14.0, 48.0, 48.0),
                    8.0,
                ));
                c.image(&self.image, Rect::new(ix + 16.0, y + 14.0, 48.0, 48.0), 1.0);
            });
            c.text(
                "Изображение",
                (ix + 16.0, y + 88.0),
                TextStyle::new(12.0, MUTED),
            );
        }
    }
}
rusterize::export_app!(Gallery);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn click_only_activates_on_release_inside() {
        let mut e = Engine::new(Gallery::default());
        let l = Layout::new(e.viewport());
        let p = (l.plus.x + 5.0, l.plus.y + 5.0).into();
        e.dispatch(Event::Pointer {
            id: 0,
            phase: PointerPhase::Down,
            position: p,
            buttons: 1,
        });
        assert_eq!(e.app.count, 0);
        e.dispatch(Event::Pointer {
            id: 1,
            phase: PointerPhase::Up,
            position: p,
            buttons: 0,
        });
        assert_eq!(e.app.count, 0);
        e.dispatch(Event::Pointer {
            id: 0,
            phase: PointerPhase::Up,
            position: p,
            buttons: 0,
        });
        assert_eq!(e.app.count, 1);
    }
    #[test]
    fn frames_valid_at_desktop_and_phone_sizes() {
        for (w, h, s) in [
            (960.0, 680.0, 1.0),
            (393.0, 852.0, 3.0),
            (320.0, 640.0, 2.0),
        ] {
            let mut e = Engine::new(Gallery::default());
            e.dispatch(Event::Resize(Viewport::new(w, h, s)));
            assert!(e.frame(0.0).is_ok());
        }
    }
}
