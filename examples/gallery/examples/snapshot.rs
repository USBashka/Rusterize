use rusterize::*;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/gallery.bgra".into());
    let mut engine = Engine::new(rusterize_gallery::Gallery::default());
    engine.dispatch(Event::Resize(Viewport::new(960.0, 680.0, 1.0)));
    let scene = engine.frame(0.0)?;
    #[cfg(windows)]
    std::fs::write(&output, windows::render_offscreen(scene, 960, 680, 1.0)?)?;
    let mut wire = Vec::new();
    protocol::encode(scene, &mut wire);
    std::fs::write(format!("{output}.rz01"), wire)?;
    println!("{output}: 960 × 680 premultiplied BGRA8");
    Ok(())
}
