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

#[cfg(feature = "native")]
pub use interop::SharedSurface;
#[cfg(feature = "native")]
mod interop {
    use super::*;

    use glow::HasContext;
    use windows::{
        core::Interface,
        Win32::Graphics::{
            Direct2D::{Common::*, *},
            Direct3D11::*,
            Dxgi::{Common::*, IDXGISurface},
        },
    };

    type OpenDevice = unsafe extern "system" fn(*mut c_void) -> *mut c_void;
    type CloseDevice = unsafe extern "system" fn(*mut c_void) -> i32;
    type RegisterObject =
        unsafe extern "system" fn(*mut c_void, *mut c_void, u32, u32, u32) -> *mut c_void;
    type UnregisterObject = unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32;
    type LockObjects = unsafe extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> i32;

    /// One D3D11 texture shared with GL. Ownership is transferred with the driver's
    /// lock/unlock operations; neither API may touch the texture while the other owns it.
    pub struct SharedSurface {
        pub texture: glow::Texture,
        pub bitmap: ID2D1Bitmap,
        pub size: [u32; 2],
        device: ID3D11Device,
        _resource: ID3D11Texture2D,
        interop: *mut c_void,
        object: *mut c_void,
        close: CloseDevice,
        unregister: UnregisterObject,
        lock: LockObjects,
        unlock: LockObjects,
        dc: HDC,
        context: HGLRC,
        locked: bool,
    }

    impl SharedSurface {
        pub fn new(
            native: &NativeContext,
            gl: &glow::Context,
            device: &ID3D11Device,
            target: &ID2D1DeviceContext,
            size: [u32; 2],
        ) -> Result<Self, Error> {
            unsafe {
                type Extensions = unsafe extern "system" fn(HDC) -> *const std::ffi::c_char;
                let extensions = native.get_proc("wglGetExtensionsStringARB");
                if extensions.is_null() {
                    return Err(Error("WGL extension discovery is unavailable".into()));
                }
                let extensions: Extensions = std::mem::transmute(extensions);
                let extensions = extensions(native.dc);
                if extensions.is_null()
                    || !std::ffi::CStr::from_ptr(extensions)
                        .to_bytes()
                        .split(|b| *b == b' ')
                        .any(|s| s == b"WGL_NV_DX_interop2")
                {
                    return Err(Error(
                        "The driver does not support WGL_NV_DX_interop2".into(),
                    ));
                }
                let proc = |name| {
                    let p = native.get_proc(name);
                    if p.is_null() {
                        Err(Error(format!("Missing {name}")))
                    } else {
                        Ok(p)
                    }
                };
                let open: OpenDevice = std::mem::transmute(proc("wglDXOpenDeviceNV")?);
                let close: CloseDevice = std::mem::transmute(proc("wglDXCloseDeviceNV")?);
                let register: RegisterObject = std::mem::transmute(proc("wglDXRegisterObjectNV")?);
                let unregister: UnregisterObject =
                    std::mem::transmute(proc("wglDXUnregisterObjectNV")?);
                let lock: LockObjects = std::mem::transmute(proc("wglDXLockObjectsNV")?);
                let unlock: LockObjects = std::mem::transmute(proc("wglDXUnlockObjectsNV")?);
                let mut resource = None;
                device
                    .CreateTexture2D(
                        &D3D11_TEXTURE2D_DESC {
                            Width: size[0],
                            Height: size[1],
                            MipLevels: 1,
                            ArraySize: 1,
                            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            SampleDesc: DXGI_SAMPLE_DESC {
                                Count: 1,
                                Quality: 0,
                            },
                            Usage: D3D11_USAGE_DEFAULT,
                            BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0
                                as u32,
                            ..Default::default()
                        },
                        None,
                        Some(&mut resource),
                    )
                    .map_err(|e| Error(e.to_string()))?;
                let resource =
                    resource.ok_or_else(|| Error("Missing shared shader texture".into()))?;
                let surface: IDXGISurface = resource.cast().map_err(|e| Error(e.to_string()))?;
                let bitmap = target
                    .CreateBitmapFromDxgiSurface(
                        &surface,
                        Some(&D2D1_BITMAP_PROPERTIES1 {
                            pixelFormat: D2D1_PIXEL_FORMAT {
                                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                            },
                            dpiX: 96.0,
                            dpiY: 96.0,
                            ..Default::default()
                        }),
                    )
                    .and_then(|v| v.cast())
                    .map_err(|e| Error(e.to_string()))?;
                let interop = open(device.as_raw());
                if interop.is_null() {
                    return Err(Error(
                        "OpenGL and Direct3D cannot share this adapter".into(),
                    ));
                }
                let texture = match gl.create_texture() {
                    Ok(t) => t,
                    Err(e) => {
                        close(interop);
                        return Err(Error(e));
                    }
                };
                let object = register(
                    interop,
                    resource.as_raw(),
                    texture.0.get(),
                    glow::TEXTURE_2D,
                    0x0002,
                );
                if object.is_null() {
                    gl.delete_texture(texture);
                    close(interop);
                    return Err(Error("Cannot register the shared shader texture".into()));
                }
                Ok(Self {
                    texture,
                    bitmap,
                    size,
                    device: device.clone(),
                    _resource: resource,
                    interop,
                    object,
                    close,
                    unregister,
                    lock,
                    unlock,
                    dc: native.dc,
                    context: native.context,
                    locked: false,
                })
            }
        }

        pub fn matches(&self, device: &ID3D11Device, size: [u32; 2]) -> bool {
            self.device == *device && self.size == size
        }

        pub fn lock(&mut self) -> Result<(), Error> {
            if unsafe { (self.lock)(self.interop, 1, &mut self.object) } == 0 {
                return Err(Error("Cannot lock the shared shader texture".into()));
            }
            self.locked = true;
            Ok(())
        }

        pub fn unlock(&mut self) -> Result<(), Error> {
            if unsafe { (self.unlock)(self.interop, 1, &mut self.object) } == 0 {
                return Err(Error("Cannot unlock the shared shader texture".into()));
            }
            self.locked = false;
            Ok(())
        }
    }

    impl Drop for SharedSurface {
        fn drop(&mut self) {
            unsafe {
                let _previous = Current {
                    dc: wglGetCurrentDC(),
                    context: wglGetCurrentContext(),
                };
                if wglMakeCurrent(self.dc, self.context).is_ok() {
                    if self.locked {
                        let _ = self.unlock();
                    }
                    (self.unregister)(self.interop, self.object);
                    glDeleteTextures(1, &self.texture.0.get());
                    (self.close)(self.interop);
                }
            }
        }
    }
}
