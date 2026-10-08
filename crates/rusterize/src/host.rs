//! Platform-host bridge. Handles are thread-local; every call belongs to the UI thread.
use crate::*;
use std::{collections::BTreeMap, ffi::CString};

pub const REDRAW: u32 = 1;
pub const ANIMATE: u32 = 2;
pub const EXIT: u32 = 4;
pub const FAILED: u32 = 0x8000_0000;

struct Entry<A: Application> {
    engine: Engine<A>,
    bytes: Vec<u8>,
    error: CString,
    failed: bool,
    title: CString,
    window_revision: u32,
}
impl<A: Application> Entry<A> {
    fn sync_window(&mut self) {
        if self.window_revision != self.engine.window_revision() {
            self.title = CString::new(self.engine.window_options().title.replace('\0', "�"))
                .unwrap_or_default();
            self.window_revision = self.engine.window_revision();
        }
    }
}
pub struct Host<A: Application> {
    next: u64,
    entries: BTreeMap<u64, Entry<A>>,
}
impl<A: Application> Default for Host<A> {
    fn default() -> Self {
        Self {
            next: 1,
            entries: BTreeMap::new(),
        }
    }
}
impl<A: Application> Host<A> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn native_draw(&mut self, id: u64, canvas: &native::HostCanvas<'_>) -> Result<(), Error> {
        let entry = self
            .entries
            .get_mut(&id)
            .ok_or_else(|| Error("invalid application handle".into()))?;
        if entry.failed {
            return Err(Error("application has failed".into()));
        }
        let viewport = entry.engine.viewport();
        entry.engine.app.native_draw(canvas, viewport)
    }
    #[cfg(target_os = "android")]
    pub fn native_draw(
        &mut self,
        id: u64,
        canvas: &mut crate::android::NativeCanvas<'_, '_>,
    ) -> Result<(), Error> {
        let entry = self
            .entries
            .get_mut(&id)
            .ok_or_else(|| Error("invalid application handle".into()))?;
        if entry.failed {
            return Err(Error("application has failed".into()));
        }
        let viewport = entry.engine.viewport();
        entry.engine.app.native_draw(canvas, viewport)
    }
    pub fn pop_native_request(
        &mut self,
        id: u64,
    ) -> Option<(native::RequestId, native::NativeRequest)> {
        self.entries
            .get_mut(&id)
            .filter(|e| !e.failed)
            .and_then(|e| e.engine.pop_native_request())
    }
    pub fn create(&mut self, app: A) -> u64 {
        let id = self.next;
        self.next = self
            .next
            .checked_add(1)
            .expect("host handle space exhausted");
        let engine = Engine::new(app);
        let error = engine.window_options().validate().err();
        let mut entry = Entry {
            engine,
            bytes: Vec::new(),
            error: error
                .as_ref()
                .map(|e| CString::new(e.to_string()).unwrap_or_default())
                .unwrap_or_default(),
            failed: error.is_some(),
            title: CString::default(),
            window_revision: 0,
        };
        entry.sync_window();
        self.entries.insert(id, entry);
        id
    }
    pub fn destroy(&mut self, id: u64) {
        self.entries.remove(&id);
    }
    pub fn status(&self, id: u64) -> u32 {
        self.entries
            .get(&id)
            .map(|e| {
                if e.failed {
                    FAILED
                } else {
                    u32::from(e.engine.needs_redraw())
                        | (u32::from(e.engine.is_animating()) << 1)
                        | (u32::from(e.engine.should_exit()) << 2)
                }
            })
            .unwrap_or(FAILED)
    }
    pub fn event(&mut self, id: u64, event: Event) -> u32 {
        if let Some(e) = self.entries.get_mut(&id) {
            if !e.failed {
                e.engine.dispatch(event);
                e.sync_window();
            }
        }
        self.status(id)
    }
    pub fn frame(&mut self, id: u64, viewport: Viewport, time: f64) -> u32 {
        if let Some(e) = self.entries.get_mut(&id) {
            if e.failed {
                return FAILED;
            }
            let result = if !viewport.valid() {
                Err(Error("invalid viewport".into()))
            } else {
                if e.engine.viewport() != viewport {
                    e.engine.dispatch(Event::Resize(viewport));
                }
                e.engine
                    .frame(time)
                    .map(|s| protocol::encode(s, &mut e.bytes))
            };
            if let Err(error) = result {
                e.bytes.clear();
                e.error = CString::new(error.to_string().replace('\0', " ")).unwrap_or_default();
                e.failed = true;
            }
            e.sync_window();
        }
        self.status(id)
    }
    pub fn bytes(&self, id: u64) -> &[u8] {
        self.entries
            .get(&id)
            .map(|e| e.bytes.as_slice())
            .unwrap_or_default()
    }
    pub fn error_ptr(&self, id: u64) -> *const std::ffi::c_char {
        self.entries
            .get(&id)
            .map(|e| e.error.as_ptr())
            .unwrap_or(c"invalid or wrong-thread application handle".as_ptr())
    }
    pub fn title(&self, id: u64) -> &std::ffi::CStr {
        self.entries
            .get(&id)
            .map(|e| e.title.as_c_str())
            .unwrap_or(c"Rusterize")
    }
    pub fn window_option(&self, id: u64, option: u32) -> u32 {
        let Some(entry) = self.entries.get(&id) else {
            return u32::MAX;
        };
        let w = entry.engine.window_options();
        let rgb = |c: Option<Color>| {
            c.map(|c| (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32)
                .unwrap_or(u32::MAX)
        };
        match option {
            0 => w.size.width.to_bits(),
            1 => w.size.height.to_bits(),
            2 => w.min_size.width.to_bits(),
            3 => w.min_size.height.to_bits(),
            4 => rgb(w.title_bar.background),
            5 => rgb(w.title_bar.foreground),
            6 => rgb(w.title_bar.border),
            7 => u32::from(w.resizable),
            8 => entry.engine.window_revision(),
            _ => u32::MAX,
        }
    }
}

/// Shared host event ABI. Unknown event kinds are ignored.
pub fn decode_event(
    kind: u32,
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
    detail: u32,
    flags: u32,
) -> Option<Event> {
    let point = Point::new(x, y);
    match kind {
        1..=4 if point.finite() => Some(Event::Pointer {
            id: detail as u64,
            phase: match kind {
                1 => PointerPhase::Down,
                2 => PointerPhase::Move,
                3 => PointerPhase::Up,
                _ => PointerPhase::Cancel,
            },
            position: point,
            buttons: flags,
        }),
        5 if point.finite() && Point::new(dx, dy).finite() => Some(Event::Scroll {
            position: point,
            delta: Point::new(dx, dy),
        }),
        6 | 7 => Some(Event::Key {
            key: decode_key(detail),
            pressed: kind == 6,
            repeat: flags & 16 != 0,
            modifiers: Modifiers {
                shift: flags & 1 != 0,
                control: flags & 2 != 0,
                alt: flags & 4 != 0,
                meta: flags & 8 != 0,
            },
        }),
        8 => char::from_u32(detail)
            .filter(|c| !c.is_control())
            .map(|c| Event::Text(c.to_string())),
        9 => Some(Event::Focus(detail != 0)),
        10 => Some(Event::Suspend),
        11 => Some(Event::Resume),
        _ => None,
    }
}
pub fn decode_key(code: u32) -> Key {
    match code {
        13 => Key::Enter,
        32 => Key::Space,
        27 => Key::Escape,
        9 => Key::Tab,
        8 => Key::Backspace,
        46 => Key::Delete,
        37 => Key::Left,
        38 => Key::Up,
        39 => Key::Right,
        40 => Key::Down,
        36 => Key::Home,
        35 => Key::End,
        _ => Key::Unknown(code),
    }
}

/// Export a `Default + Application` type for the Android, GTK and Apple hosts.
/// Call once, in the application's library crate.
#[macro_export]
macro_rules! export_app {
    ($app:ty) => {
        std::thread_local! { static RUSTERIZE_HOST: std::cell::RefCell<$crate::host::Host<$app>> = std::cell::RefCell::new($crate::host::Host::default()); }
        std::thread_local! { static RUSTERIZE_POLLING: std::cell::Cell<bool> = const {std::cell::Cell::new(false)}; }
        /// # Safety
        /// The callback must satisfy rusterize::native::set_c_service's contract.
        #[no_mangle] pub unsafe extern "C" fn rusterize_set_native_service(callback:$crate::native::ServiceCallback) {unsafe {$crate::native::set_c_service(callback);}}
        #[no_mangle] pub extern "C" fn rusterize_poll_native(id:u64)->u32 {
            if RUSTERIZE_POLLING.with(|p|p.replace(true)) {return rusterize_status(id);}
            for _ in 0..64 {
                let request=RUSTERIZE_HOST.with(|h|h.borrow_mut().pop_native_request(id));
                let Some((request_id,request))=request else {break};
                let result=$crate::native::execute_host_request(&request);
                RUSTERIZE_HOST.with(|h|h.borrow_mut().event(id,$crate::Event::NativeResult{id:request_id,result}));
            }
            RUSTERIZE_POLLING.with(|p|p.set(false));rusterize_status(id)
        }
        #[no_mangle] pub extern "C" fn rusterize_create() -> u64 { RUSTERIZE_HOST.with(|h| h.borrow_mut().create(<$app>::default())) }
        /// # Safety
        /// The pointers must be live, borrowed native objects on the calling UI thread.
        #[cfg(any(target_os="linux",target_os="macos"))]
        #[no_mangle] pub unsafe extern "C" fn rusterize_native_draw(id:u64,context:*mut std::ffi::c_void,view:*mut std::ffi::c_void,window:*mut std::ffi::c_void)->u32 {
            let platform=if cfg!(target_os="linux") {$crate::native::HostPlatform::Gtk} else {$crate::native::HostPlatform::AppKit};
            let canvas=unsafe {$crate::native::HostCanvas::from_raw(platform,context,view,window)};
            RUSTERIZE_HOST.with(|h|h.borrow_mut().native_draw(id,&canvas)).map_or($crate::host::FAILED,|_|0)
        }
        #[no_mangle] pub extern "C" fn rusterize_destroy(id: u64) { RUSTERIZE_HOST.with(|h| h.borrow_mut().destroy(id)); }
        #[no_mangle] pub extern "C" fn rusterize_status(id: u64) -> u32 { RUSTERIZE_HOST.with(|h| h.borrow().status(id)) }
        #[no_mangle] pub extern "C" fn rusterize_title(id:u64)->*const std::ffi::c_char {RUSTERIZE_HOST.with(|h|h.borrow().title(id).as_ptr())}
        #[no_mangle] pub extern "C" fn rusterize_window_option(id:u64,option:u32)->u32 {RUSTERIZE_HOST.with(|h|h.borrow().window_option(id,option))}
        #[no_mangle] pub extern "C" fn rusterize_event(id: u64, kind: u32, x: f32, y: f32, dx: f32, dy: f32, detail: u32, flags: u32) -> u32 {
            RUSTERIZE_HOST.with(|h| { let mut h = h.borrow_mut(); if let Some(e) = $crate::host::decode_event(kind,x,y,dx,dy,detail,flags) { h.event(id,e) } else { h.status(id) } })
        }
        #[no_mangle] pub extern "C" fn rusterize_frame(id: u64, width: f32, height: f32, scale: f32, seconds: f64) -> u32 {
            RUSTERIZE_HOST.with(|h| h.borrow_mut().frame(id,$crate::Viewport::new(width,height,scale),seconds))
        }
        #[no_mangle] pub extern "C" fn rusterize_data(id: u64) -> *const u8 { RUSTERIZE_HOST.with(|h|h.borrow().bytes(id).as_ptr()) }
        #[no_mangle] pub extern "C" fn rusterize_len(id: u64) -> usize { RUSTERIZE_HOST.with(|h|h.borrow().bytes(id).len()) }
        #[no_mangle] pub extern "C" fn rusterize_error(id: u64) -> *const std::ffi::c_char { RUSTERIZE_HOST.with(|h|h.borrow().error_ptr(id)) }
        $crate::__android_exports!();
    }
}

#[doc(hidden)]
#[cfg(not(target_os = "android"))]
#[macro_export]
macro_rules! __android_exports {
    () => {};
}

#[doc(hidden)]
#[cfg(target_os = "android")]
#[macro_export]
macro_rules! __android_exports {
    () => {
        #[no_mangle]
        pub extern "system" fn Java_dev_rusterize_RusterizeView_nativeCreate(
            mut env: $crate::jni::JNIEnv,
            class: $crate::jni::objects::JClass,
        ) -> $crate::jni::sys::jlong {
            if let Err(error) = $crate::android::install_service(&mut env, class) {
                let _ = env.throw_new("java/lang/IllegalStateException", error.to_string());
                return 0;
            }
            rusterize_create() as i64
        }
        #[no_mangle]
        pub extern "system" fn Java_dev_rusterize_RusterizeView_nativePoll(
            _: $crate::jni::JNIEnv,
            _: $crate::jni::objects::JClass,
            id: i64,
        ) -> i32 {
            rusterize_poll_native(id as u64) as i32
        }
        #[no_mangle]
        pub extern "system" fn Java_dev_rusterize_RusterizeView_nativeDestroy(
            _: $crate::jni::JNIEnv,
            _: $crate::jni::objects::JClass,
            id: i64,
        ) {
            rusterize_destroy(id as u64);
        }
        #[no_mangle]
        pub extern "system" fn Java_dev_rusterize_RusterizeView_nativeDraw<'local>(
            mut env: $crate::jni::JNIEnv<'local>,
            _: $crate::jni::objects::JClass<'local>,
            id: i64,
            canvas: $crate::jni::objects::JObject<'local>,
            view: $crate::jni::objects::JObject<'local>,
        ) {
            let mut native = $crate::android::NativeCanvas {
                env: &mut env,
                canvas,
                view,
            };
            let result =
                RUSTERIZE_HOST.with(|h| h.borrow_mut().native_draw(id as u64, &mut native));
            if let Err(e) = result {
                let _ = env.throw_new("java/lang/IllegalStateException", e.to_string());
            }
        }
        #[no_mangle]
        pub extern "system" fn Java_dev_rusterize_RusterizeView_nativeEvent(
            _: $crate::jni::JNIEnv,
            _: $crate::jni::objects::JClass,
            id: i64,
            kind: i32,
            x: f32,
            y: f32,
            dx: f32,
            dy: f32,
            detail: i32,
            flags: i32,
        ) -> i32 {
            rusterize_event(
                id as u64,
                kind as u32,
                x,
                y,
                dx,
                dy,
                detail as u32,
                flags as u32,
            ) as i32
        }
        #[no_mangle]
        pub extern "system" fn Java_dev_rusterize_RusterizeView_nativeStatus(
            _: $crate::jni::JNIEnv,
            _: $crate::jni::objects::JClass,
            id: i64,
        ) -> i32 {
            rusterize_status(id as u64) as i32
        }
        #[no_mangle]
        pub extern "system" fn Java_dev_rusterize_RusterizeView_nativeOption(
            _: $crate::jni::JNIEnv,
            _: $crate::jni::objects::JClass,
            id: i64,
            option: i32,
        ) -> i32 {
            rusterize_window_option(id as u64, option as u32) as i32
        }
        #[no_mangle]
        pub extern "system" fn Java_dev_rusterize_RusterizeView_nativeTitle(
            mut env: $crate::jni::JNIEnv,
            _: $crate::jni::objects::JClass,
            id: i64,
        ) -> $crate::jni::sys::jstring {
            RUSTERIZE_HOST.with(|h| {
                match env.new_string(h.borrow().title(id as u64).to_string_lossy()) {
                    Ok(s) => s.into_raw(),
                    Err(e) => {
                        let _ = env.throw_new("java/lang/IllegalStateException", e.to_string());
                        std::ptr::null_mut()
                    }
                }
            })
        }
        #[no_mangle]
        pub extern "system" fn Java_dev_rusterize_RusterizeView_nativeFrame(
            mut env: $crate::jni::JNIEnv,
            _: $crate::jni::objects::JClass,
            id: i64,
            width: f32,
            height: f32,
            scale: f32,
            seconds: f64,
        ) -> $crate::jni::sys::jbyteArray {
            let status = rusterize_frame(id as u64, width, height, scale, seconds);
            RUSTERIZE_HOST.with(|h| {
                let h = h.borrow();
                if status & $crate::host::FAILED != 0 {
                    let message = unsafe { std::ffi::CStr::from_ptr(h.error_ptr(id as u64)) }
                        .to_string_lossy();
                    let _ = env.throw_new("java/lang/IllegalStateException", message.as_ref());
                    std::ptr::null_mut()
                } else {
                    $crate::android::byte_array(&mut env, h.bytes(id as u64))
                }
            })
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    struct App;
    impl Application for App {
        fn draw(&mut self, c: &mut Canvas<'_>, _: Viewport) {
            c.clear(Color::BLACK);
        }
    }
    #[test]
    fn stale_handles_are_rejected() {
        let mut h = Host::default();
        let a = h.create(App);
        let b = h.create(App);
        assert_ne!(a, b);
        assert_eq!(h.frame(a, Viewport::default(), 0.0), 0);
        assert!(h.bytes(a).starts_with(b"RZ02"));
        h.destroy(a);
        h.destroy(a);
        assert_eq!(h.status(a), FAILED);
        assert_eq!(h.status(b), REDRAW);
        assert_eq!(h.frame(b, Viewport::new(f32::NAN, 1.0, 1.0), 0.0), FAILED);
    }
}
