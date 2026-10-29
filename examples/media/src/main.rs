#![cfg_attr(windows, windows_subsystem = "windows")]
fn main() {
    #[cfg(windows)]
    if let Err(error) = rusterize::run_app(rusterize_media::App::default()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
    #[cfg(not(windows))]
    eprintln!("Use the platform host built by scripts/build.py");
}
