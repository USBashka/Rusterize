use jni::{sys::jbyteArray, JNIEnv};

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
    let vm = env.get_java_vm().map_err(|e| crate::Error(e.to_string()))?;
    let class = env
        .new_global_ref(class)
        .map_err(|e| crate::Error(e.to_string()))?;
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
