#![cfg_attr(windows, windows_subsystem = "windows")]
fn main() {
    #[cfg(windows)]
    if let Err(error) = rusterize::run_app(rusterize_minimal::App) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
