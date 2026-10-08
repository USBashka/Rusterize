use crate::Error;
use std::ffi::{c_char, c_void, CString};
type Handle = *mut c_void;
#[link(name = "OpenGL", kind = "framework")]
extern "C" {
    fn CGLChoosePixelFormat(attrs: *const u32, format: *mut Handle, count: *mut i32) -> i32;
    fn CGLDestroyPixelFormat(format: Handle) -> i32;
    fn CGLCreateContext(format: Handle, shared: Handle, context: *mut Handle) -> i32;
    fn CGLDestroyContext(context: Handle) -> i32;
    fn CGLGetCurrentContext() -> Handle;
    fn CGLSetCurrentContext(context: Handle) -> i32;
    fn dlsym(handle: Handle, symbol: *const c_char) -> *const c_void;
}
pub struct NativeContext {
    context: Handle,
}
pub struct Current {
    context: Handle,
}
impl Drop for Current {
    fn drop(&mut self) {
        unsafe {
            CGLSetCurrentContext(self.context);
        }
    }
}
impl NativeContext {
    pub fn new() -> Result<Self, Error> {
        unsafe {
            let mut format = std::ptr::null_mut();
            let mut count = 0;
            // OpenGL 4.1 core. CGL is system-provided but deprecated by Apple.
            let error = CGLChoosePixelFormat([99, 0x4100, 0].as_ptr(), &mut format, &mut count);
            if error != 0 || format.is_null() {
                return Err(Error(format!("Shader CGL pixel format: {error}")));
            }
            let mut context = std::ptr::null_mut();
            let error = CGLCreateContext(format, std::ptr::null_mut(), &mut context);
            CGLDestroyPixelFormat(format);
            if error != 0 || context.is_null() {
                return Err(Error(format!("Shader CGL context: {error}")));
            }
            Ok(Self { context })
        }
    }
    pub fn activate(&self) -> Result<Current, Error> {
        unsafe {
            let previous = Current {
                context: CGLGetCurrentContext(),
            };
            let error = CGLSetCurrentContext(self.context);
            if error != 0 {
                return Err(Error(format!("Shader CGL activation: {error}")));
            }
            Ok(previous)
        }
    }
    pub fn get_proc(&self, name: &str) -> *const c_void {
        CString::new(name).map_or(std::ptr::null(), |s| unsafe {
            dlsym(-2isize as Handle, s.as_ptr())
        })
    }
}
impl Drop for NativeContext {
    fn drop(&mut self) {
        unsafe {
            if CGLGetCurrentContext() == self.context {
                CGLSetCurrentContext(std::ptr::null_mut());
            }
            CGLDestroyContext(self.context);
        }
    }
}
