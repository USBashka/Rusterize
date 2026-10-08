#![cfg(all(windows, feature = "native"))]
use rusterize::{native::*, *};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

#[test]
fn window_services_native_draw_and_native_messages_complete_in_the_event_loop() {
    use ::windows::Win32::{Foundation::*, UI::WindowsAndMessaging::*};
    struct App {
        results: Arc<AtomicUsize>,
        drawn: Arc<AtomicBool>,
        message: Arc<AtomicBool>,
        resized: bool,
    }
    impl Application for App {
        fn event(&mut self, event: Event, c: &mut Context) {
            match event {
                Event::Resume => {
                    c.set_animation(true);
                    c.set_cursor(Cursor::Crosshair);
                    c.set_window_size(Size::new(360.0, 340.0));
                }
                Event::Resize(v) => {
                    self.resized =
                        (v.size.width - 360.0).abs() < 1.0 && (v.size.height - 340.0).abs() < 1.0
                }
                Event::NativeResult { result, .. } => {
                    assert_eq!(result.unwrap(), NativeResponse::Done);
                    self.results.fetch_add(1, Ordering::SeqCst);
                }
                Event::Frame { elapsed, .. } => {
                    assert!(elapsed < 3.0, "native event loop did not complete");
                    if self.resized
                        && self.results.load(Ordering::SeqCst) >= 2
                        && self.drawn.load(Ordering::SeqCst)
                        && self.message.load(Ordering::SeqCst)
                    {
                        c.exit();
                    }
                }
                _ => {}
            }
        }
        fn draw(&mut self, c: &mut Canvas<'_>, _: Viewport) {
            c.clear(Color::WHITE);
        }
        fn native_draw(&mut self, n: &windows::NativeCanvas<'_>, _: Viewport) -> Result<(), Error> {
            use ::windows::Win32::Graphics::Direct2D::Common::*;
            unsafe {
                let b = n.target.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 0.1,
                        g: 0.5,
                        b: 0.8,
                        a: 1.0,
                    },
                    None,
                )?;
                n.target.FillRectangle(
                    &D2D_RECT_F {
                        left: 10.0,
                        top: 10.0,
                        right: 30.0,
                        bottom: 30.0,
                    },
                    &b,
                );
            }
            if !self.drawn.swap(true, Ordering::SeqCst) {
                unsafe {
                    PostMessageW(windows::current_window(), WM_APP + 99, WPARAM(0), LPARAM(0))?;
                }
            }
            Ok(())
        }
        fn native_event(&mut self, m: windows::WindowMessage) -> Option<LRESULT> {
            if m.message == WM_APP + 99 {
                self.message.store(true, Ordering::SeqCst);
                Some(LRESULT(0))
            } else {
                None
            }
        }
    }
    let results = Arc::new(AtomicUsize::new(0));
    let drawn = Arc::new(AtomicBool::new(false));
    let message = Arc::new(AtomicBool::new(false));
    run(
        App {
            results: results.clone(),
            drawn: drawn.clone(),
            message: message.clone(),
            resized: false,
        },
        WindowOptions::default()
            .with_title("Rusterize native test")
            .with_size(320.0, 200.0),
    )
    .unwrap();
    assert!(
        results.load(Ordering::SeqCst) >= 2
            && drawn.load(Ordering::SeqCst)
            && message.load(Ordering::SeqCst)
    );
}
