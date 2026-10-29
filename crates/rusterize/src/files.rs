//! Typed file locations and per-application, replace-safe persistent data.
use crate::{native::*, Error};
use std::path::PathBuf;

pub const MAX_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileLocation {
    SystemPath(PathBuf),
    ContentUri(String),
    BrowserToken(String),
}
impl FileLocation {
    pub(crate) fn from_picker(value: PathBuf) -> Self {
        let text = value.to_string_lossy();
        if text.starts_with("content://") {
            Self::ContentUri(text.into_owned())
        } else if text.starts_with("rusterize-file:") {
            Self::BrowserToken(text.into_owned())
        } else {
            Self::SystemPath(value)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileRequest {
    Pick(FileDialog),
    Read(FileLocation),
    /// Atomic replacement for system paths and browser handles. Android document
    /// providers only support writing a newly created export; existing URIs fail.
    Write {
        location: FileLocation,
        bytes: Vec<u8>,
    },
    Release(FileLocation),
    Load {
        app: String,
        key: String,
    },
    Store {
        app: String,
        key: String,
        bytes: Vec<u8>,
    },
}

pub(crate) fn validate_name(value: &str) -> Result<(), Error> {
    if value.is_empty()
        || value.len() > 120
        || value == "."
        || value == ".."
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        || value.ends_with('.')
    {
        return Err(Error("storage names must be 1–120 ASCII letters, digits, dot, dash or underscore, without traversal".into()));
    }
    // Windows device names must also be rejected on other platforms for portability.
    let stem = value.split('.').next().unwrap_or("").to_ascii_uppercase();
    if [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ]
    .contains(&stem.as_str())
    {
        return Err(Error("reserved storage name".into()));
    }
    Ok(())
}

pub(crate) fn encode(request: &FileRequest, w: &mut crate::protocol::Writer) -> Result<(), Error> {
    match request {
        FileRequest::Pick(dialog) => {
            w.0.extend(encode_request(&NativeRequest::FileDialog(dialog.clone()))?)
        }
        FileRequest::Read(location)
        | FileRequest::Write { location, .. }
        | FileRequest::Release(location) => {
            w.u32(match request {
                FileRequest::Read(_) => 40,
                FileRequest::Write { .. } => 41,
                _ => 44,
            });
            match location {
                FileLocation::SystemPath(path) => {
                    w.u32(0);
                    w.bytes(
                        path.to_str()
                            .ok_or_else(|| Error("host path is not UTF-8".into()))?
                            .as_bytes(),
                    );
                }
                FileLocation::ContentUri(uri) => {
                    check_string(uri)?;
                    if !uri.starts_with("content://") {
                        return Err(Error("expected Android content URI".into()));
                    }
                    w.u32(1);
                    w.bytes(uri.as_bytes());
                }
                FileLocation::BrowserToken(token) => {
                    check_string(token)?;
                    if !token.starts_with("rusterize-file:") {
                        return Err(Error("invalid browser token".into()));
                    }
                    w.u32(2);
                    w.bytes(token.as_bytes());
                }
            }
            if let FileRequest::Write { bytes, .. } = request {
                bounded(bytes)?;
                w.bytes(bytes);
            }
        }
        FileRequest::Load { app, key } | FileRequest::Store { app, key, .. } => {
            validate_name(app)?;
            validate_name(key)?;
            w.u32(if matches!(request, FileRequest::Load { .. }) {
                42
            } else {
                43
            });
            w.bytes(app.as_bytes());
            w.bytes(key.as_bytes());
            if let FileRequest::Store { bytes, .. } = request {
                bounded(bytes)?;
                w.bytes(bytes);
            }
        }
    }
    Ok(())
}
pub(crate) fn bounded(bytes: &[u8]) -> Result<(), Error> {
    if bytes.len() > MAX_BYTES {
        Err(Error("file operation exceeds 32 MiB".into()))
    } else {
        Ok(())
    }
}
pub(crate) fn decode(request: &FileRequest, bytes: Vec<u8>) -> Result<NativeResponse, Error> {
    match request {
        FileRequest::Pick(dialog) => {
            match decode_response(&NativeRequest::FileDialog(dialog.clone()), bytes)? {
                NativeResponse::Paths(paths) => Ok(NativeResponse::Files(
                    paths.into_iter().map(FileLocation::from_picker).collect(),
                )),
                other => Ok(other),
            }
        }
        FileRequest::Read(_) => {
            bounded(&bytes)?;
            Ok(NativeResponse::Bytes(bytes))
        }
        FileRequest::Load { .. } => match bytes.first() {
            Some(0) if bytes.len() == 1 => Ok(NativeResponse::Missing),
            Some(1) => {
                bounded(&bytes[1..])?;
                Ok(NativeResponse::Bytes(bytes[1..].to_vec()))
            }
            _ => Err(Error("invalid storage response".into())),
        },
        _ => Ok(NativeResponse::Done),
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod desktop {
    use super::*;
    use std::{
        fs::{self, OpenOptions},
        io::{Read, Write},
        path::Path,
        sync::atomic::{AtomicU64, Ordering},
    };

    pub(super) fn root(app: &str) -> Result<PathBuf, Error> {
        validate_name(app)?;
        #[cfg(target_os = "android")]
        let root = crate::android::data_directory()?;
        #[cfg(target_os = "windows")]
        let root = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| Error("LOCALAPPDATA is unavailable".into()))?;
        #[cfg(target_os = "macos")]
        let root = std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library/Application Support"))
            .ok_or_else(|| Error("HOME is unavailable".into()))?;
        #[cfg(not(any(target_os = "android", target_os = "windows", target_os = "macos")))]
        let root = std::env::var_os("XDG_DATA_HOME")
            .filter(|p| Path::new(p).is_absolute())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .ok_or_else(|| Error("application data directory is unavailable".into()))?;
        if !root.is_absolute() {
            return Err(Error("application data directory is not absolute".into()));
        }
        Ok(root.join(app))
    }
    fn read(path: &Path) -> Result<Vec<u8>, Error> {
        let file = fs::File::open(path).map_err(io_error)?;
        if file.metadata().map_err(io_error)?.len() > MAX_BYTES as u64 {
            return Err(Error("file exceeds 32 MiB".into()));
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        bounded(&bytes)?;
        Ok(bytes)
    }
    pub(crate) fn io_error(e: std::io::Error) -> Error {
        Error(format!("file {:?}: {e}", e.kind()))
    }

    pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
        bounded(bytes)?;
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            if path.as_os_str().encode_wide().any(|c| c == 0) {
                return Err(Error("file path contains NUL".into()));
            }
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let mut selected = None;
        for _ in 0..32 {
            let temp = parent.join(format!(
                ".rusterize-{}-{}.tmp",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match OpenOptions::new().write(true).create_new(true).open(&temp) {
                Ok(file) => {
                    selected = Some((temp, file));
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(io_error(e)),
            }
        }
        let (temp, mut file) =
            selected.ok_or_else(|| Error("cannot reserve a temporary file".into()))?;
        let result = (|| {
            file.write_all(bytes).map_err(io_error)?;
            file.sync_all().map_err(io_error)?;
            drop(file);
            #[cfg(not(windows))]
            fs::rename(&temp, path).map_err(io_error)?;
            #[cfg(windows)]
            {
                use std::os::windows::ffi::OsStrExt;
                #[link(name = "kernel32")]
                extern "system" {
                    fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
                }
                let from: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
                let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
                if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 1 | 8) } == 0 {
                    return Err(io_error(std::io::Error::last_os_error()));
                }
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
    pub(crate) fn execute(request: &FileRequest) -> Result<NativeResponse, Error> {
        match request {
            FileRequest::Read(FileLocation::SystemPath(path)) => {
                Ok(NativeResponse::Bytes(read(path)?))
            }
            FileRequest::Write {
                location: FileLocation::SystemPath(path),
                bytes,
            } => {
                atomic_write(path, bytes)?;
                Ok(NativeResponse::Done)
            }
            FileRequest::Release(FileLocation::SystemPath(_)) => Ok(NativeResponse::Done),
            FileRequest::Load { app, key } => {
                validate_name(key)?;
                let path = root(app)?.join(key);
                match fs::metadata(&path) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        Ok(NativeResponse::Missing)
                    }
                    Err(e) => Err(io_error(e)),
                    Ok(_) => Ok(NativeResponse::Bytes(read(&path)?)),
                }
            }
            FileRequest::Store { app, key, bytes } => {
                validate_name(key)?;
                bounded(bytes)?;
                let root = root(app)?;
                fs::create_dir_all(&root).map_err(io_error)?;
                atomic_write(&root.join(key), bytes)?;
                Ok(NativeResponse::Done)
            }
            #[cfg(target_os = "android")]
            FileRequest::Read(FileLocation::ContentUri(_))
            | FileRequest::Write {
                location: FileLocation::ContentUri(_),
                ..
            }
            | FileRequest::Release(FileLocation::ContentUri(_)) => {
                let mut w = crate::protocol::Writer(Vec::new());
                encode(request, &mut w)?;
                decode(request, crate::android::worker_service(&w.0)?)
            }
            _ => Err(Error(
                "file location is not supported on this platform".into(),
            )),
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use desktop::execute;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    #[test]
    fn names_and_location_types_are_not_interchangeable() {
        for name in [
            "",
            "..",
            "../escape",
            "a/b",
            "C:\\data",
            "a\0b",
            "CON",
            "con.txt",
            "LPT9",
        ] {
            assert!(validate_name(name).is_err(), "{name}");
        }
        assert!(validate_name("dev.example.app").is_ok());
        assert!(matches!(
            FileLocation::from_picker("content://provider/1".into()),
            FileLocation::ContentUri(_)
        ));
        assert!(matches!(
            FileLocation::from_picker("rusterize-file:1/a".into()),
            FileLocation::BrowserToken(_)
        ));
        assert!(execute(&FileRequest::Read(FileLocation::BrowserToken(
            "rusterize-file:1/a".into()
        )))
        .is_err());
        assert_eq!(
            decode(
                &FileRequest::Load {
                    app: "app".into(),
                    key: "key".into()
                },
                vec![0]
            )
            .unwrap(),
            NativeResponse::Missing
        );
        assert_eq!(
            decode(
                &FileRequest::Load {
                    app: "app".into(),
                    key: "key".into()
                },
                vec![1]
            )
            .unwrap(),
            NativeResponse::Bytes(vec![])
        );
    }
    #[test]
    fn atomic_replacement_and_failure_preserve_previous_data() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../target/rusterize-files-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("settings");
        desktop::atomic_write(&file, b"old").unwrap();
        desktop::atomic_write(&file, b"new").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"new");
        assert!(desktop::atomic_write(&file, &vec![0; MAX_BYTES + 1]).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"new");
        let directory = root.join("existing-directory");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("old"), b"preserved").unwrap();
        assert!(desktop::atomic_write(&directory, b"replacement").is_err());
        assert_eq!(std::fs::read(directory.join("old")).unwrap(), b"preserved");
        assert!(std::fs::read_dir(&root).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")));
        std::fs::remove_file(directory.join("old")).unwrap();
        std::fs::remove_dir(directory).unwrap();
        std::fs::remove_file(file).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
