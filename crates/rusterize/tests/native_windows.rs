#![cfg(all(windows, feature = "native"))]
use rusterize::*;

fn pixel(image: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
    image[(y * w + x) * 4..(y * w + x + 1) * 4]
        .try_into()
        .unwrap()
}

#[test]
fn native_clip_transform_alpha_and_restore() {
    let mut scene = Scene::default();
    let mut c = Canvas::new(&mut scene);
    c.clear(Color::WHITE);
    c.with_save(|c| {
        c.translate(10.0, 10.0);
        c.clip(Rect::new(0.0, 0.0, 20.0, 20.0));
        c.fill_rect(
            Rect::new(-10.0, -10.0, 50.0, 50.0),
            Color::rgba(255, 0, 0, 128),
        );
    });
    c.fill_rect(Rect::new(40.0, 40.0, 10.0, 10.0), Color::rgb(0, 0, 255));
    c.finish().unwrap();
    let pixels = windows::render_offscreen(&scene, 64, 64, 1.0).unwrap();
    assert_eq!(pixel(&pixels, 64, 5, 5), [255, 255, 255, 255]);
    let mixed = pixel(&pixels, 64, 20, 20);
    assert!((mixed[0] as i32 - 127).abs() <= 1);
    assert_eq!(mixed[2..], [255, 255]);
    assert_eq!(pixel(&pixels, 64, 35, 20), [255, 255, 255, 255]);
    assert_eq!(pixel(&pixels, 64, 45, 45), [255, 0, 0, 255]);
}
#[test]
fn native_image_rgba_orientation_and_dpi() {
    let image = Image::rgba(
        2,
        2,
        vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
        ],
    )
    .unwrap();
    let mut s = Scene::default();
    let mut c = Canvas::new(&mut s);
    c.clear(Color::BLACK);
    c.image(&image, Rect::new(2.0, 2.0, 8.0, 8.0), 1.0);
    c.finish().unwrap();
    let p = windows::render_offscreen(&s, 24, 24, 2.0).unwrap();
    assert_eq!(pixel(&p, 24, 4, 4), [0, 0, 255, 255]);
    assert_eq!(pixel(&p, 24, 19, 4), [0, 255, 0, 255]);
    assert_eq!(pixel(&p, 24, 4, 19), [255, 0, 0, 255]);
    assert_eq!(pixel(&p, 24, 19, 19), [0, 255, 255, 255]);
    assert_eq!(pixel(&p, 24, 21, 21), [0, 0, 0, 255]);
}
#[test]
fn native_text_paths_and_gradient_produce_pixels() {
    let mut s = Scene::default();
    let mut c = Canvas::new(&mut s);
    c.clear(Color::WHITE);
    c.fill_rect(
        Rect::new(0.0, 0.0, 100.0, 20.0),
        Paint::Linear {
            from: (0.0, 0.0).into(),
            to: (100.0, 0.0).into(),
            start: Color::BLACK,
            end: Color::WHITE,
        },
    );
    c.text("Ёж", (10.0, 50.0), TextStyle::new(24.0, Color::BLACK));
    c.fill(
        Path::builder()
            .move_to((10.0, 70.0))
            .line_to((40.0, 70.0))
            .line_to((25.0, 90.0))
            .close()
            .build(),
        Color::BLACK,
    );
    c.finish().unwrap();
    let p = windows::render_offscreen(&s, 100, 100, 1.0).unwrap();
    assert!(pixel(&p, 100, 5, 10)[0] < pixel(&p, 100, 95, 10)[0]);
    assert!(p[25 * 100 * 4..52 * 100 * 4]
        .chunks_exact(4)
        .any(|p| p[0] < 100));
    assert_eq!(pixel(&p, 100, 25, 75), [0, 0, 0, 255]);
}

#[test]
fn native_titlebar_colors_and_reset_without_showing_a_window() {
    use ::windows::{
        core::w,
        Win32::{Foundation::HWND, UI::WindowsAndMessaging::*},
    };
    unsafe {
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!("Rusterize hidden test"),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            80,
            80,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let style = TitleBarStyle::colors(Color::rgb(12, 34, 56), Color::rgb(210, 220, 230))
            .with_border(Color::rgb(20, 40, 60));
        let support = rusterize::windows::set_title_bar(hwnd, style);
        let reset = rusterize::windows::set_title_bar(hwnd, TitleBarStyle::default());
        assert_eq!(support, reset);
        assert_eq!(
            rusterize::windows::set_title_bar(HWND::default(), style),
            rusterize::windows::TitleBarSupport::default()
        );
        DestroyWindow(hwnd).unwrap();
    }
}

#[test]
fn native_text_metrics_wrapping_unicode_and_pagination() {
    let style = TextStyle::new(22.0, Color::BLACK).font("Segoe UI");
    let narrow = measure_text("iiiiii", &style).unwrap();
    let wide = measure_text("WWWWWW", &style).unwrap();
    assert!(wide.width > narrow.width * 2.0);
    assert!(wide.baseline > 0.0 && wide.height >= wide.baseline);
    assert!(
        measure_text("text   ", &style)
            .unwrap()
            .width_including_trailing_whitespace
            > measure_text("text", &style).unwrap().width
    );
    for text in [
        "",
        "\n",
        "Ёж 👩‍💻 и е\u{301}\nالعربية\n",
        "Нативная раскладка учитывает ширину глифов и переносит слова.",
    ] {
        let layout = TextLayout::new(text, style.clone(), TextOptions::wrap(110.0)).unwrap();
        assert!(!layout.lines().is_empty());
        assert_eq!(
            layout
                .lines()
                .iter()
                .map(|l| &text[l.range.clone()])
                .collect::<String>(),
            text
        );
        let first = &layout.lines()[0];
        assert_eq!(
            layout.fitting_prefix(first.top + first.height),
            first.range.end
        );
        assert_eq!(layout.fitting_prefix(-1.0), 0);
        assert_eq!(
            layout.fitting_prefix(layout.metrics().height + 0.01),
            text.len()
        );
    }
    let layout =
        TextLayout::new("слово слово слово", style.clone(), TextOptions::wrap(90.0)).unwrap();
    assert!(layout.lines().len() >= 3);
    assert!(layout.metrics().width <= 90.0);
    assert!(measure_text("x", &TextStyle::new(f32::NAN, Color::BLACK)).is_err());
    assert!(TextLayout::new("x", style, TextOptions::wrap(0.0)).is_err());
}

#[test]
fn layout_measurement_and_native_draw_share_the_same_pixels() {
    let style = TextStyle::new(22.0, Color::BLACK).italic().underline();
    let layout = TextLayout::new("Ёж и AV", style.clone(), TextOptions::default()).unwrap();
    let mut baseline = Scene::default();
    let mut c = Canvas::new(&mut baseline);
    c.clear(Color::WHITE);
    c.text(
        layout.text(),
        (12.0, 12.0 + layout.metrics().baseline),
        style,
    );
    c.finish().unwrap();
    let mut block = Scene::default();
    let mut c = Canvas::new(&mut block);
    c.clear(Color::WHITE);
    c.text_layout(&layout, (12.0, 12.0));
    c.finish().unwrap();
    for scale in [1.0, 1.5, 2.0] {
        let a = windows::render_offscreen(&baseline, 400, 150, scale).unwrap();
        let b = windows::render_offscreen(&block, 400, 150, scale).unwrap();
        assert_eq!(a, b);
        assert!(a.chunks_exact(4).any(|p| p[0] < 128));
    }
}

#[test]
fn full_native_drawing_and_directwrite_hit_testing_are_accessible() {
    use ::windows::Win32::Graphics::{Direct2D::Common::*, DirectWrite::*};
    let scene = Scene::default();
    let pixels = windows::render_offscreen_with(&scene, 32, 32, 1.0, |native| {
        unsafe {
            let brush = native.target.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
                None,
            )?;
            native.target.FillRectangle(
                &D2D_RECT_F {
                    left: 4.0,
                    top: 4.0,
                    right: 20.0,
                    bottom: 20.0,
                },
                &brush,
            );
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(pixel(&pixels, 32, 10, 10), [0, 0, 255, 255]);
    let layout = windows::create_text_layout(
        "AV Ёж",
        &TextStyle::new(20.0, Color::BLACK),
        TextOptions::default(),
    )
    .unwrap();
    let (mut x, mut y, mut hit) = (0.0, 0.0, DWRITE_HIT_TEST_METRICS::default());
    unsafe {
        layout
            .HitTestTextPosition(3, false, &mut x, &mut y, &mut hit)
            .unwrap();
    }
    assert_eq!(hit.textPosition, 3);
    assert!(x > 0.0 && hit.height > 0.0);
}
