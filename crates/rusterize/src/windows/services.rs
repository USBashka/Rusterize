use super::*;
use ::windows::Win32::{
    System::{DataExchange::*, Memory::*},
    UI::Shell::*,
};
thread_local! {
    static OWNER: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
    static CURSORS: RefCell<HashMap<isize,HCURSOR>> = RefCell::new(HashMap::new());
    static FULLSCREEN: RefCell<HashMap<isize,(isize,WINDOWPLACEMENT)>> = RefCell::new(HashMap::new());
}
pub(super) struct OwnerGuard(HWND);
pub(super) fn enter(hwnd: HWND) -> OwnerGuard {
    OwnerGuard(OWNER.with(|o| o.replace(hwnd)))
}
impl Drop for OwnerGuard {
    fn drop(&mut self) {
        OWNER.with(|o| o.set(self.0));
    }
}
/// Borrowed HWND for the current application's callback. It must not be destroyed by the app.
pub fn current_window() -> Option<HWND> {
    OWNER.with(|o| {
        let h = o.get();
        (!h.is_invalid()).then_some(h)
    })
}
pub(super) fn cursor(hwnd: HWND) -> Option<HCURSOR> {
    CURSORS.with(|c| c.borrow().get(&(hwnd.0 as isize)).copied())
}
pub(super) fn forget(hwnd: HWND) {
    CURSORS.with(|c| c.borrow_mut().remove(&(hwnd.0 as isize)));
    FULLSCREEN.with(|c| c.borrow_mut().remove(&(hwnd.0 as isize)));
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
struct Clipboard;
impl Drop for Clipboard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}
pub(crate) fn query(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let mut r = native::Reader::new(bytes);
    let op = r.u32()?;
    if op == 1 {
        let (s, style, options) = super::text::read(&mut r)?;
        return super::text::measure(&s, &style, options);
    }
    if op == 2 {
        return Ok(1023u32.to_le_bytes().to_vec());
    }
    let hwnd = current_window()
        .ok_or_else(|| Error("this native operation requires an application window".into()))?;
    let mut result = crate::protocol::Writer(Vec::new());
    unsafe {
        match op {
            3 => {
                OpenClipboard(Some(hwnd))?;
                let _guard = Clipboard;
                if IsClipboardFormatAvailable(13).is_err() {
                    return Ok(Vec::new());
                }
                let handle = GetClipboardData(13)?;
                let memory = HGLOBAL(handle.0);
                let count = GlobalSize(memory) / 2;
                if count > 32 * 1024 * 1024 {
                    return Err(Error("clipboard text exceeds 64 MiB".into()));
                }
                let p = GlobalLock(memory).cast::<u16>();
                if p.is_null() {
                    return Err(::windows::core::Error::from_thread().into());
                }
                let units = std::slice::from_raw_parts(p, count);
                let end = units.iter().position(|v| *v == 0).unwrap_or(count);
                let text = String::from_utf16_lossy(&units[..end]);
                let _ = GlobalUnlock(memory);
                return Ok(text.into_bytes());
            }
            4 => {
                let s = wide(&r.string()?);
                OpenClipboard(Some(hwnd))?;
                let _guard = Clipboard;
                let mem = GlobalAlloc(GMEM_MOVEABLE, s.len() * 2)?;
                let p = GlobalLock(mem).cast::<u16>();
                if p.is_null() {
                    let _ = GlobalFree(Some(mem));
                    return Err(::windows::core::Error::from_thread().into());
                }
                std::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
                let _ = GlobalUnlock(mem);
                let transfer =
                    EmptyClipboard().and_then(|_| SetClipboardData(13, Some(HANDLE(mem.0))));
                if let Err(e) = transfer {
                    let _ = GlobalFree(Some(mem));
                    return Err(e.into());
                }
            }
            5 => {
                let index = r.u32()?;
                let value = match index {
                    0 => IDC_ARROW,
                    1 => IDC_IBEAM,
                    2 => IDC_HAND,
                    3 => IDC_CROSS,
                    4 => IDC_SIZEALL,
                    5 => IDC_SIZEWE,
                    6 => IDC_SIZENS,
                    7 => PCWSTR::null(),
                    _ => return Err(Error("unknown cursor".into())),
                };
                let cursor = if index == 7 {
                    HCURSOR::default()
                } else {
                    LoadCursorW(None, value)?
                };
                CURSORS.with(|c| c.borrow_mut().insert(hwnd.0 as isize, cursor));
                SetCursor(Some(cursor));
            }
            6 => {
                let state = r.u32()?;
                let key = hwnd.0 as isize;
                if state != 3 {
                    if let Some((style, placement)) =
                        FULLSCREEN.with(|f| f.borrow_mut().remove(&key))
                    {
                        SetWindowLongPtrW(hwnd, GWL_STYLE, style);
                        SetWindowPlacement(hwnd, &placement)?;
                        SetWindowPos(
                            hwnd,
                            None,
                            0,
                            0,
                            0,
                            0,
                            SWP_NOMOVE
                                | SWP_NOSIZE
                                | SWP_NOZORDER
                                | SWP_NOACTIVATE
                                | SWP_FRAMECHANGED,
                        )?;
                    }
                    let _ = ShowWindow(
                        hwnd,
                        match state {
                            0 => SW_RESTORE,
                            1 => SW_MINIMIZE,
                            2 => SW_MAXIMIZE,
                            _ => return Err(Error("unknown window state".into())),
                        },
                    );
                } else if !FULLSCREEN.with(|f| f.borrow().contains_key(&key)) {
                    let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
                    let mut placement = WINDOWPLACEMENT {
                        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                        ..Default::default()
                    };
                    GetWindowPlacement(hwnd, &mut placement)?;
                    let mut info = MONITORINFO {
                        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    if !GetMonitorInfoW(
                        MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
                        &mut info,
                    )
                    .as_bool()
                    {
                        return Err(::windows::core::Error::from_thread().into());
                    }
                    FULLSCREEN.with(|f| f.borrow_mut().insert(key, (style, placement)));
                    SetWindowLongPtrW(hwnd, GWL_STYLE, style & !(WS_OVERLAPPEDWINDOW.0 as isize));
                    let m = info.rcMonitor;
                    SetWindowPos(
                        hwnd,
                        None,
                        m.left,
                        m.top,
                        m.right - m.left,
                        m.bottom - m.top,
                        SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                    )?;
                }
            }
            7 => {
                let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
                let mut rect = RECT {
                    right: (r.f32()? * scale).round() as i32,
                    bottom: (r.f32()? * scale).round() as i32,
                    ..Default::default()
                };
                AdjustWindowRectExForDpi(
                    &mut rect,
                    WINDOW_STYLE(GetWindowLongPtrW(hwnd, GWL_STYLE) as u32),
                    false,
                    WINDOW_EX_STYLE(GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32),
                    GetDpiForWindow(hwnd),
                )?;
                SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                )?;
            }
            8 => {
                let x = r.f32()? as i32;
                let y = r.f32()? as i32;
                SetWindowPos(
                    hwnd,
                    None,
                    x,
                    y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                )?;
            }
            9 => {
                SetWindowPos(
                    hwnd,
                    Some(if r.u32()? != 0 {
                        HWND_TOPMOST
                    } else {
                        HWND_NOTOPMOST
                    }),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )?;
            }
            10 => {
                let save = r.u32()? != 0;
                let directory = r.u32()? != 0;
                let title = wide(&r.string()?);
                let name = wide(&r.string()?);
                let dialog: IFileDialog = if save {
                    CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)?
                } else {
                    CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?
                };
                let flags = dialog.GetOptions()?
                    | FOS_FORCEFILESYSTEM
                    | if directory {
                        FOS_PICKFOLDERS
                    } else {
                        FILEOPENDIALOGOPTIONS(0)
                    };
                dialog.SetOptions(flags)?;
                dialog.SetTitle(PCWSTR(title.as_ptr()))?;
                if name.len() > 1 {
                    dialog.SetFileName(PCWSTR(name.as_ptr()))?;
                }
                match dialog.Show(Some(hwnd)) {
                    Ok(()) => {
                        let item = dialog.GetResult()?;
                        let path = item.GetDisplayName(SIGDN_FILESYSPATH)?;
                        let text = path.to_string();
                        CoTaskMemFree(Some(path.0.cast()));
                        result.u32(1);
                        result.bytes(text.map_err(|e| Error(e.to_string()))?.as_bytes());
                    }
                    Err(e)
                        if e.code() == ::windows::core::HRESULT::from_win32(ERROR_CANCELLED.0) =>
                    {
                        result.u32(0)
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            11 => {
                let title = wide(&r.string()?);
                let message = wide(&r.string()?);
                if MessageBoxW(
                    Some(hwnd),
                    PCWSTR(message.as_ptr()),
                    PCWSTR(title.as_ptr()),
                    MB_OK | MB_ICONINFORMATION,
                )
                .0 == 0
                {
                    return Err(::windows::core::Error::from_thread().into());
                }
            }
            12 => {
                let uri = wide(&r.string()?);
                let code = ShellExecuteW(
                    Some(hwnd),
                    w!("open"),
                    PCWSTR(uri.as_ptr()),
                    None,
                    None,
                    SW_SHOWNORMAL,
                )
                .0 as isize;
                if code <= 32 {
                    return Err(Error(format!("ShellExecute failed: {code}")));
                }
            }
            _ => return Err(Error(format!("unsupported native operation: {op}"))),
        }
    }
    Ok(result.0)
}
