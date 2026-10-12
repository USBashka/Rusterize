use rusterize::{shader::*, *};
use std::{cell::RefCell, collections::HashMap, f32::consts::TAU};

const ORANGE: Color = Color::hex(0xe65e38);
const WHITE: Color = Color::hex(0xf6f5f0);
static SILK: ShaderSource = include!(concat!(env!("OUT_DIR"), "/silk.rs"));

#[derive(Clone, Copy)]
struct Settings {
    scroll: f32,
    time: f32,
    dark: bool,
    paused: bool,
    mode: u32,
    amount: f32,
    color: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            scroll: 0.0,
            time: 0.0,
            dark: false,
            paused: false,
            mode: 0,
            amount: 0.5,
            color: 0,
        }
    }
}
thread_local! { static SETTINGS: RefCell<Settings> = RefCell::new(Settings::default()); }

// The browser supplies viewport state; all drawing and layout stay in Rust.
#[no_mangle]
pub extern "C" fn app_environment(scroll: f32, time: f32, dark: u32, paused: u32) {
    if !scroll.is_finite() || !time.is_finite() {
        return;
    }
    SETTINGS.with(|value| {
        let mut s = value.borrow_mut();
        s.scroll = scroll.max(0.0);
        s.time = time.max(0.0);
        s.dark = dark != 0;
        s.paused = paused != 0;
    });
}

#[no_mangle]
pub extern "C" fn app_action(action: u32, value: f32) {
    if !value.is_finite() {
        return;
    }
    SETTINGS.with(|state| {
        let mut s = state.borrow_mut();
        match action {
            0 => s.mode = (value as u32).min(3),
            1 => s.amount = value.clamp(0.0, 1.0),
            2 => s.color = (value as u32).min(2),
            _ => {}
        }
    });
}

struct Layout {
    x: f32,
    w: f32,
    narrow: bool,
    hero: Rect,
    platforms: f32,
    demo: f32,
    panel: Rect,
    features: f32,
    start: f32,
    code: Rect,
    footer: f32,
}
impl Layout {
    fn new(width: f32) -> Self {
        let width = width.clamp(280.0, 10000.0);
        let narrow = width < 760.0;
        let w = (width - if narrow { 40.0 } else { 96.0 }).min(1184.0);
        let x = (width - w) * 0.5;
        let hero = if narrow {
            Rect::new(x, 442.0, w, 300.0)
        } else {
            Rect::new(x + w * 0.57, 134.0, w * 0.43, 410.0)
        };
        let platforms = if narrow { 784.0 } else { 636.0 };
        let demo = platforms + if narrow { 214.0 } else { 150.0 };
        let panel = Rect::new(x, demo + 98.0, w, if narrow { 500.0 } else { 420.0 });
        let features = panel.y + panel.height + 38.0;
        let start = features + if narrow { 388.0 } else { 234.0 };
        let code = if narrow {
            Rect::new(x, start + 144.0, w, 312.0)
        } else {
            Rect::new(x + w * 0.45, start - 10.0, w * 0.55, 326.0)
        };
        let footer = code.y + code.height + 98.0;
        Self {
            x,
            w,
            narrow,
            hero,
            platforms,
            demo,
            panel,
            features,
            start,
            code,
            footer,
        }
    }
    fn control(&self, id: u32) -> Rect {
        let Self { x, w, panel: p, .. } = *self;
        match id {
            0 if !self.narrow => Rect::new(x + w - 440.0, 24.0, 144.0, 44.0),
            1 if !self.narrow => Rect::new(x + w - 292.0, 24.0, 146.0, 44.0),
            2 => Rect::new(x + w - 132.0, 24.0, 84.0, 44.0),
            3 => Rect::new(x + w - 40.0, 24.0, 40.0, 44.0),
            4 => Rect::new(
                x,
                if self.narrow { 352.0 } else { 426.0 },
                if self.narrow { 154.0 } else { 184.0 },
                52.0,
            ),
            5 => Rect::new(
                x + if self.narrow { 162.0 } else { 196.0 },
                if self.narrow { 352.0 } else { 426.0 },
                if self.narrow { w - 162.0 } else { 160.0 },
                52.0,
            ),
            6 => Rect::new(
                self.hero.x + self.hero.width - 100.0,
                self.hero.y + self.hero.height - 40.0,
                100.0,
                40.0,
            ),
            7..=10 => {
                let tab_w = (w - 32.0) / 4.0;
                Rect::new(x + 16.0 + (id - 7) as f32 * tab_w, p.y + 14.0, tab_w, 44.0)
            }
            11 => Rect::new(
                x + 28.0,
                p.y + p.height - if self.narrow { 122.0 } else { 70.0 },
                if self.narrow { w - 56.0 } else { w * 0.42 },
                40.0,
            ),
            12..=14 => Rect::new(
                x + w - 164.0 + (id - 12) as f32 * 46.0,
                p.y + p.height - 64.0,
                40.0,
                40.0,
            ),
            15 => Rect::new(
                self.code.x + self.code.width - 126.0,
                self.code.y + 10.0,
                114.0,
                40.0,
            ),
            16 => Rect::new(x, self.footer + 28.0, 176.0, 44.0),
            17 => Rect::new(x + w - 138.0, self.footer + 28.0, 138.0, 44.0),
            18 => Rect::new(
                x + if self.narrow { 28.0 } else { w * 0.42 + 56.0 },
                p.y + p.height - 64.0,
                96.0,
                40.0,
            ),
            _ => Rect::default(),
        }
    }
}

#[no_mangle]
pub extern "C" fn app_layout(width: f32, element: u32, component: u32) -> f32 {
    if !width.is_finite() {
        return 0.0;
    }
    let l = Layout::new(width);
    if element == 100 {
        return match component {
            0 => l.footer + 128.0,
            1 => l.demo - 24.0,
            2 => l.start - 48.0,
            _ => 0.0,
        };
    }
    let r = l.control(element);
    match component {
        0 => r.x,
        1 => r.y,
        2 => r.width,
        3 => r.height,
        _ => 0.0,
    }
}

#[derive(Default)]
pub struct App {
    text: HashMap<String, TextLayout>,
    renderer: Option<ShaderRenderer>,
    shader_failed: bool,
    drag: Option<u64>,
    offset: Point,
    width: f32,
}
fn text(c: &mut Canvas<'_>, value: &str, x: f32, y: f32, size: f32, color: Color, bold: bool) {
    let style = TextStyle::new(size, color).font("Manrope");
    c.text(value, (x, y), if bold { style.bold() } else { style });
}
fn mono(c: &mut Canvas<'_>, value: &str, x: f32, y: f32, size: f32, color: Color) {
    c.text(
        value,
        (x, y),
        TextStyle::new(size, color).family(FontFamily::Monospace),
    );
}
fn centered(c: &mut Canvas<'_>, value: &str, r: Rect, size: f32, color: Color) {
    let style = TextStyle::new(size, color).font("Manrope").bold();
    let width = c
        .measure_text(value, &style)
        .map(|m| m.width)
        .unwrap_or(0.0);
    c.text(
        value,
        (
            r.x + (r.width - width) / 2.0,
            r.y + r.height / 2.0 + size * 0.35,
        ),
        style,
    );
}
fn outline(c: &mut Canvas<'_>, r: Rect, radius: f32, color: Color) {
    c.stroke(Shape::RoundedRect(r, radius), color, 1.0);
}
fn accent(s: Settings) -> Color {
    [ORANGE, Color::hex(0x90bda5), Color::hex(0xb4a4ef)][s.color as usize]
}
impl App {
    fn paragraph(&mut self, c: &mut Canvas<'_>, value: &str, r: Rect, size: f32, color: Color) {
        let key = format!("{value}:{size}:{}:{color:?}", r.width);
        if !self.text.contains_key(&key) {
            if self.text.len() > 100 {
                self.text.clear();
            }
            if let Ok(layout) = TextLayout::new(
                value,
                TextStyle::new(size, color).font("Manrope"),
                TextOptions::wrap(r.width),
            ) {
                self.text.insert(key.clone(), layout);
            }
        }
        if let Some(layout) = self.text.get(&key) {
            c.text_layout(layout, (r.x, r.y));
        }
    }

    fn sculpture(c: &mut Canvas<'_>, r: Rect, time: f32, color: Color, offset: Point) {
        let scale = r.width.min(r.height) * 0.34;
        let cx = r.x + r.width * 0.5 + offset.x;
        let cy = r.y + r.height * 0.47 + offset.y;
        for ring in 0..40 {
            let u = ring as f32 / 40.0 * TAU;
            let mut path = Path::builder();
            for step in 0..=64 {
                let v = step as f32 / 64.0 * TAU;
                let radius = 0.77 + 0.3 * v.cos();
                let x = radius * u.cos();
                let y = radius * u.sin();
                let z = 0.3 * v.sin();
                let a = time * 0.18 + 0.45;
                let px = x * a.cos() + z * a.sin();
                let pz = -x * a.sin() + z * a.cos();
                let py = y * 0.77 - pz * 0.64;
                let point = (
                    cx + (px * 0.92 - py * 0.38) * scale,
                    cy + (px * 0.38 + py * 0.92) * scale,
                );
                path = if step == 0 {
                    path.move_to(point)
                } else {
                    path.line_to(point)
                };
            }
            c.stroke(
                path.build(),
                color.with_alpha(140 + (u.sin() * 40.0 + 40.0) as u8),
                1.15,
            );
        }
    }

    fn draw_demo(&mut self, c: &mut Canvas<'_>, l: &Layout, s: Settings) {
        let p = l.panel;
        let a = accent(s);
        c.rounded_rect(p, 20.0, Color::hex(0x192522));
        for (i, title) in ["Графика", "Движение", "Текст", "WGSL"].iter().enumerate()
        {
            let r = l.control(7 + i as u32);
            if s.mode == i as u32 {
                c.rounded_rect(r, 10.0, Color::hex(0x35413d));
            }
            centered(
                c,
                title,
                r,
                if l.narrow { 12.0 } else { 14.0 },
                if s.mode == i as u32 {
                    WHITE
                } else {
                    Color::hex(0x9aada3)
                },
            );
        }
        let stage = Rect::new(
            p.x + 24.0,
            p.y + 80.0,
            p.width - 48.0,
            p.height - if l.narrow { 224.0 } else { 182.0 },
        );
        c.with_save(|c| {
            c.clip(Shape::RoundedRect(stage, 10.0));
            for ix in 0..(stage.width / 22.0) as u32 {
                for iy in 0..(stage.height / 22.0) as u32 {
                    c.circle((stage.x + ix as f32 * 22.0 + 11.0, stage.y + iy as f32 * 22.0 + 11.0), 0.7, Color::hex(0x3a4841));
                }
            }
            match s.mode {
                0 => {
                    let tile = stage.height.min(stage.width * 0.39).min(180.0);
                    let gap = if l.narrow { 20.0 } else { 60.0 };
                    let start = stage.x + (stage.width - tile * 2.0 - gap) / 2.0;
                    let y = stage.y + (stage.height - tile) / 2.0;
                    c.with_save(|c| {
                        c.translate(start + tile / 2.0, y + tile / 2.0);
                        c.transform(Transform::rotate(-0.2 + s.time.sin() * 0.1));
                        c.rounded_rect(Rect::new(-tile / 2.0, -tile / 2.0, tile, tile), s.amount * tile / 2.0,
                            Paint::Linear { from: (-tile / 2.0, -tile / 2.0).into(), to: (tile / 2.0, tile / 2.0).into(), start: a, end: Color::hex(0xf4d693) });
                        c.circle((0.0, 0.0), tile * 0.16, Color::hex(0x192522));
                    });
                    c.with_save(|c| {
                        c.translate(start + tile * 1.5 + gap, y + tile / 2.0);
                        c.transform(Transform::rotate(0.2));
                        c.clip(Shape::RoundedRect(Rect::new(-tile / 2.0, -tile / 2.0, tile, tile), 12.0 + s.amount * tile * 0.32));
                        c.fill_rect(Rect::new(-tile, -tile, tile * 2.0, tile * 2.0), a.with_alpha(40));
                        for i in -8..12 {
                            let x = i as f32 * 16.0 + (s.time * 8.0) % 16.0;
                            c.line((x, -tile), (x - tile, tile), a, 5.0);
                        }
                    });
                }
                1 => {
                    let center = Point::new(stage.x + stage.width / 2.0 + self.offset.x, stage.y + stage.height / 2.0 + self.offset.y);
                    for i in 0..5 {
                        let radius = 22.0 + i as f32 * 23.0;
                        c.stroke(Shape::Ellipse(Rect::new(center.x - radius, center.y - radius * 0.65, radius * 2.0, radius * 1.3)), a.with_alpha(80), 1.0);
                        let angle = s.time * (0.5 + s.amount * 1.7) + i as f32 * 1.8;
                        c.circle((center.x + angle.cos() * radius, center.y + angle.sin() * radius * 0.65), 6.0 + i as f32 * 1.5, a);
                    }
                    c.circle(center, 14.0, WHITE);
                }
                2 => {
                    let width = stage.width * (0.52 + s.amount * 0.48);
                    let box_r = Rect::new(stage.x + (stage.width - width) / 2.0, stage.y + 8.0, width, stage.height - 16.0);
                    c.rounded_rect(box_r, 12.0, Color::hex(0x26342e));
                    c.line((box_r.x + box_r.width - 1.0, box_r.y), (box_r.x + box_r.width - 1.0, box_r.y + box_r.height), a, 2.0);
                    self.paragraph(c, "Живой текст.\nВсё начинается с идеи: ёмкой, ясной, вашей. Rust рисует, система подбирает глифы и переносит строки.", box_r.inset(18.0), if l.narrow { 17.0 } else { 23.0 }, WHITE);
                }
                _ => {
                    if self.renderer.is_none() && !self.shader_failed {
                        match ShaderRenderer::new() { Ok(r) => self.renderer = Some(r), Err(_) => self.shader_failed = true }
                    }
                    if let Some(renderer) = &mut self.renderer {
                        let shader = SILK.shader(ShaderParams { time: s.time, data: [[s.amount, a.r as f32 / 255.0, a.g as f32 / 255.0, a.b as f32 / 255.0], [0.0; 4], [0.0; 4], [0.0; 4]] });
                        // A bounded offscreen texture keeps the demo light on high-DPI phones.
                        match renderer.render(&shader, [640, 220], 1.0) {
                            Ok(image) => c.image(&image, stage, 1.0),
                            Err(_) => self.shader_failed = true,
                        }
                    }
                    if self.shader_failed {
                        self.paragraph(c, "WebGL 2 недоступен в этом браузере. Остальные примеры работают на Canvas 2D.", stage.inset(20.0), 18.0, WHITE);
                    }
                }
            }
        });
        let desc = [
            "Скругление, градиент и отсечение",
            "Перетащите центр орбит",
            "Ширина строки и переносы Unicode",
            "Волны на GPU · WGSL → WebGL 2",
        ][s.mode as usize];
        text(
            c,
            desc,
            p.x + 28.0,
            p.y + p.height - if l.narrow { 146.0 } else { 87.0 },
            if l.narrow { 12.0 } else { 13.0 },
            Color::hex(0xb5c4bb),
            false,
        );
        let slider = l.control(11);
        centered(
            c,
            if s.paused {
                "▶ Движение"
            } else {
                "Ⅱ Пауза"
            },
            l.control(18),
            11.0,
            WHITE,
        );
        c.rounded_rect(
            Rect::new(slider.x, slider.y + 18.0, slider.width, 4.0),
            2.0,
            Color::hex(0x4b5c51),
        );
        c.rounded_rect(
            Rect::new(slider.x, slider.y + 18.0, slider.width * s.amount, 4.0),
            2.0,
            a,
        );
        c.circle(
            (slider.x + slider.width * s.amount, slider.y + 20.0),
            9.0,
            WHITE,
        );
        for i in 0..3 {
            let r = l.control(12 + i);
            c.circle(
                (r.x + 20.0, r.y + 20.0),
                12.0,
                [ORANGE, Color::hex(0x90bda5), Color::hex(0xb4a4ef)][i as usize],
            );
            if s.color == i {
                c.stroke(Shape::Ellipse(r.inset(3.0)), WHITE, 1.0);
            }
        }
    }
}

impl Application for App {
    fn window_options(&self) -> WindowOptions {
        WindowOptions::default().with_title("Rusterize — рисуйте на Rust")
    }
    fn event(&mut self, event: Event, c: &mut Context) {
        let s = SETTINGS.with(|s| *s.borrow());
        let l = Layout::new(self.width.max(320.0));
        match event {
            Event::Pointer {
                id,
                phase: PointerPhase::Down,
                position,
                ..
            } if s.mode == 1 => {
                let point = Point::new(position.x, position.y + s.scroll);
                let r = Rect::new(
                    l.panel.x + 24.0,
                    l.panel.y + 80.0,
                    l.panel.width - 48.0,
                    l.panel.height - if l.narrow { 224.0 } else { 182.0 },
                );
                if r.contains(point) {
                    self.drag = Some(id);
                }
            }
            Event::Pointer {
                id,
                phase: PointerPhase::Move,
                position,
                ..
            } if self.drag == Some(id) => {
                let stage_h = l.panel.height - if l.narrow { 224.0 } else { 182.0 };
                self.offset = Point::new(
                    (position.x - l.panel.x - l.panel.width / 2.0)
                        .clamp(-l.panel.width * 0.23, l.panel.width * 0.23),
                    (position.y + s.scroll - l.panel.y - 80.0 - stage_h / 2.0).clamp(-60.0, 60.0),
                );
                c.request_redraw();
            }
            Event::Pointer {
                id,
                phase: PointerPhase::Up | PointerPhase::Cancel,
                ..
            } if self.drag == Some(id) => self.drag = None,
            Event::Focus(false) | Event::Suspend => self.drag = None,
            _ => {}
        }
    }
    fn draw(&mut self, c: &mut Canvas<'_>, v: Viewport) {
        self.width = v.size.width;
        let s = SETTINGS.with(|s| *s.borrow());
        let l = Layout::new(v.size.width);
        let ink = Color::hex(if s.dark { 0xf0f0e8 } else { 0x252a24 });
        let muted = Color::hex(if s.dark { 0xb2b9ad } else { 0x6b7468 });
        let border = Color::hex(if s.dark { 0x42483e } else { 0xd9ddd1 });
        c.clear(Color::hex(if s.dark { 0x151b17 } else { 0xf8f8f2 }));
        c.translate(0.0, -s.scroll);
        let (x, w) = (l.x, l.w);
        // The viewport-sized surface avoids allocating a page-sized high-DPI bitmap.
        c.rounded_rect(Rect::new(x, 31.0, 30.0, 30.0), 8.0, ORANGE);
        text(c, "r", x + 9.0, 53.0, 24.0, WHITE, true);
        text(
            c,
            "rusterize",
            x + 40.0,
            53.0,
            if l.narrow { 19.0 } else { 23.0 },
            ink,
            true,
        );
        if !l.narrow {
            centered(c, "Возможности", l.control(0), 13.0, muted);
            centered(c, "Документация ↗", l.control(1), 13.0, muted);
        }
        centered(c, "GitHub ↗", l.control(2), 13.0, ink);
        centered(c, if s.dark { "☼" } else { "◐" }, l.control(3), 23.0, ink);
        c.line((x, 86.0), (x + w, 86.0), border, 1.0);
        mono(
            c,
            "RUST / NATIVE / WEB",
            x,
            if l.narrow { 134.0 } else { 155.0 },
            11.0,
            muted,
        );
        let fs = if l.narrow {
            (w / 8.8).min(48.0)
        } else {
            (w * 0.054).min(64.0)
        };
        let y = if l.narrow { 196.0 } else { 238.0 };
        text(c, "Ближе к системе.", x - 2.0, y, fs, ink, true);
        text(c, "Свободнее", x - 2.0, y + fs * 1.18, fs, ink, true);
        text(c, "в идеях.", x - 2.0, y + fs * 2.36, fs, ORANGE, true);
        if !l.narrow {
            self.paragraph(
                c,
                "Ваш интерфейс на Rust. Нативное рисование на каждой платформе.",
                Rect::new(x, 490.0, w * 0.49, 60.0),
                16.0,
                muted,
            );
        } else {
            self.paragraph(
                c,
                "Один Rust-код. Пять платформ.",
                Rect::new(x, 322.0, w, 30.0),
                14.0,
                muted,
            );
        }
        c.rounded_rect(l.control(4), 9.0, ORANGE);
        centered(
            c,
            "Попробовать ↓",
            l.control(4),
            if l.narrow { 13.0 } else { 14.0 },
            WHITE,
        );
        outline(c, l.control(5), 9.0, border);
        centered(
            c,
            "Начать писать",
            l.control(5),
            if l.narrow { 11.0 } else { 14.0 },
            ink,
        );
        if l.hero.y + l.hero.height > s.scroll && l.hero.y < s.scroll + v.size.height {
            for col in 0..(l.hero.width / 24.0) as u32 {
                for row in 0..(l.hero.height / 24.0) as u32 {
                    c.circle(
                        (l.hero.x + col as f32 * 24.0, l.hero.y + row as f32 * 24.0),
                        0.75,
                        border,
                    );
                }
            }
            Self::sculpture(c, l.hero, s.time, ORANGE, Point::default());
            mono(
                c,
                "Canvas / Rust",
                l.hero.x,
                l.hero.y + l.hero.height - 14.0,
                10.0,
                muted,
            );
            centered(
                c,
                if s.paused {
                    "▶ Движение"
                } else {
                    "Ⅱ Пауза"
                },
                l.control(6),
                11.0,
                ink,
            );
        }
        c.line(
            (x, l.platforms - 35.0),
            (x + w, l.platforms - 35.0),
            border,
            1.0,
        );
        let platforms = [
            ("Windows", "Direct2D"),
            ("Android", "Canvas"),
            ("Linux", "Cairo + Pango"),
            ("macOS", "Core Graphics"),
            ("Web", "WASM + Canvas"),
        ];
        for (i, (name, backend)) in platforms.iter().enumerate() {
            let cols = if l.narrow { 3 } else { 5 };
            let px = x + (i % cols) as f32 * w / cols as f32;
            let py = l.platforms + (i / cols) as f32 * 68.0;
            text(
                c,
                name,
                px,
                py,
                if l.narrow { 16.0 } else { 19.0 },
                ink,
                true,
            );
            mono(
                c,
                backend,
                px,
                py + 24.0,
                if l.narrow { 9.0 } else { 11.0 },
                muted,
            );
        }
        text(
            c,
            "Попробуйте в деле.",
            x,
            l.demo,
            if l.narrow { 15.0 } else { 29.0 },
            ink,
            true,
        );
        self.paragraph(
            c,
            "Меняйте параметры. Пробуйте в движении.",
            Rect::new(x, l.demo + 18.0, w, 40.0),
            if l.narrow { 13.0 } else { 15.0 },
            muted,
        );
        if l.panel.y + l.panel.height > s.scroll && l.panel.y < s.scroll + v.size.height {
            self.draw_demo(c, &l, s);
        }
        let features = [
            (
                "Одна логика",
                "Состояние, ввод и рисунок — в одном Application на Rust.",
            ),
            (
                "Системная графика",
                "Текст, пути, градиенты и изображения через Canvas своей платформы.",
            ),
            (
                "Нужное — по выбору",
                "WGSL-шейдеры и WebView подключаются отдельными features.",
            ),
        ];
        for (i, (title, body)) in features.iter().enumerate() {
            let fw = if l.narrow { w } else { (w - 64.0) / 3.0 };
            let fx = x + if l.narrow {
                0.0
            } else {
                i as f32 * (fw + 32.0)
            };
            let fy = l.features + if l.narrow { i as f32 * 112.0 } else { 0.0 };
            c.line((fx, fy), (fx + fw, fy), border, 1.0);
            c.circle((fx + 4.0, fy + 28.0), 3.0, ORANGE);
            text(c, title, fx + 18.0, fy + 34.0, 17.0, ink, true);
            self.paragraph(c, body, Rect::new(fx, fy + 50.0, fw, 64.0), 13.0, muted);
        }
        text(
            c,
            "От идеи",
            x,
            l.start + 25.0,
            if l.narrow { 32.0 } else { 40.0 },
            ink,
            true,
        );
        text(
            c,
            "к первому кадру.",
            x,
            l.start + 72.0,
            if l.narrow { 32.0 } else { 40.0 },
            ink,
            true,
        );
        self.paragraph(
            c,
            "Создайте Application, нарисуйте сцену — и соберите её для нужной платформы.",
            Rect::new(
                x,
                l.start + 96.0,
                if l.narrow { w } else { w * 0.36 },
                100.0,
            ),
            if l.narrow { 12.0 } else { 15.0 },
            muted,
        );
        c.rounded_rect(
            l.code,
            16.0,
            Color::hex(if s.dark { 0x222b24 } else { 0xeef0e6 }),
        );
        mono(c, "lib.rs", l.code.x + 20.0, l.code.y + 36.0, 12.0, muted);
        centered(c, "Копировать", l.control(15), 11.0, muted);
        c.line(
            (l.code.x + 18.0, l.code.y + 53.0),
            (l.code.x + l.code.width - 18.0, l.code.y + 53.0),
            border,
            1.0,
        );
        let lines = [
            "use rusterize::*;",
            "",
            "#[derive(Default)]",
            "pub struct App;",
            "",
            "impl Application for App {",
            "  fn draw(&mut self, c: &mut Canvas<'_>,",
            "          _: Viewport) {",
            "    c.clear(Color::hex(0xf8f8f2));",
            "    c.circle((120., 120.), 48.,",
            "             Color::hex(0xe65e38));",
            "  }",
            "}",
            "rusterize::export_app!(App);",
        ];
        let size = if l.narrow {
            (l.code.width - 32.0) / 23.5
        } else {
            12.0
        }
        .min(12.0);
        for (i, line) in lines.iter().enumerate() {
            mono(
                c,
                line,
                l.code.x + 18.0,
                l.code.y + 78.0 + i as f32 * 16.0,
                size,
                if i == 0 || i == 5 || i == 13 {
                    ORANGE
                } else {
                    ink
                },
            );
        }
        c.line((x, l.footer), (x + w, l.footer), border, 1.0);
        centered(c, "Исходный код ↗", l.control(16), 12.0, ink);
        centered(c, "Документация ↗", l.control(17), 12.0, ink);
        mono(
            c,
            "Rusterize 0.1 / MIT + Apache-2.0",
            x,
            l.footer + 96.0,
            10.0,
            muted,
        );
    }
}
rusterize::export_app!(App);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controls_fit_responsive_layout() {
        for width in [320.0, 360.0, 390.0, 759.0, 760.0, 1024.0, 1440.0, 2560.0] {
            let l = Layout::new(width);
            assert!(l.footer > l.code.y + l.code.height);
            for id in 0..19 {
                let r = l.control(id);
                assert!(r.x >= 0.0 && r.x + r.width <= width + 0.1, "{width}: {id}");
                assert!(r.width >= 0.0 && r.height >= 0.0);
            }
        }
    }
}
