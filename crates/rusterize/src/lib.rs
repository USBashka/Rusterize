//! Native-canvas applications with one drawing and event model.
//!
//! Coordinates are logical pixels, Y points down, colours are straight-alpha sRGB.
//! Native text uses a baseline, not the top of a bounding box.
//!
//! ```
//! use rusterize::{Canvas, Color, Rect, Scene};
//! let mut scene = Scene::default();
//! let mut canvas = Canvas::new(&mut scene);
//! canvas.clear(Color::WHITE);
//! canvas.fill_rect(Rect::new(8.0, 8.0, 80.0, 40.0), Color::rgb(30, 120, 240));
//! canvas.finish().unwrap();
//! assert_eq!(scene.commands().len(), 2);
//! ```

mod app;
mod canvas;
mod geometry;
pub mod host;
mod paint;
pub mod protocol;

pub use app::*;
pub use canvas::*;
pub use geometry::*;
pub use paint::*;

#[cfg(target_os = "android")]
#[doc(hidden)]
pub mod android;
#[cfg(all(windows, feature = "native"))]
pub mod windows;
#[cfg(target_os = "android")]
#[doc(hidden)]
pub use jni;

/// Run on Windows. Other hosts call the same application through [`export_app!`].
#[cfg(all(windows, feature = "native"))]
pub fn run(app: impl Application + 'static, options: WindowOptions) -> Result<(), Error> {
    windows::run(app, options)
}

/// Run with settings from [`Application::window_options`].
#[cfg(all(windows, feature = "native"))]
pub fn run_app(app: impl Application + 'static) -> Result<(), Error> {
    let options = app.window_options();
    run(app, options)
}

/// Framework errors never require parsing native error codes in application code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
