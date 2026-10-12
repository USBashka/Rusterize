//! Browser integration fixture: compiled Rust checks plus real RZ02 pixels/input.
use rusterize::{native::*, *};
use std::cell::RefCell;

thread_local! { static METRICS: RefCell<[u32; 24]> = const { RefCell::new([0; 24]) }; }
fn record(index: usize, value: u32) {
    METRICS.with(|m| m.borrow_mut()[index] = value);
}
fn increment(index: usize) {
    METRICS.with(|m| m.borrow_mut()[index] += 1);
}
#[no_mangle]
pub extern "C" fn web_check_metric(index: u32) -> u32 {
    METRICS.with(|m| m.borrow().get(index as usize).copied().unwrap_or(0))
}

#[derive(Default)]
pub struct Checks {
    invalid: bool,
}
impl Application for Checks {
    fn event(&mut self, event: Event, ctx: &mut Context) {
        match event {
            Event::Resume => {
                increment(0);
                assert!(native::capabilities().supports(
                    NativeCapabilities::TEXT_LAYOUT
                        | NativeCapabilities::CURSOR
                        | NativeCapabilities::FILE_DIALOG
                        | NativeCapabilities::WEBVIEW
                ));
                let style = TextStyle::new(20.0, Color::BLACK);
                assert!(
                    measure_text("WWWW", &style).unwrap().width
                        > measure_text("iiii", &style).unwrap().width
                );
                for text in [
                    "",
                    "\n",
                    "\r\n",
                    "Ё\n\nالعربية\n",
                    "👩‍💻e\u{301}",
                    "日本語 café\r\nЁ",
                ] {
                    let layout =
                        TextLayout::new(text, style.clone(), TextOptions::wrap(1.0)).unwrap();
                    assert_eq!(
                        layout
                            .lines()
                            .iter()
                            .map(|l| &text[l.range.clone()])
                            .collect::<String>(),
                        text
                    );
                    if text == "👩‍💻e\u{301}" {
                        assert_eq!(layout.lines().len(), 2);
                        assert_eq!(&text[layout.lines()[0].range.clone()], "👩‍💻");
                    }
                    assert_eq!(layout.fitting_prefix(layout.metrics().height), text.len());
                }
                ctx.set_title("Rusterize Web checks — Ё");
            }
            Event::Suspend => increment(1),
            Event::Resize(v) => {
                record(2, v.size.width as u32);
                record(3, v.size.height as u32);
                record(4, (v.scale * 100.0) as u32);
            }
            Event::Pointer {
                id,
                phase,
                position,
                buttons,
            } => {
                increment(5 + phase as usize);
                record(9, id as u32);
                record(10, position.x.max(0.0) as u32);
                record(11, buttons);
            }
            Event::Key {
                key,
                pressed,
                repeat,
                modifiers,
            } => {
                increment(if pressed { 12 } else { 13 });
                record(14, u32::from(repeat));
                record(
                    15,
                    u32::from(modifiers.shift) | (u32::from(modifiers.control) << 1),
                );
                if pressed {
                    match key {
                        Key::Space => {
                            ctx.set_cursor(Cursor::Hand);
                            ctx.set_always_on_top(true);
                        }
                        Key::Enter => ctx.set_animation(true),
                        Key::Tab => ctx.set_animation(false),
                        Key::Escape => ctx.exit(),
                        Key::Delete => {
                            self.invalid = true;
                            ctx.request_redraw();
                        }
                        _ => {}
                    }
                }
            }
            Event::Text(text) => record(16, text.chars().last().unwrap() as u32),
            Event::Scroll { delta, .. } => {
                record(17, delta.y as u32);
            }
            Event::Focus(value) => {
                record(18, u32::from(value));
            }
            Event::NativeResult { result, .. } => increment(if result.is_ok() { 19 } else { 20 }),
            Event::Frame { delta, .. } => {
                increment(21);
                record(22, (delta * 1000.0) as u32);
            }
            #[allow(unreachable_patterns)] // The workspace can also enable Event::WebView.
            _ => {}
        }
    }
    fn draw(&mut self, c: &mut Canvas<'_>, _: Viewport) {
        increment(23);
        c.clear(Color::TRANSPARENT);
        if self.invalid {
            c.restore();
            return;
        }
        c.fill_rect(Rect::new(0.0, 0.0, 20.0, 20.0), Color::rgb(255, 0, 0));
        c.with_save(|c| {
            c.translate(30.0, 0.0);
            c.clip(Rect::new(0.0, 0.0, 10.0, 10.0));
            c.fill_rect(Rect::new(0.0, 0.0, 20.0, 20.0), Color::rgb(0, 255, 0));
        });
        c.rounded_rect(Rect::new(60.0, 0.0, 20.0, 20.0), 8.0, Color::rgb(0, 0, 255));
        c.ellipse(Rect::new(90.0, 0.0, 20.0, 20.0), Color::rgb(255, 255, 0));
        let path = Path::builder()
            .move_to((120.0, 0.0))
            .line_to((140.0, 0.0))
            .cubic_to((140.0, 20.0), (120.0, 20.0), (120.0, 20.0))
            .close()
            .build();
        c.fill(path, Color::rgb(0, 255, 255));
        c.stroke(Rect::new(152.0, 2.0, 16.0, 16.0), Color::BLACK, 4.0);
        c.fill_rect(
            Rect::new(0.0, 30.0, 20.0, 20.0),
            Paint::Linear {
                from: (0.0, 30.0).into(),
                to: (20.0, 30.0).into(),
                start: Color::BLACK,
                end: Color::WHITE,
            },
        );
        let image = Image::rgba(1, 1, vec![255, 0, 255, 255]).unwrap();
        c.image(&image, Rect::new(30.0, 30.0, 20.0, 20.0), 0.5);
        c.text(
            "Ёё",
            (0.0, 82.0),
            TextStyle::new(22.0, Color::BLACK)
                .bold()
                .italic()
                .underline()
                .strikethrough(),
        );
        let layout = TextLayout::new(
            "Ёж идёт\nпо лесу 👩‍💻",
            TextStyle::new(18.0, Color::BLACK),
            TextOptions::wrap(120.0),
        )
        .unwrap();
        c.text_layout(&layout, (0.0, 100.0));
    }
}
rusterize::export_app!(Checks);
