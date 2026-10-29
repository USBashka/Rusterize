//! Minimal EGLImage bridge for the Android Java compositor. No pixels cross JNI.
use jni::{
    objects::{JClass, JObject},
    sys::jlong,
    JNIEnv, NativeMethod,
};
use std::{
    ffi::{c_char, c_void},
    ptr,
};

type Handle = *mut c_void;
#[link(name = "android")]
extern "C" {
    fn AHardwareBuffer_fromHardwareBuffer(
        env: *mut jni::sys::JNIEnv,
        buffer: jni::sys::jobject,
    ) -> Handle;
}
#[link(name = "EGL")]
extern "C" {
    fn eglGetProcAddress(name: *const c_char) -> *const c_void;
    fn eglGetCurrentDisplay() -> Handle;
}

// Called only with the Java compositor's ES context current and its target texture bound.
unsafe extern "system" fn attach(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    buffer: JObject<'_>,
) -> jlong {
    let get = eglGetProcAddress(c"eglGetNativeClientBufferANDROID".as_ptr());
    let create = eglGetProcAddress(c"eglCreateImageKHR".as_ptr());
    let bind = eglGetProcAddress(c"glEGLImageTargetTexture2DOES".as_ptr());
    let destroy = eglGetProcAddress(c"eglDestroyImageKHR".as_ptr());
    if get.is_null() || create.is_null() || bind.is_null() || destroy.is_null() {
        return 0;
    }
    let get: unsafe extern "C" fn(Handle) -> Handle = std::mem::transmute(get);
    let create: unsafe extern "C" fn(Handle, Handle, u32, Handle, *const i32) -> Handle =
        std::mem::transmute(create);
    let bind: unsafe extern "C" fn(u32, Handle) = std::mem::transmute(bind);
    let native = AHardwareBuffer_fromHardwareBuffer(env.get_raw(), buffer.as_raw());
    if native.is_null() {
        return 0;
    }
    let client = get(native);
    if client.is_null() {
        return 0;
    }
    let image = create(
        eglGetCurrentDisplay(),
        ptr::null_mut(),
        0x3140,
        client,
        [0x30D2, 1, 0x3038].as_ptr(),
    );
    if !image.is_null() {
        bind(0x0DE1, image);
    }
    image as jlong
}

unsafe extern "system" fn release(_env: JNIEnv<'_>, _class: JClass<'_>, image: jlong) {
    let destroy = eglGetProcAddress(c"eglDestroyImageKHR".as_ptr());
    if !destroy.is_null() && image != 0 {
        let destroy: unsafe extern "C" fn(Handle, Handle) -> u32 = std::mem::transmute(destroy);
        destroy(eglGetCurrentDisplay(), image as Handle);
    }
}

pub(crate) fn register(env: &mut JNIEnv<'_>) -> jni::errors::Result<()> {
    let class = env.find_class("dev/rusterize/NativeShaders")?;
    env.register_native_methods(
        &class,
        &[
            NativeMethod {
                name: "attachHardwareBuffer".into(),
                sig: "(Landroid/hardware/HardwareBuffer;)J".into(),
                fn_ptr: attach as *mut c_void,
            },
            NativeMethod {
                name: "releaseHardwareBuffer".into(),
                sig: "(J)V".into(),
                fn_ptr: release as *mut c_void,
            },
        ],
    )?;
    env.set_static_field(
        &class,
        ("dev/rusterize/NativeShaders", "enabled", "Z"),
        jni::objects::JValue::Bool(1),
    )?;
    Ok(())
}
