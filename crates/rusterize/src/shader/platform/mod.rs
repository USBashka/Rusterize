#[cfg(target_os = "windows")]
mod wgl;
#[cfg(target_os = "windows")]
pub use wgl::*;
#[cfg(any(target_os = "linux", target_os = "android"))]
mod egl;
#[cfg(any(target_os = "linux", target_os = "android"))]
pub use egl::*;
#[cfg(target_os = "macos")]
mod cgl;
#[cfg(target_os = "macos")]
pub use cgl::*;
#[cfg(not(any(
    target_os = "windows",
    target_os = "linux",
    target_os = "android",
    target_os = "macos"
)))]
compile_error!("Rusterize shaders support Windows, Linux, Android and macOS");
