use crate::Error;
use std::ffi::{c_void, CString};
use windows::{
    core::{w, PCSTR},
    Win32::{
        Foundation::*,
        Graphics::{Gdi::*, OpenGL::*},
        System::LibraryLoader::*,
        UI::WindowsAndMessaging::*,
    },
};

pub struct NativeContext {
    window: HWND,
    dc: HDC,
    context: HGLRC,
}
pub struct Current {
    dc: HDC,
    context: HGLRC,
}
impl Drop for Current {
    fn drop(&mut self) {
        unsafe {
            let _ = wglMakeCurrent(self.dc, self.context);
        }
    }
}
impl NativeContext {
    pub fn new() -> Result<Self, Error> {
        unsafe {
            let window = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!(""),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )
            .map_err(|e| Error(e.to_string()))?;
            let dc = GetDC(Some(window));
            let mut native = Self {
                window,
                dc,
                context: HGLRC::default(),
            };
            let descriptor = PIXELFORMATDESCRIPTOR {
                nSize: std::mem::size_of::<PIXELFORMATDESCRIPTOR>() as u16,
                nVersion: 1,
                dwFlags: PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL,
                iPixelType: PFD_TYPE_RGBA,
                cColorBits: 32,
                cAlphaBits: 8,
                iLayerType: PFD_MAIN_PLANE.0 as u8,
                ..Default::default()
            };
            let format = ChoosePixelFormat(dc, &descriptor);
            if format == 0 {
                return Err(Error("Cannot select a shader OpenGL pixel format".into()));
            }
            SetPixelFormat(dc, format, &descriptor).map_err(|e| Error(e.to_string()))?;
            native.context = wglCreateContext(dc).map_err(|e| Error(e.to_string()))?;
            let _previous = native.activate()?;
            let create = wglGetProcAddress(PCSTR(c"wglCreateContextAttribsARB".as_ptr().cast()))
                .ok_or_else(|| {
                    Error("The graphics driver does not provide OpenGL 3.3 contexts".into())
                })?;
            type Create = unsafe extern "system" fn(HDC, HGLRC, *const i32) -> HGLRC;
            let create: Create = std::mem::transmute(create);
            let attrs = [0x2091, 3, 0x2092, 3, 0x9126, 1, 0];
            let modern = create(dc, HGLRC::default(), attrs.as_ptr());
            if modern.is_invalid() {
                return Err(Error("Cannot create an OpenGL 3.3 core context".into()));
            }
            let old = native.context;
            if let Err(error) = wglMakeCurrent(dc, modern) {
                let _ = wglDeleteContext(modern);
                return Err(Error(error.to_string()));
            }
            native.context = modern;
            let _ = wglDeleteContext(old);
            Ok(native)
        }
    }
    pub fn activate(&self) -> Result<Current, Error> {
        unsafe {
            let previous = Current {
                dc: wglGetCurrentDC(),
                context: wglGetCurrentContext(),
            };
            wglMakeCurrent(self.dc, self.context).map_err(|e| Error(e.to_string()))?;
            Ok(previous)
        }
    }
    pub fn get_proc(&self, name: &str) -> *const c_void {
        let Ok(name) = CString::new(name) else {
            return std::ptr::null();
        };
        unsafe {
            if let Some(proc) = wglGetProcAddress(PCSTR(name.as_ptr().cast())) {
                let address = proc as *const c_void;
                if ![1, 2, 3, usize::MAX].contains(&(address as usize)) {
                    return address;
                }
            }
            GetModuleHandleW(w!("opengl32.dll"))
                .ok()
                .and_then(|m| GetProcAddress(m, PCSTR(name.as_ptr().cast())))
                .map_or(std::ptr::null(), |p| p as *const c_void)
        }
    }
}
impl Drop for NativeContext {
    fn drop(&mut self) {
        unsafe {
            if !self.context.is_invalid() {
                if wglGetCurrentContext() == self.context {
                    let _ = wglMakeCurrent(HDC::default(), HGLRC::default());
                }
                let _ = wglDeleteContext(self.context);
            }
            if !self.dc.is_invalid() {
                ReleaseDC(Some(self.window), self.dc);
            }
            let _ = DestroyWindow(self.window);
        }
    }
}
