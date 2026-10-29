use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
};

const STARTER: &str = r#"use rusterize::*;

#[derive(Default)]
pub struct App { count: u32 }

impl Application for App {
    fn window_options(&self) -> WindowOptions {
        WindowOptions::default().with_title("Моё приложение").with_title_bar(TitleBarStyle::dark())
    }
    fn event(&mut self, event: Event, context: &mut Context) {
        match event {
            Event::Pointer { phase: PointerPhase::Up, .. }
            | Event::Key { key: Key::Space, pressed: true, repeat: false, .. } => {
                self.count += 1;
                context.request_redraw();
            }
            _ => {}
        }
    }
    fn draw(&mut self, canvas: &mut Canvas<'_>, viewport: Viewport) {
        canvas.clear(Color::hex(0x101923));
        canvas.circle((viewport.size.width/2.0, viewport.size.height/2.0), 48.0, Color::hex(0x79e2c0));
        canvas.text(format!("Нажатия: {}", self.count), (24.0, 48.0), TextStyle::new(24.0, Color::WHITE));
    }
}

rusterize::export_app!(App);
"#;
const MAIN: &str = r#"#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
fn main() {
    #[cfg(target_os = "windows")]
    if let Err(error) = rusterize::run_app(rusterize_app::App::default()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
    #[cfg(not(target_os = "windows"))]
    { eprintln!("Use python scripts/build.py with the native host for your platform"); std::process::exit(1); }
}
"#;
const FILES: &[(&str, &str)] = &[
    (
        "hosts/web/media.js",
        include_str!("../../../hosts/web/media.js"),
    ),
    (
        "hosts/web/media-worker.js",
        include_str!("../../../hosts/web/media-worker.js"),
    ),
    (
        "hosts/android/java/dev/rusterize/NativeFiles.java",
        include_str!("../../../hosts/android/java/dev/rusterize/NativeFiles.java"),
    ),
    (
        "hosts/android/java/dev/rusterize/NativeAudio.java",
        include_str!("../../../hosts/android/java/dev/rusterize/NativeAudio.java"),
    ),
    (
        "hosts/web/wire.js",
        include_str!("../../../hosts/web/wire.js"),
    ),
    (
        "hosts/web/services.js",
        include_str!("../../../hosts/web/services.js"),
    ),
    (
        "hosts/web/shaders.js",
        include_str!("../../../hosts/web/shaders.js"),
    ),
    (
        "hosts/web/webviews.js",
        include_str!("../../../hosts/web/webviews.js"),
    ),
    (
        "hosts/web/files.js",
        include_str!("../../../hosts/web/files.js"),
    ),
    (
        "hosts/web/index.html",
        include_str!("../../../hosts/web/index.html"),
    ),
    (
        "hosts/web/rusterize.js",
        include_str!("../../../hosts/web/rusterize.js"),
    ),
    ("docs/WEB.md", include_str!("../../../docs/WEB.md")),
    (
        "hosts/android/webview/NativeWebViews.java",
        include_str!("../../../hosts/android/webview/NativeWebViews.java"),
    ),
    (
        "hosts/android/no-webview/NativeWebViews.java",
        include_str!("../../../hosts/android/no-webview/NativeWebViews.java"),
    ),
    (
        "hosts/linux/webview.h",
        include_str!("../../../hosts/linux/webview.h"),
    ),
    (
        "hosts/macos/webview.swift",
        include_str!("../../../hosts/macos/webview.swift"),
    ),
    ("docs/WEBVIEW.md", include_str!("../../../docs/WEBVIEW.md")),
    ("src/lib.rs", STARTER),
    ("src/main.rs", MAIN),
    (".gitignore", "/target/\n/dist/\n*.keystore\n__pycache__/\n"),
    (
        ".cargo/config.toml",
        include_str!("../../../.cargo/config.toml"),
    ),
    (
        "scripts/build.py",
        include_str!("../../../scripts/build.py"),
    ),
    (
        "hosts/rusterize.h",
        include_str!("../../../hosts/rusterize.h"),
    ),
    (
        "hosts/android/AndroidManifest.xml",
        include_str!("../../../hosts/android/AndroidManifest.xml"),
    ),
    (
        "hosts/android/java/dev/rusterize/MainActivity.java",
        include_str!("../../../hosts/android/java/dev/rusterize/MainActivity.java"),
    ),
    (
        "hosts/android/java/dev/rusterize/RusterizeView.java",
        include_str!("../../../hosts/android/java/dev/rusterize/RusterizeView.java"),
    ),
    (
        "hosts/android/java/dev/rusterize/NativeShaders.java",
        include_str!("../../../hosts/android/java/dev/rusterize/NativeShaders.java"),
    ),
    (
        "hosts/linux/main.c",
        include_str!("../../../hosts/linux/main.c"),
    ),
    (
        "hosts/linux/gpu.h",
        include_str!("../../../hosts/linux/gpu.h"),
    ),
    (
        "hosts/macos/main.swift",
        include_str!("../../../hosts/macos/main.swift"),
    ),
    (
        "hosts/macos/gpu.swift",
        include_str!("../../../hosts/macos/gpu.swift"),
    ),
    ("SKILL.md", include_str!("../../../SKILL.md")),
    ("docs/API.md", include_str!("../../../docs/API.md")),
    ("docs/MEDIA.md", include_str!("../../../docs/MEDIA.md")),
    (
        "docs/MEDIA_LICENSES.md",
        include_str!("../../../docs/MEDIA_LICENSES.md"),
    ),
    (
        "docs/MEDIA_VALIDATION.md",
        include_str!("../../../docs/MEDIA_VALIDATION.md"),
    ),
    (
        "docs/LYRIXX_MIGRATION.md",
        include_str!("../../../docs/LYRIXX_MIGRATION.md"),
    ),
    ("docs/SHADERS.md", include_str!("../../../docs/SHADERS.md")),
    (
        "docs/NATIVE_API.md",
        include_str!("../../../docs/NATIVE_API.md"),
    ),
    (
        "docs/ARCHITECTURE.md",
        include_str!("../../../docs/ARCHITECTURE.md"),
    ),
    (
        "docs/protocol.md",
        include_str!("../../../docs/protocol.md"),
    ),
    (
        "docs/VALIDATION.md",
        include_str!("../../../docs/VALIDATION.md"),
    ),
];
fn error(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}
fn quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}
fn generate(destination: &Path, framework: &Path) -> io::Result<()> {
    if destination.exists() {
        return Err(error(
            "The destination already exists; choose a new directory",
        ));
    }
    let name = destination
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| error("Invalid project directory"))?
        .to_ascii_lowercase()
        .replace('_', "-");
    if name.is_empty()
        || !name.as_bytes()[0].is_ascii_alphabetic()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(error(
            "Use an ASCII project name: letters, digits and hyphens",
        ));
    }
    let library = framework.join("crates/rusterize").canonicalize()?;
    if !library.join("Cargo.toml").is_file() {
        return Err(error("--framework must point to the Rusterize checkout"));
    }
    let path = library
        .to_string_lossy()
        .trim_start_matches("\\\\?\\")
        .replace('\\', "/");
    let manifest=format!("[package]\nname = {}\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n\n[lib]\nname = \"rusterize_app\"\ncrate-type = [\"rlib\", \"staticlib\", \"cdylib\"]\n\n[[bin]]\nname = {}\npath = \"src/main.rs\"\n\n[dependencies]\nrusterize = {{ path = {} }}\n\n[profile.release]\nopt-level = \"s\"\nlto = \"thin\"\ncodegen-units = 1\nstrip = \"symbols\"\npanic = \"abort\"\n",quote(&name),quote(&name),quote(&path));
    fs::create_dir(destination)?;
    fs::write(destination.join("Cargo.toml"), manifest)?;
    for (relative, text) in FILES {
        let file = destination.join(relative);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(file, text)?;
    }
    fs::write(destination.join("README.md"),format!("# {name}\n\nПриложение на Rusterize. Код: `src/lib.rs`.\n\n```sh\npython scripts/build.py --platform windows\npython scripts/build.py --platform android\npython3 scripts/build.py --platform linux\npython3 scripts/build.py --platform macos\nrustup target add wasm32-unknown-unknown\npython scripts/build.py --platform web\n```\n\nИспользуй нужную платформу и установленный SDK. [API](docs/API.md), [веб](docs/WEB.md), [SKILL.md](SKILL.md).\n\nДля веба: `python -m http.server 8000 --bind 127.0.0.1 --directory dist/web/{name}`, затем открой `http://127.0.0.1:8000`.\n\nПуть к фреймворку задан в Cargo.toml; при переносе проекта обнови его. Android APK имеет отладочную подпись.\n"))?;
    println!(
        "Created {}\nBuild: python scripts/build.py --platform <windows|android|linux|macos|web>",
        destination.display()
    );
    Ok(())
}
fn main() -> std::process::ExitCode {
    match execute() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Rusterize: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
fn execute() -> io::Result<()> {
    let mut args: Vec<_> = env::args().skip(1).collect();
    if args.first().is_some_and(|s| s == "rusterize") {
        args.remove(0);
    }
    match args.first().map(String::as_str) {
        Some("new") => {
            let destination = args.get(1).ok_or_else(|| {
                error("Usage: cargo rusterize new <directory> [--framework <checkout>]")
            })?;
            let framework = if args.len() == 5 && args[2] == "--framework" {
                return Err(error("Unexpected extra argument"));
            } else if args.len() == 4 && args[2] == "--framework" {
                PathBuf::from(&args[3])
            } else if args.len() == 2 {
                env::var_os("RUSTERIZE_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
            } else {
                return Err(error(
                    "Usage: cargo rusterize new <directory> [--framework <checkout>]",
                ));
            };
            generate(Path::new(destination), &framework)
        }
        Some("build") => {
            let script = env::current_dir()?.join("scripts/build.py");
            if !script.exists() {
                return Err(error(
                    "Run build from a Rusterize application's root directory",
                ));
            }
            let python = env::var("PYTHON").unwrap_or_else(|_| {
                if cfg!(windows) {
                    "python".into()
                } else {
                    "python3".into()
                }
            });
            if Command::new(python)
                .arg(script)
                .args(&args[1..])
                .status()?
                .success()
            {
                Ok(())
            } else {
                Err(error("Build failed"))
            }
        }
        Some("doctor") => {
            for tool in [
                "rustc",
                "cargo",
                if cfg!(windows) { "python" } else { "python3" },
            ] {
                match Command::new(tool).arg("--version").output() {
                    Ok(o) if o.status.success() => print!("{}", String::from_utf8_lossy(&o.stdout)),
                    _ => println!("{tool}: not found"),
                }
            }
            for name in [
                "ANDROID_HOME",
                "ANDROID_NDK_HOME",
                "JAVA_HOME",
                "RUSTERIZE_HOME",
            ] {
                println!(
                    "{name}: {}",
                    env::var(name).unwrap_or_else(|_| {
                        "not set (build.py also checks conventional locations)".into()
                    })
                );
            }
            Ok(())
        }
        None | Some("help" | "--help" | "-h") => {
            println!("cargo rusterize new <directory> [--framework <checkout>]\ncargo rusterize build [--platform windows|android|linux|macos|web] [build options]\ncargo rusterize doctor");
            Ok(())
        }
        _ => Err(error("Unknown command; use --help")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_to_overwrite_existing_project() {
        assert!(generate(Path::new(env!("CARGO_MANIFEST_DIR")), Path::new(".")).is_err());
    }
    #[test]
    fn quotes_paths_for_toml() {
        assert_eq!(quote("C:\\a\"b"), "\"C:\\\\a\\\"b\"");
    }
}
