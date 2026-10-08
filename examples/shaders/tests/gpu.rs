use rusterize::{shader::*, *};
use rusterize_shaders::{TEXTURE, UV, WAVE};

fn pixel(image: &Image, x: usize, y: usize) -> &[u8] {
    let offset = (y * image.width() as usize + x) * 4;
    &image.pixels()[offset..offset + 4]
}
fn near(value: u8, expected: u8) {
    assert!(
        (value as i32 - expected as i32).abs() <= 2,
        "{value} != {expected}"
    );
}

#[test]
#[ignore = "Requires a native graphics driver; run --test gpu -- --ignored"]
fn demo_pause_resize_and_resume_preserve_state() {
    fn image(scene: &Scene) -> Image {
        scene
            .commands()
            .iter()
            .find_map(|command| match command {
                Command::Image { image, .. } => Some(image.clone()),
                _ => None,
            })
            .expect("demo must draw its shader, not an error message")
    }
    let mut engine = Engine::new(rusterize_shaders::App::default());
    engine.dispatch(Event::Resize(Viewport::new(320.0, 240.0, 1.0)));
    let first = image(engine.frame(0.0).unwrap());
    let animated = image(engine.frame(0.05).unwrap());
    assert_ne!(first.pixels(), animated.pixels());
    engine.dispatch(Event::Key {
        key: Key::Space,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    assert!(!engine.is_animating());
    let paused = image(engine.frame(0.1).unwrap());
    assert_eq!(paused.id(), image(engine.frame(0.2).unwrap()).id());
    assert!(!engine.needs_redraw());
    engine.dispatch(Event::Suspend);
    engine.dispatch(Event::Resume);
    assert!(!engine.is_animating());
    assert_eq!(paused.pixels(), image(engine.frame(0.3).unwrap()).pixels());
    engine.dispatch(Event::Pointer {
        id: 0,
        phase: PointerPhase::Up,
        position: Point::new(40.0, 100.0),
        buttons: 0,
    });
    assert!(engine.is_animating());
    engine.dispatch(Event::Resize(Viewport::new(400.0, 300.0, 1.5)));
    let resized = image(engine.frame(0.4).unwrap());
    assert_eq!((resized.width(), resized.height()), (528, 252));
}

#[cfg(windows)]
#[test]
#[ignore = "Requires a native graphics driver; run --test gpu -- --ignored"]
fn shader_demo_runs_and_resizes_in_a_real_win32_window() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    struct WindowApp {
        app: rusterize_shaders::App,
        frames: Arc<AtomicUsize>,
        resized: bool,
    }
    impl Application for WindowApp {
        fn event(&mut self, event: Event, c: &mut Context) {
            match &event {
                Event::Resume => {
                    c.set_window_size(Size::new(400.0, 360.0));
                }
                Event::Resize(v) => {
                    self.resized =
                        (v.size.width - 400.0).abs() < 1.0 && (v.size.height - 360.0).abs() < 1.0
                }
                Event::Frame { elapsed, .. } => {
                    assert!(*elapsed < 5.0, "shader window did not complete");
                    if *elapsed > 0.5 && self.resized && self.frames.load(Ordering::SeqCst) >= 2 {
                        c.exit();
                    }
                }
                _ => {}
            }
            self.app.event(event, c);
        }
        fn draw(&mut self, c: &mut Canvas<'_>, viewport: Viewport) {
            self.app.draw(c, viewport);
        }
        fn native_draw(&mut self, _: &windows::NativeCanvas<'_>, _: Viewport) -> Result<(), Error> {
            self.frames.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }
    let frames = Arc::new(AtomicUsize::new(0));
    run(
        WindowApp {
            app: Default::default(),
            frames: frames.clone(),
            resized: false,
        },
        WindowOptions::default()
            .with_title("Rusterize WGSL test")
            .with_size(360.0, 340.0),
    )
    .unwrap();
    assert!(frames.load(Ordering::SeqCst) >= 2);
}

#[test]
#[ignore = "Requires a native graphics driver; run --test gpu -- --ignored"]
fn real_driver_renders_coordinates_uniforms_alpha_texture_and_animation() {
    let mut renderer = ShaderRenderer::new().expect("Shader GPU context");
    eprintln!("Shader driver: {}", renderer.renderer_name());
    let params = ShaderParams {
        data: [[0.25, 0.5, 0.0, 0.0], [0.0; 4], [0.0; 4], [0.0; 4]],
        ..Default::default()
    };
    let a = renderer.render(&UV.shader(params), [4, 4], 1.0).unwrap();
    for (x, y) in [(0, 0), (3, 0), (0, 3), (3, 3)] {
        let p = pixel(&a, x, y);
        near(p[0], ((x as f32 + 0.5) / 4.0 * 255.0) as u8);
        near(p[1], ((y as f32 + 0.5) / 4.0 * 255.0) as u8);
        near(p[2], 64);
        near(p[3], 128);
    }
    assert_eq!(
        a.id(),
        renderer
            .render(&UV.shader(params), [4, 4], 1.0)
            .unwrap()
            .id()
    );
    let source = Image::rgba(
        2,
        2,
        vec![
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 64, 255, 255, 255, 0,
        ],
    )
    .unwrap();
    let parameters = ShaderParams {
        data: [[1.0; 4], [0.0; 4], [0.0; 4], [0.0; 4]],
        ..Default::default()
    };
    let sampled = renderer
        .render(&TEXTURE.shader(parameters).with_image(&source), [2, 2], 1.0)
        .unwrap();
    assert_eq!(sampled.pixels(), source.pixels());
    let white = renderer
        .render(&TEXTURE.shader(parameters), [2, 2], 1.0)
        .unwrap();
    assert!(white.pixels().iter().all(|v| *v == 255));
    let still = renderer
        .render(&WAVE.shader(ShaderParams::default()), [32, 24], 1.0)
        .unwrap();
    let animated = renderer
        .render(
            &WAVE.shader(ShaderParams {
                time: 1.0,
                ..Default::default()
            }),
            [32, 24],
            1.0,
        )
        .unwrap();
    assert_ne!(still.pixels(), animated.pixels());
    assert_ne!(still.id(), animated.id());
    for _ in 0..3 {
        let mut second = ShaderRenderer::new().unwrap();
        assert_eq!(
            a.pixels(),
            second
                .render(&UV.shader(params), [4, 4], 1.0)
                .unwrap()
                .pixels()
        );
    }
    assert_eq!(
        a.pixels(),
        renderer
            .render(&UV.shader(params), [4, 4], 1.0)
            .unwrap()
            .pixels()
    );
}

#[test]
#[ignore = "Requires a native graphics driver; run --test gpu -- --ignored"]
fn invalid_requests_fail_and_canvas_retains_its_atomic_frame_contract() {
    let mut renderer = ShaderRenderer::new().unwrap();
    for size in [[0, 4], [u32::MAX, 1], [4096, 4096]] {
        assert!(renderer
            .render(&WAVE.shader(ShaderParams::default()), size, 1.0)
            .is_err());
    }
    for scale in [0.0, -1.0, f32::INFINITY, f32::NAN] {
        assert!(renderer
            .render(&WAVE.shader(ShaderParams::default()), [2, 2], scale)
            .is_err());
    }
    let invalid = WAVE.shader(ShaderParams {
        time: f32::NAN,
        ..Default::default()
    });
    let mut scene = Scene::default();
    let mut c = Canvas::new(&mut scene);
    c.clear(Color::WHITE);
    c.shader(
        &mut renderer,
        Rect::new(0.0, 0.0, 20.0, 10.0),
        1.5,
        &invalid,
    );
    assert!(c.finish().is_err());
    assert!(scene.commands().is_empty());
    let mut c = Canvas::new(&mut scene);
    c.shader(
        &mut renderer,
        Rect::new(0.0, 0.0, 20.0, 10.0),
        1.5,
        &WAVE.shader(ShaderParams::default()),
    );
    c.finish().unwrap();
    let Command::Image { image, .. } = &scene.commands()[0] else {
        panic!("shader must use the native Image contract")
    };
    assert_eq!((image.width(), image.height()), (30, 15));
}

#[cfg(windows)]
#[test]
#[ignore = "Requires a native graphics driver; run --test gpu -- --ignored"]
fn native_compositor_preserves_clip_transform_order_and_alpha() {
    let mut renderer = ShaderRenderer::new().unwrap();
    let mut scene = Scene::default();
    let params = ShaderParams {
        data: [[0.25, 0.5, 0.0, 0.0], [0.0; 4], [0.0; 4], [0.0; 4]],
        ..Default::default()
    };
    let mut c = Canvas::new(&mut scene);
    c.clear(Color::WHITE);
    c.with_save(|c| {
        c.translate(4.0, 4.0);
        c.clip(Rect::new(0.0, 0.0, 4.0, 4.0));
        c.shader(
            &mut renderer,
            Rect::new(0.0, 0.0, 8.0, 8.0),
            1.0,
            &UV.shader(params),
        );
    });
    c.fill_rect(Rect::new(5.0, 5.0, 1.0, 1.0), Color::BLACK);
    c.finish().unwrap();
    for scale in [1.0, 1.5, 2.0] {
        let width = (16.0 * scale) as usize;
        let bgra = windows::render_offscreen(&scene, width as u32, width as u32, scale).unwrap();
        let outside = (10 * width + 10) * 4;
        if scale == 1.0 {
            assert_eq!(&bgra[outside..outside + 4], &[255; 4]);
        }
        let pos = ((6.0 * scale) as usize * width + (6.0 * scale) as usize) * 4;
        near(bgra[pos], 159);
        assert_eq!(bgra[pos + 3], 255);
    }
}
