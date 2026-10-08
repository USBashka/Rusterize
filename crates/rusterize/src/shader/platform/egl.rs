use crate::Error;
use std::ffi::{c_char, c_void, CString};
use std::ptr::null_mut;
type Handle = *mut c_void;
#[link(name = "EGL")]
extern "C" {
    fn eglGetDisplay(native: Handle) -> Handle;
    fn eglGetProcAddress(name: *const c_char) -> *const c_void;
    fn eglInitialize(display: Handle, major: *mut i32, minor: *mut i32) -> u32;
    fn eglChooseConfig(
        display: Handle,
        attrs: *const i32,
        configs: *mut Handle,
        count: i32,
        found: *mut i32,
    ) -> u32;
    fn eglBindAPI(api: u32) -> u32;
    fn eglQueryAPI() -> u32;
    fn eglCreateContext(
        display: Handle,
        config: Handle,
        shared: Handle,
        attrs: *const i32,
    ) -> Handle;
    fn eglCreatePbufferSurface(display: Handle, config: Handle, attrs: *const i32) -> Handle;
    fn eglMakeCurrent(display: Handle, draw: Handle, read: Handle, context: Handle) -> u32;
    fn eglGetCurrentDisplay() -> Handle;
    fn eglGetCurrentContext() -> Handle;
    fn eglGetCurrentSurface(which: i32) -> Handle;
    fn eglDestroyContext(display: Handle, context: Handle) -> u32;
    fn eglDestroySurface(display: Handle, surface: Handle) -> u32;
    fn eglGetError() -> u32;
}
fn failure(action: &str) -> Error {
    Error(format!("{action}: EGL 0x{:04x}", unsafe { eglGetError() }))
}
pub struct NativeContext {
    display: Handle,
    surface: Handle,
    context: Handle,
}
pub struct Current {
    owned_display: Handle,
    api: u32,
    display: Handle,
    draw: Handle,
    read: Handle,
    context: Handle,
}
impl Drop for Current {
    fn drop(&mut self) {
        unsafe {
            eglBindAPI(0x30A0);
            eglMakeCurrent(self.owned_display, null_mut(), null_mut(), null_mut());
            eglBindAPI(self.api);
            eglMakeCurrent(self.display, self.draw, self.read, self.context);
        }
    }
}
impl NativeContext {
    pub fn new() -> Result<Self, Error> {
        unsafe {
            let mut display: Handle = null_mut();
            // A surfaceless display also supports headless tests on Mesa without X11.
            #[cfg(target_os = "linux")]
            {
                let proc = eglGetProcAddress(c"eglGetPlatformDisplayEXT".as_ptr());
                if !proc.is_null() {
                    let get: unsafe extern "C" fn(u32, Handle, *const i32) -> Handle =
                        std::mem::transmute(proc);
                    let candidate = get(0x31DD, null_mut(), std::ptr::null());
                    if !candidate.is_null() && eglInitialize(candidate, null_mut(), null_mut()) != 0
                    {
                        display = candidate;
                    }
                }
            }
            if display.is_null() {
                display = eglGetDisplay(null_mut());
                if display.is_null() || eglInitialize(display, null_mut(), null_mut()) == 0 {
                    return Err(failure("Cannot initialize shader EGL display"));
                }
            }
            let attrs = [
                0x3033, 1, 0x3040, 0x40, 0x3024, 8, 0x3023, 8, 0x3022, 8, 0x3021, 8, 0x3038,
            ];
            let mut config = null_mut();
            let mut count = 0;
            if eglChooseConfig(display, attrs.as_ptr(), &mut config, 1, &mut count) == 0
                || count == 0
            {
                return Err(failure("No OpenGL ES 3 shader configuration"));
            }
            let mut native = Self {
                display,
                surface: null_mut(),
                context: null_mut(),
            };
            let previous_api = eglQueryAPI();
            if eglBindAPI(0x30A0) == 0 {
                return Err(failure("Cannot bind OpenGL ES"));
            }
            native.context =
                eglCreateContext(display, config, null_mut(), [0x3098, 3, 0x3038].as_ptr());
            eglBindAPI(previous_api);
            if native.context.is_null() {
                return Err(failure("Cannot create OpenGL ES 3 shader context"));
            }
            native.surface =
                eglCreatePbufferSurface(display, config, [0x3057, 1, 0x3056, 1, 0x3038].as_ptr());
            if native.surface.is_null() {
                return Err(failure("Cannot create shader EGL surface"));
            }
            Ok(native)
        }
    }
    pub fn activate(&self) -> Result<Current, Error> {
        unsafe {
            let display = eglGetCurrentDisplay();
            let previous = Current {
                owned_display: self.display,
                api: eglQueryAPI(),
                display: if display.is_null() {
                    self.display
                } else {
                    display
                },
                draw: eglGetCurrentSurface(0x3059),
                read: eglGetCurrentSurface(0x305A),
                context: eglGetCurrentContext(),
            };
            if eglBindAPI(0x30A0) == 0
                || eglMakeCurrent(self.display, self.surface, self.surface, self.context) == 0
            {
                return Err(failure("Cannot activate shader EGL context"));
            }
            Ok(previous)
        }
    }
    pub fn get_proc(&self, name: &str) -> *const c_void {
        CString::new(name).map_or(std::ptr::null(), |s| unsafe {
            eglGetProcAddress(s.as_ptr())
        })
    }
}
impl Drop for NativeContext {
    fn drop(&mut self) {
        unsafe {
            if !self.context.is_null() {
                if eglGetCurrentContext() == self.context {
                    eglMakeCurrent(self.display, null_mut(), null_mut(), null_mut());
                }
                eglDestroyContext(self.display, self.context);
            }
            if !self.surface.is_null() {
                eglDestroySurface(self.display, self.surface);
            }
            // EGLDisplay may also belong to GTK/Android or another renderer.
            // Never terminate their process-wide display or release their thread state.
        }
    }
}
