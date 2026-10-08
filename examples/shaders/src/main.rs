fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|s| s == "--bench") {
        use rusterize::shader::ShaderParams;
        let mut renderer = rusterize_shaders::checked_renderer()?;
        println!("{}", renderer.renderer_name());
        for size in [[256, 128], [512, 256], [1024, 512]] {
            for i in 0..10 {
                renderer.render(
                    &rusterize_shaders::WAVE.shader(ShaderParams {
                        time: i as f32,
                        ..Default::default()
                    }),
                    size,
                    1.0,
                )?;
            }
            let start = std::time::Instant::now();
            for i in 0..100 {
                renderer.render(
                    &rusterize_shaders::WAVE.shader(ShaderParams {
                        time: i as f32 * 0.03,
                        ..Default::default()
                    }),
                    size,
                    1.0,
                )?;
            }
            println!(
                "{}x{}: {:.3} ms/render including readback (100 frames)",
                size[0],
                size[1],
                start.elapsed().as_secs_f64() * 10.0
            );
        }
        return Ok(());
    }
    #[cfg(windows)]
    {
        use rusterize::*;
        if let Some(path) = std::env::args().nth(1).filter(|s| s.ends_with(".bgra")) {
            let mut engine = Engine::new(rusterize_shaders::App::default());
            engine.dispatch(Event::Resize(Viewport::new(760.0, 540.0, 1.0)));
            std::fs::write(
                path,
                windows::render_offscreen(engine.frame(0.0)?, 760, 540, 1.0)?,
            )?;
            return Ok(());
        }
        run_app(rusterize_shaders::App::default())?;
    }
    Ok(())
}
