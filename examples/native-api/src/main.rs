#[cfg(windows)]
use rusterize::*;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(windows)]
    {
        if let Some(path) = std::env::args().nth(1).filter(|s| s.ends_with(".bgra")) {
            let mut engine = Engine::new(rusterize_native_api::App::default());
            engine.dispatch(Event::Resize(Viewport::new(800.0, 600.0, 1.0)));
            std::fs::write(
                path,
                windows::render_offscreen(engine.frame(0.0)?, 800, 600, 1.0)?,
            )?;
            return Ok(());
        }
        run_app(rusterize_native_api::App::default())?;
    }
    Ok(())
}
