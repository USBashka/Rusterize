#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
fn main() {
    #[cfg(target_os = "windows")]
    if let Err(error) = rusterize::run_app(rusterize_webview::App::default()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
    #[cfg(not(target_os = "windows"))]
    eprintln!("Use scripts/build.py with your platform's native host");
}
