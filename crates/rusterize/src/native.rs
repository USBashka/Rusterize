//! Native services, capability discovery and a versioned host extension point.
use crate::*;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostPlatform {
    Gtk,
    AppKit,
}
/// Borrowed system objects, valid only inside `Application::native_draw`.
/// GTK: cairo_t / GtkWidget / GtkWindow. AppKit: CGContext / NSView / NSWindow.
/// Use the platform's FFI bindings; never release these borrowed objects.
pub struct HostCanvas<'a> {
    pub platform: HostPlatform,
    pub render_context: *mut std::ffi::c_void,
    pub view: *mut std::ffi::c_void,
    pub window: *mut std::ffi::c_void,
    marker: std::marker::PhantomData<&'a Rc<()>>,
}
impl HostCanvas<'_> {
    /// # Safety
    /// All pointers must reference live objects of the specified platform for the
    /// entire callback, on their owning UI thread. A snapshot may have no window.
    pub unsafe fn from_raw(
        platform: HostPlatform,
        render_context: *mut std::ffi::c_void,
        view: *mut std::ffi::c_void,
        window: *mut std::ffi::c_void,
    ) -> Self {
        Self {
            platform,
            render_context,
            view,
            window,
            marker: std::marker::PhantomData,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    Arrow,
    Text,
    Hand,
    Crosshair,
    Move,
    ResizeHorizontal,
    ResizeVertical,
    Hidden,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowState {
    Normal,
    Minimized,
    Maximized,
    Fullscreen,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDialog {
    pub title: String,
    pub suggested_name: String,
    pub directory: bool,
    pub save: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub enum NativeRequest {
    ReadClipboard,
    WriteClipboard(String),
    SetCursor(Cursor),
    SetWindowState(WindowState),
    SetWindowSize(Size),
    SetWindowPosition(Point),
    SetAlwaysOnTop(bool),
    FileDialog(FileDialog),
    MessageDialog { title: String, message: String },
    OpenUri(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeResponse {
    Done,
    Text(String),
    Paths(Vec<std::path::PathBuf>),
    Cancelled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RequestId(pub u64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeCapabilities(pub u32);
impl NativeCapabilities {
    pub const TEXT_LAYOUT: u32 = 1;
    pub const CLIPBOARD: u32 = 2;
    pub const CURSOR: u32 = 4;
    pub const WINDOW_STATE: u32 = 8;
    pub const WINDOW_SIZE: u32 = 16;
    pub const WINDOW_POSITION: u32 = 32;
    pub const ALWAYS_ON_TOP: u32 = 64;
    pub const FILE_DIALOG: u32 = 128;
    pub const MESSAGE_DIALOG: u32 = 256;
    pub const OPEN_URI: u32 = 512;
    pub fn supports(self, features: u32) -> bool {
        self.0 & features == features
    }
}
pub fn capabilities() -> NativeCapabilities {
    query(&2u32.to_le_bytes())
        .ok()
        .and_then(|v| Reader::new(&v).u32().ok())
        .map(NativeCapabilities)
        .unwrap_or_default()
}

type Service = Rc<dyn Fn(&[u8]) -> Result<Vec<u8>, Error>>;
thread_local! { static SERVICE: RefCell<Option<Service>> = const { RefCell::new(None) }; }
/// Install a synchronous service on the calling UI thread. Host callbacks must not panic.
/// See `docs/protocol.md` for the request and response format.
pub fn set_service(service: impl Fn(&[u8]) -> Result<Vec<u8>, Error> + 'static) {
    SERVICE.with(|s| *s.borrow_mut() = Some(Rc::new(service)));
}
pub fn clear_service() {
    SERVICE.with(|s| *s.borrow_mut() = None);
}
pub(crate) fn query(request: &[u8]) -> Result<Vec<u8>, Error> {
    let service = SERVICE.with(|s| s.borrow().clone());
    if let Some(service) = service {
        return service(request);
    }
    #[cfg(all(windows, feature = "native"))]
    {
        crate::windows::services::query(request)
    }
    #[cfg(not(all(windows, feature = "native")))]
    {
        Err(Error(
            "native service is not installed on this thread".into(),
        ))
    }
}

pub(crate) fn execute(request: &NativeRequest) -> Result<NativeResponse, Error> {
    let mut w = crate::protocol::Writer(Vec::new());
    match request {
        NativeRequest::ReadClipboard => w.u32(3),
        NativeRequest::WriteClipboard(s) => {
            check_string(s)?;
            w.u32(4);
            w.bytes(s.as_bytes());
        }
        NativeRequest::SetCursor(c) => {
            w.u32(5);
            w.u32(*c as u32);
        }
        NativeRequest::SetWindowState(s) => {
            w.u32(6);
            w.u32(*s as u32);
        }
        NativeRequest::SetWindowSize(s) => {
            if ![s.width, s.height]
                .iter()
                .all(|n| n.is_finite() && (1.0..=32768.0).contains(n))
            {
                return Err(Error("invalid window size".into()));
            }
            w.u32(7);
            w.f32(s.width);
            w.f32(s.height);
        }
        NativeRequest::SetWindowPosition(p) => {
            if !p.finite() || p.x.abs() > 1_000_000.0 || p.y.abs() > 1_000_000.0 {
                return Err(Error("invalid window position".into()));
            }
            w.u32(8);
            w.point(*p);
        }
        NativeRequest::SetAlwaysOnTop(v) => {
            w.u32(9);
            w.u32(u32::from(*v));
        }
        NativeRequest::FileDialog(d) => {
            check_string(&d.title)?;
            check_string(&d.suggested_name)?;
            if d.save && d.directory {
                return Err(Error("a save dialog cannot select directories".into()));
            }
            w.u32(10);
            w.u32(u32::from(d.save));
            w.u32(u32::from(d.directory));
            w.bytes(d.title.as_bytes());
            w.bytes(d.suggested_name.as_bytes());
        }
        NativeRequest::MessageDialog { title, message } => {
            check_string(title)?;
            check_string(message)?;
            w.u32(11);
            w.bytes(title.as_bytes());
            w.bytes(message.as_bytes());
        }
        NativeRequest::OpenUri(uri) => {
            check_string(uri)?;
            if !uri.contains(':') || uri.chars().any(char::is_control) {
                return Err(Error("invalid URI".into()));
            }
            w.u32(12);
            w.bytes(uri.as_bytes());
        }
    }
    let result = query(&w.0)?;
    match request {
        NativeRequest::ReadClipboard => Ok(NativeResponse::Text(
            String::from_utf8(result).map_err(|_| Error("clipboard was not UTF-8".into()))?,
        )),
        NativeRequest::FileDialog(_) => {
            let mut r = Reader::new(&result);
            let count = r.u32()?;
            if count > 1024 {
                return Err(Error("invalid file count".into()));
            }
            let paths = (0..count)
                .map(|_| r.string().map(std::path::PathBuf::from))
                .collect::<Result<Vec<_>, _>>()?;
            if !r.done() {
                return Err(Error("invalid file response".into()));
            }
            Ok(if paths.is_empty() {
                NativeResponse::Cancelled
            } else {
                NativeResponse::Paths(paths)
            })
        }
        _ => Ok(NativeResponse::Done),
    }
}
#[doc(hidden)]
pub fn execute_host_request(request: &NativeRequest) -> Result<NativeResponse, Error> {
    execute(request)
}
fn check_string(s: &str) -> Result<(), Error> {
    if s.contains('\0') || s.len() > 1024 * 1024 {
        Err(Error("native string contains NUL or exceeds 1 MiB".into()))
    } else {
        Ok(())
    }
}

pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    pub fn take(&mut self, count: usize) -> Result<&'a [u8], Error> {
        let end = self
            .offset
            .checked_add(count)
            .filter(|n| *n <= self.bytes.len())
            .ok_or_else(|| Error("truncated native response".into()))?;
        let b = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(b)
    }
    pub fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn f32(&mut self) -> Result<f32, Error> {
        let v = f32::from_bits(self.u32()?);
        if v.is_finite() {
            Ok(v)
        } else {
            Err(Error("non-finite native value".into()))
        }
    }
    pub fn string(&mut self) -> Result<String, Error> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| Error("invalid native UTF-8".into()))
    }
    pub fn done(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

/// C hosts return a borrowed buffer beginning with status u32 (0 success, 1 error).
pub type ServiceCallback = unsafe extern "C" fn(*const u8, usize, *mut usize) -> *const u8;
/// # Safety
/// Callback and its returned allocation must stay valid until the service is replaced.
/// It must return at most 64 MiB, not unwind, and run on the registering thread.
pub unsafe fn set_c_service(callback: ServiceCallback) {
    set_service(move |request| {
        let mut len = 0;
        let ptr = unsafe { callback(request.as_ptr(), request.len(), &mut len) };
        if ptr.is_null() || !(4..=64 * 1024 * 1024).contains(&len) {
            return Err(Error("invalid native service response".into()));
        }
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
        let status = u32::from_le_bytes(bytes[..4].try_into().unwrap());
        if status == 0 {
            Ok(bytes[4..].to_vec())
        } else {
            Err(Error(String::from_utf8_lossy(&bytes[4..]).into_owned()))
        }
    });
}
