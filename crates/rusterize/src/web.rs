//! Browser services and asynchronous access to files selected by the user.
//! File paths are opaque browser tokens, valid only for this mounted instance.
use crate::{native::*, protocol::Writer, Context, Error, Event};
use std::{cell::RefCell, collections::BTreeMap, path::PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub enum FileRequest {
    Read(PathBuf),
    Write { path: PathBuf, bytes: Vec<u8> },
    ListDirectory(PathBuf),
    Release(PathBuf),
}
/// Read a file token. Completion returns `NativeResponse::Bytes` (up to 32 MiB).
pub fn read_file(context: &mut Context, path: impl Into<PathBuf>) -> RequestId {
    context.native_request(NativeRequest::BrowserFile(FileRequest::Read(path.into())))
}
/// Write a selected save/open token. The browser may ask for write permission.
pub fn write_file(context: &mut Context, path: impl Into<PathBuf>, bytes: Vec<u8>) -> RequestId {
    context.native_request(NativeRequest::BrowserFile(FileRequest::Write {
        path: path.into(),
        bytes,
    }))
}
/// List direct children as tokens in `NativeResponse::Paths` (at most 1024).
pub fn list_directory(context: &mut Context, path: impl Into<PathBuf>) -> RequestId {
    context.native_request(NativeRequest::BrowserFile(FileRequest::ListDirectory(
        path.into(),
    )))
}
/// Release a token and all children obtained through it.
pub fn release_file(context: &mut Context, path: impl Into<PathBuf>) -> RequestId {
    context.native_request(NativeRequest::BrowserFile(FileRequest::Release(
        path.into(),
    )))
}
pub(crate) fn encode_file(request: &FileRequest, w: &mut Writer) -> Result<(), Error> {
    let (op, path) = match request {
        FileRequest::Read(p) => (13, p),
        FileRequest::Write { path, .. } => (14, path),
        FileRequest::ListDirectory(p) => (15, p),
        FileRequest::Release(p) => (16, p),
    };
    let path = path
        .to_str()
        .ok_or_else(|| Error("browser file token must be UTF-8".into()))?;
    check_string(path)?;
    if !path.starts_with("rusterize-file:") {
        return Err(Error(
            "expected a browser file token, not a filesystem path".into(),
        ));
    }
    w.u32(op);
    w.bytes(path.as_bytes());
    if let FileRequest::Write { bytes, .. } = request {
        if bytes.len() > 32 * 1024 * 1024 {
            return Err(Error(
                "browser files are limited to 32 MiB per operation".into(),
            ));
        }
        w.bytes(bytes);
    }
    Ok(())
}
pub(crate) fn decode_file(request: &FileRequest, bytes: Vec<u8>) -> Result<NativeResponse, Error> {
    match request {
        FileRequest::Read(_) => Ok(NativeResponse::Bytes(bytes)),
        FileRequest::ListDirectory(_) => {
            let mut r = Reader::new(&bytes);
            let count = r.u32()?;
            if count > 1024 {
                return Err(Error("directory listing exceeds 1024 entries".into()));
            }
            let paths = (0..count)
                .map(|_| r.string().map(PathBuf::from))
                .collect::<Result<Vec<_>, _>>()?;
            if !r.done() {
                return Err(Error("invalid directory response".into()));
            }
            Ok(NativeResponse::Paths(paths))
        }
        _ => Ok(NativeResponse::Done),
    }
}

thread_local! {
    static PENDING: RefCell<BTreeMap<(u64,u64), NativeRequest>> = const { RefCell::new(BTreeMap::new()) };
}

#[link(wasm_import_module = "rusterize")]
extern "C" {
    // The JS host computes and retains a response, without re-entering Rust.
    fn service(request: *const u8, length: usize) -> usize;
    // Copy into a fresh view of memory: allocating the response may grow memory.
    fn service_read(output: *mut u8, length: usize);
    fn request(app: u64, id: u64, data: *const u8, length: usize);
    fn cancel_requests(app: u64);
}

#[doc(hidden)]
pub fn submit(app: u64, id: RequestId, operation: NativeRequest) -> Result<(), Error> {
    let bytes = encode_request(&operation)?;
    PENDING.with(|p| {
        let mut p = p.borrow_mut();
        if p.len() >= 1024 {
            return Err(Error("too many pending browser requests".into()));
        }
        p.insert((app, id.0), operation);
        Ok(())
    })?;
    // JS copies all request data before returning and delivers the result later.
    unsafe { request(app, id.0, bytes.as_ptr(), bytes.len()) };
    Ok(())
}
#[doc(hidden)]
pub fn poll_result(app: u64) -> Option<Event> {
    let mut request = Writer(Vec::new());
    request.u32(22);
    request.0.extend_from_slice(&app.to_le_bytes());
    let bytes = query(&request.0).ok()?;
    let mut r = Reader::new(&bytes);
    if r.u32().ok()? == 0 {
        return None;
    }
    let id = u64::from_le_bytes(r.take(8).ok()?.try_into().ok()?);
    let status = r.u32().ok()?;
    let operation = PENDING.with(|p| p.borrow_mut().remove(&(app, id)))?;
    let result = if status == 0 {
        decode_response(&operation, bytes[16..].to_vec())
    } else {
        Err(Error(String::from_utf8_lossy(&bytes[16..]).into_owned()))
    };
    Some(Event::NativeResult {
        id: RequestId(id),
        result,
    })
}
#[doc(hidden)]
pub fn cancel(app: u64) {
    PENDING.with(|p| p.borrow_mut().retain(|(host, _), _| *host != app));
    unsafe { cancel_requests(app) };
}

pub(crate) fn query(request: &[u8]) -> Result<Vec<u8>, Error> {
    // SAFETY: synchronous imports borrow the request and destination only during
    // the call. The bundled browser host checks both ranges and never re-enters.
    let length = unsafe { service(request.as_ptr(), request.len()) };
    if !(4..=64 * 1024 * 1024 + 16384).contains(&length) {
        return Err(Error("invalid browser service response length".into()));
    }
    let mut response = vec![0; length];
    unsafe { service_read(response.as_mut_ptr(), length) };
    let status = u32::from_le_bytes(response[..4].try_into().unwrap());
    if status != 0 {
        return Err(Error(String::from_utf8_lossy(&response[4..]).into_owned()));
    }
    response.drain(..4);
    Ok(response)
}
