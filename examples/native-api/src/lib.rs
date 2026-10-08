use rusterize::{native::*, *};

const SAMPLE:&str="Ёж идёт по лесу. Над тропинкой дрожат жёлтые листья, а сквозь ветви пробивается вечерний свет.\n\nВ дорожной тетради соседствуют записи на разных языках: café, 日本語, العربية. На последней странице — маленький рисунок: 👩‍💻.";
#[derive(Default)]
pub struct App {
    layout: Option<TextLayout>,
    status: String,
    fullscreen: bool,
    capabilities: NativeCapabilities,
}
impl App {
    fn button_supported(&self, index: usize) -> bool {
        self.capabilities.supports(match index {
            0 => NativeCapabilities::CLIPBOARD,
            1 | 2 => NativeCapabilities::FILE_DIALOG,
            _ => NativeCapabilities::WINDOW_STATE,
        })
    }
    fn button(index: usize, width: f32) -> Rect {
        let columns = if width < 600.0 { 2 } else { 4 };
        let w = (width - 48.0 - 12.0 * (columns - 1) as f32) / columns as f32;
        Rect::new(
            24.0 + (index % columns) as f32 * (w + 12.0),
            80.0 + (index / columns) as f32 * 54.0,
            w,
            42.0,
        )
    }
    fn verify_text_backend() -> Result<(), Error> {
        let style = TextStyle::new(24.0, Color::WHITE);
        let thin = measure_text("iiiiii", &style)?;
        let wide = measure_text("WWWWWW", &style)?;
        if thin.width >= wide.width {
            return Err(Error("native font metrics failed".into()));
        }
        for s in ["", "\n", "Ё 👩‍💻\n\nالعربية\n"] {
            let layout = TextLayout::new(s, style.clone(), TextOptions::wrap(120.0))?;
            if layout
                .lines()
                .iter()
                .map(|l| &s[l.range.clone()])
                .collect::<String>()
                != s
            {
                return Err(Error("native Unicode line ranges failed".into()));
            }
        }
        Ok(())
    }
}
impl Application for App {
    fn window_options(&self) -> WindowOptions {
        WindowOptions::default()
            .with_title("Нативный API")
            .with_size(800.0, 600.0)
            .with_title_bar(TitleBarStyle::dark())
    }
    fn event(&mut self, event: Event, context: &mut Context) {
        match event {
            Event::Resume => {
                self.capabilities = native::capabilities();
                if self.capabilities.supports(NativeCapabilities::CURSOR) {
                    context.set_cursor(Cursor::Arrow);
                }
            }
            Event::NativeResult { result, .. } => {
                self.status = match result {
                    Ok(NativeResponse::Text(s)) => s,
                    Ok(NativeResponse::Paths(p)) => p
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                    Ok(NativeResponse::Cancelled) => "Выбор отменён".into(),
                    Ok(NativeResponse::Done) => String::new(),
                    Err(e) => e.to_string(),
                };
                context.request_redraw();
            }
            Event::Pointer {
                phase: PointerPhase::Up,
                position,
                ..
            } => {
                let width = self
                    .layout
                    .as_ref()
                    .map_or(800.0, |l| l.options().width + 48.0);
                if let Some(index) = (0..4).find(|i| Self::button(*i, width).contains(position)) {
                    if !self.button_supported(index) {
                        return;
                    }
                    match index {
                        0 => {
                            context.read_clipboard();
                        }
                        1 => {
                            context.file_dialog(FileDialog {
                                title: "Открыть файл".into(),
                                ..Default::default()
                            });
                        }
                        2 => {
                            context.file_dialog(FileDialog {
                                title: "Сохранить файл".into(),
                                suggested_name: "Документ.txt".into(),
                                save: true,
                                ..Default::default()
                            });
                        }
                        _ => {
                            self.fullscreen = !self.fullscreen;
                            context.set_window_state(if self.fullscreen {
                                WindowState::Fullscreen
                            } else {
                                WindowState::Normal
                            });
                        }
                    }
                }
            }
            Event::Key {
                key: Key::Escape,
                pressed: true,
                ..
            } => {
                self.fullscreen = false;
                context.set_window_state(WindowState::Normal);
            }
            _ => {}
        }
    }
    fn draw(&mut self, c: &mut Canvas<'_>, v: Viewport) {
        c.clear(Color::hex(0x101923));
        c.text(
            "Нативные возможности",
            (24.0, 48.0),
            TextStyle::new(26.0, Color::WHITE).bold(),
        );
        for (i, label) in ["Буфер обмена", "Открыть", "Сохранить", "Полный экран"]
            .iter()
            .enumerate()
        {
            let rect = Self::button(i, v.size.width);
            let enabled = self.button_supported(i);
            c.rounded_rect(
                rect,
                8.0,
                Color::hex(if enabled { 0x29435c } else { 0x192733 }),
            );
            c.text(
                *label,
                (rect.x + 12.0, rect.y + 27.0),
                TextStyle::new(
                    16.0,
                    if enabled {
                        Color::WHITE
                    } else {
                        Color::hex(0x6c7a87)
                    },
                ),
            );
        }
        let width = (v.size.width - 48.0).max(1.0);
        if self
            .layout
            .as_ref()
            .is_none_or(|l| l.options().width != width)
        {
            Self::verify_text_backend().expect("native text self-check");
            self.layout = Some(
                TextLayout::new(
                    SAMPLE,
                    TextStyle::new(24.0, Color::hex(0xe8edf5)).family(FontFamily::Serif),
                    TextOptions::wrap(width),
                )
                .expect("native layout"),
            );
        }
        let layout = self.layout.as_ref().unwrap();
        let top = if v.size.width < 600.0 { 212.0 } else { 158.0 };
        c.text_layout(layout, (24.0, top));
        if !self.status.is_empty() {
            if let Ok(status) = TextLayout::new(
                self.status.as_str(),
                TextStyle::new(16.0, Color::hex(0x9ddac8)),
                TextOptions::wrap(width),
            ) {
                c.text_layout(&status, (24.0, top + layout.metrics().height + 30.0));
            }
        }
    }
}
rusterize::export_app!(App);
