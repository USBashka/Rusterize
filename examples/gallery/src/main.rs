#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
fn main() {
    #[cfg(target_os = "windows")]
    if let Err(error) = rusterize::run_app(rusterize_gallery::Gallery::default()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
    #[cfg(not(target_os = "windows"))]
    eprintln!("Build with the native host; see hosts/ and README.md");
}
