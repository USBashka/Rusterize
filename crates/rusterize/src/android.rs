use crate::{native::*, Error, Event};
use jni::{sys::jbyteArray, JNIEnv};
use std::{cell::RefCell, collections::BTreeMap, sync::OnceLock};

static JAVA: OnceLock<(jni::JavaVM, jni::objects::GlobalRef)> = OnceLock::new();
static DATA: OnceLock<std::path::PathBuf> = OnceLock::new();
static SERVICE_CLASS: OnceLock<jni::objects::GlobalRef> = OnceLock::new();

pub fn install_context(
    env: &mut JNIEnv<'_>,
    context: &jni::objects::JObject<'_>,
) -> Result<(), Error> {
    if JAVA.get().is_none() {
        let vm = env.get_java_vm().map_err(|e| Error(e.to_string()))?;
        let context = env
            .new_global_ref(context)
            .map_err(|e| Error(e.to_string()))?;
        #[cfg(feature = "audio")]
        unsafe {
            ndk_context::initialize_android_context(
                vm.get_java_vm_pointer().cast(),
                context.as_obj().as_raw().cast(),
            );
        }
        JAVA.set((vm, context))
            .map_err(|_| Error("Android context was already initialized".into()))?;
    }
    if DATA.get().is_none() {
        let file = env
            .call_method(context, "getFilesDir", "()Ljava/io/File;", &[])
            .and_then(|r| r.l())
            .map_err(|e| Error(e.to_string()))?;
        let path = env
            .call_method(&file, "getAbsolutePath", "()Ljava/lang/String;", &[])
            .and_then(|r| r.l())
            .map_err(|e| Error(e.to_string()))?;
        let path: String = env
            .get_string(&jni::objects::JString::from(path))
            .map_err(|e| Error(e.to_string()))?
            .into();
        let _ = DATA.set(path.into());
    }
    Ok(())
}
#[cfg(feature = "files")]
pub(crate) fn data_directory() -> Result<std::path::PathBuf, Error> {
    DATA.get()
        .cloned()
        .ok_or_else(|| Error("Android application storage is not initialized".into()))
}

/// Only worker-safe file operations may use this entry point.
#[cfg(feature = "files")]
pub(crate) fn worker_service(request: &[u8]) -> Result<Vec<u8>, Error> {
    let vm = &JAVA
        .get()
        .ok_or_else(|| Error("Android VM unavailable".into()))?
        .0;
    let class = SERVICE_CLASS
        .get()
        .ok_or_else(|| Error("Android file service unavailable".into()))?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|e| Error(e.to_string()))?;
    let result: jni::errors::Result<Vec<u8>> = env.with_local_frame(16, |env| {
        let bytes = env.byte_array_from_slice(request)?;
        let cls: &jni::objects::JClass<'_> = class.as_obj().into();
        let result = env
            .call_static_method(
                cls,
                "fileService",
                "([B)[B",
                &[jni::objects::JValue::Object(bytes.as_ref())],
            )?
            .l()?;
        env.convert_byte_array(jni::objects::JByteArray::from(result))
    });
    let bytes = result.map_err(|e| Error(e.to_string()))?;
    if bytes.len() < 4 {
        return Err(Error("invalid Android file response".into()));
    }
    if bytes[..4] != [0; 4] {
        return Err(Error(String::from_utf8_lossy(&bytes[4..]).into_owned()));
    }
    Ok(bytes[4..].to_vec())
}

thread_local! {
    #[allow(clippy::missing_const_for_thread_local)]
    static PENDING: RefCell<BTreeMap<(u64,u64), NativeRequest>> = const { RefCell::new(BTreeMap::new()) };
}
#[doc(hidden)]
pub fn submit(app: u64, id: RequestId, request: &NativeRequest) -> Result<bool, Error> {
    let picker = match request {
        NativeRequest::FileDialog(_) => true,
        #[cfg(feature = "files")]
        NativeRequest::Files(crate::files::FileRequest::Pick(_)) => true,
        _ => false,
    };
    if !picker {
        return Ok(false);
    }
    let mut bytes = 60u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&app.to_le_bytes());
    bytes.extend_from_slice(&id.0.to_le_bytes());
    bytes.extend_from_slice(&encode_request(request)?);
    query(&bytes)?;
    PENDING.with(|p| p.borrow_mut().insert((app, id.0), request.clone()));
    Ok(true)
}
#[doc(hidden)]
pub fn poll_result(app: u64) -> Option<Event> {
    let mut bytes = 61u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&app.to_le_bytes());
    let bytes = query(&bytes).ok()?;
    let mut r = Reader::new(&bytes);
    if r.u32().ok()? == 0 {
        return None;
    }
    let id = u64::from_le_bytes(r.take(8).ok()?.try_into().ok()?);
    let status = r.u32().ok()?;
    let request = PENDING.with(|p| p.borrow_mut().remove(&(app, id)))?;
    let result = if status == 0 {
        decode_response(&request, bytes[16..].to_vec())
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
    PENDING.with(|p| p.borrow_mut().retain(|(owner, _), _| *owner != app));
    let mut bytes = 63u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&app.to_le_bytes());
    let _ = query(&bytes);
}

/// Real Java Canvas and View, borrowed for the duration of native_draw.
pub struct NativeCanvas<'a, 'local> {
    pub env: &'a mut JNIEnv<'local>,
    pub canvas: jni::objects::JObject<'local>,
    pub view: jni::objects::JObject<'local>,
}

pub fn install_service(
    env: &mut JNIEnv<'_>,
    class: jni::objects::JClass<'_>,
) -> Result<(), crate::Error> {
    #[cfg(feature = "shaders")]
    crate::shader::android::register(env).map_err(|e| crate::Error(e.to_string()))?;
    let vm = env.get_java_vm().map_err(|e| crate::Error(e.to_string()))?;
    let class = env
        .new_global_ref(class)
        .map_err(|e| crate::Error(e.to_string()))?;
    let _ = SERVICE_CLASS.set(class.clone());
    crate::native::set_service(move |request| {
        let call = || -> jni::errors::Result<Vec<u8>> {
            let mut env = vm.attach_current_thread()?;
            env.with_local_frame(16, |env| {
                let array = env.byte_array_from_slice(request)?;
                let cls: &jni::objects::JClass<'_> = class.as_obj().into();
                let result = env.call_static_method(
                    cls,
                    "nativeService",
                    "([B)[B",
                    &[jni::objects::JValue::Object(array.as_ref())],
                );
                let object = match result {
                    Ok(v) => v.l()?,
                    Err(e) => {
                        if env.exception_check()? {
                            env.exception_describe()?;
                            env.exception_clear()?;
                        }
                        return Err(e);
                    }
                };
                let array = jni::objects::JByteArray::from(object);
                let result = env.convert_byte_array(&array)?;
                Ok(result)
            })
        };
        let bytes = call().map_err(|e| crate::Error(e.to_string()))?;
        if bytes.len() < 4 {
            return Err(crate::Error("invalid Android service response".into()));
        }
        if bytes[..4] == [0, 0, 0, 0] {
            Ok(bytes[4..].to_vec())
        } else {
            Err(crate::Error(
                String::from_utf8_lossy(&bytes[4..]).into_owned(),
            ))
        }
    });
    Ok(())
}

pub fn byte_array(env: &mut JNIEnv<'_>, bytes: &[u8]) -> jbyteArray {
    match env.byte_array_from_slice(bytes) {
        Ok(array) => array.into_raw(),
        Err(error) => {
            let _ = env.throw_new("java/lang/IllegalStateException", error.to_string());
            std::ptr::null_mut()
        }
    }
}
