#![cfg(all(feature = "media", not(target_arch = "wasm32")))]
use rusterize::{native::*, *};
use std::time::{Duration, Instant};

#[derive(Default)]
struct App {
    results: Vec<(RequestId, Result<NativeResponse, Error>)>,
}
impl Application for App {
    fn event(&mut self, event: Event, c: &mut Context) {
        match event {
            Event::Resume => {
                c.decode_image(b"corrupt".to_vec());
                c.decode_audio(include_bytes!("../../../examples/media/assets/demo.mp3").to_vec());
                c.decode_image(include_bytes!("../../../examples/media/assets/cover.png").to_vec());
            }
            Event::NativeResult { id, result } => self.results.push((id, result)),
            _ => {}
        }
    }
    fn draw(&mut self, c: &mut Canvas<'_>, _: Viewport) {
        c.clear(Color::BLACK);
    }
}
#[test]
fn worker_results_wake_idle_engine_and_preserve_request_order() {
    let mut engine = Engine::new(App::default());
    assert!(engine.pop_native_request().is_none());
    assert!(engine.is_animating());
    let start = Instant::now();
    while engine.app.results.len() < 3 && start.elapsed() < Duration::from_secs(10) {
        engine.frame(start.elapsed().as_secs_f64()).unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(engine.app.results.len(), 3);
    assert_eq!(
        engine
            .app
            .results
            .iter()
            .map(|r| r.0 .0)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert!(engine.app.results[0].1.is_err());
    assert!(matches!(
        engine.app.results[1].1,
        Ok(NativeResponse::Audio(_))
    ));
    assert!(matches!(
        engine.app.results[2].1,
        Ok(NativeResponse::Image(_))
    ));
    assert!(!engine.is_animating());
}
