use crate::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub size: Size,
    pub scale: f32,
}
impl Viewport {
    pub fn new(width: f32, height: f32, scale: f32) -> Self {
        Self {
            size: Size::new(width, height),
            scale,
        }
    }
    pub fn valid(self) -> bool {
        self.size.width.is_finite()
            && self.size.height.is_finite()
            && self.scale.is_finite()
            && self.size.width >= 0.0
            && self.size.height >= 0.0
            && self.scale > 0.0
    }
}
impl Default for Viewport {
    fn default() -> Self {
        Self::new(800.0, 600.0, 1.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerPhase {
    Down,
    Move,
    Up,
    Cancel,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Enter,
    Space,
    Escape,
    Tab,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Unknown(u32),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    NativeResult {
        id: native::RequestId,
        result: Result<native::NativeResponse, Error>,
    },
    Resize(Viewport),
    /// IDs are stable for one contact. Mouse is zero. Buttons: left=1, right=2, middle=4.
    Pointer {
        id: u64,
        phase: PointerPhase,
        position: Point,
        buttons: u32,
    },
    Scroll {
        position: Point,
        delta: Point,
    },
    Key {
        key: Key,
        pressed: bool,
        repeat: bool,
        modifiers: Modifiers,
    },
    Text(String),
    Focus(bool),
    Suspend,
    Resume,
    /// Monotonic elapsed seconds, delivered only while animation is requested.
    Frame {
        elapsed: f64,
        delta: f64,
    },
}

#[derive(Default, Debug)]
pub struct Context {
    pub(crate) redraw: bool,
    pub(crate) animate: bool,
    pub(crate) exit: bool,
    window: WindowOptions,
    window_revision: u32,
    native_requests: std::collections::VecDeque<(native::RequestId, native::NativeRequest)>,
    next_request: u64,
}
impl Context {
    /// Queue a native operation; completion is delivered as `Event::NativeResult`.
    pub fn native_request(&mut self, request: native::NativeRequest) -> native::RequestId {
        self.next_request = self
            .next_request
            .checked_add(1)
            .expect("native request IDs exhausted");
        let id = native::RequestId(self.next_request);
        self.native_requests.push_back((id, request));
        self.request_redraw();
        id
    }
    pub fn read_clipboard(&mut self) -> native::RequestId {
        self.native_request(native::NativeRequest::ReadClipboard)
    }
    pub fn write_clipboard(&mut self, text: impl Into<String>) -> native::RequestId {
        self.native_request(native::NativeRequest::WriteClipboard(text.into()))
    }
    pub fn set_cursor(&mut self, cursor: native::Cursor) -> native::RequestId {
        self.native_request(native::NativeRequest::SetCursor(cursor))
    }
    pub fn set_window_state(&mut self, state: native::WindowState) -> native::RequestId {
        self.native_request(native::NativeRequest::SetWindowState(state))
    }
    pub fn set_window_size(&mut self, size: Size) -> native::RequestId {
        self.native_request(native::NativeRequest::SetWindowSize(size))
    }
    pub fn set_window_position(&mut self, position: Point) -> native::RequestId {
        self.native_request(native::NativeRequest::SetWindowPosition(position))
    }
    pub fn set_always_on_top(&mut self, enabled: bool) -> native::RequestId {
        self.native_request(native::NativeRequest::SetAlwaysOnTop(enabled))
    }
    pub fn file_dialog(&mut self, options: native::FileDialog) -> native::RequestId {
        self.native_request(native::NativeRequest::FileDialog(options))
    }
    pub fn message_dialog(
        &mut self,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> native::RequestId {
        self.native_request(native::NativeRequest::MessageDialog {
            title: title.into(),
            message: message.into(),
        })
    }
    pub fn open_uri(&mut self, uri: impl Into<String>) -> native::RequestId {
        self.native_request(native::NativeRequest::OpenUri(uri.into()))
    }
    pub fn request_redraw(&mut self) {
        self.redraw = true;
    }
    pub fn set_animation(&mut self, enabled: bool) {
        self.animate = enabled;
        self.redraw = true;
    }
    pub fn exit(&mut self) {
        self.exit = true;
    }
    /// Change the native window title without recreating the window.
    pub fn set_title(&mut self, title: impl Into<String>) {
        let title = title.into().replace('\0', "�");
        if self.window.title != title {
            self.window.title = title;
            self.window_revision = self.window_revision.wrapping_add(1);
            self.request_redraw();
        }
    }
    /// Change the native chrome. Unsupported colour attributes retain OS defaults.
    pub fn set_title_bar(&mut self, style: TitleBarStyle) {
        if self.window.title_bar != style {
            self.window.title_bar = style;
            self.window_revision = self.window_revision.wrapping_add(1);
            self.request_redraw();
        }
    }
    pub fn window_options(&self) -> &WindowOptions {
        &self.window
    }
}

pub trait Application {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn native_draw(
        &mut self,
        _canvas: &native::HostCanvas<'_>,
        _viewport: Viewport,
    ) -> Result<(), Error> {
        Ok(())
    }
    #[cfg(target_os = "android")]
    fn native_draw(
        &mut self,
        _canvas: &mut crate::android::NativeCanvas<'_, '_>,
        _viewport: Viewport,
    ) -> Result<(), Error> {
        Ok(())
    }
    /// Platform extension for all Direct2D/DirectWrite operations. Do not call BeginDraw/EndDraw.
    #[cfg(all(windows, feature = "native"))]
    fn native_draw(
        &mut self,
        _canvas: &crate::windows::NativeCanvas<'_>,
        _viewport: Viewport,
    ) -> Result<(), Error> {
        Ok(())
    }
    /// Return Some only for messages handled by the application. Framework lifecycle and paint messages are reserved.
    #[cfg(all(windows, feature = "native"))]
    fn native_event(
        &mut self,
        _message: crate::windows::WindowMessage,
    ) -> Option<::windows::Win32::Foundation::LRESULT> {
        None
    }

    /// Initial window settings, also consumed by Android, GTK and AppKit hosts.
    fn window_options(&self) -> WindowOptions {
        WindowOptions::default()
    }
    fn event(&mut self, _event: Event, _context: &mut Context) {}
    fn draw(&mut self, canvas: &mut Canvas<'_>, viewport: Viewport);
}

/// Opaque native chrome colours. `None` restores the operating system's default.
/// Alpha is ignored. A mobile host applies the background to its system bars.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TitleBarStyle {
    pub background: Option<Color>,
    pub foreground: Option<Color>,
    pub border: Option<Color>,
}
impl TitleBarStyle {
    pub const fn colors(background: Color, foreground: Color) -> Self {
        Self {
            background: Some(background),
            foreground: Some(foreground),
            border: None,
        }
    }
    pub const fn dark() -> Self {
        Self::colors(Color::hex(0x202020), Color::hex(0xf0f0f0))
    }
    pub const fn light() -> Self {
        Self::colors(Color::hex(0xf5f5f5), Color::hex(0x202020))
    }
    pub const fn with_border(mut self, color: Color) -> Self {
        self.border = Some(color);
        self
    }
}

#[derive(Clone, Debug)]
pub struct WindowOptions {
    pub title: String,
    pub size: Size,
    pub min_size: Size,
    pub title_bar: TitleBarStyle,
    pub resizable: bool,
}
impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            title: "Rusterize".into(),
            size: Size::new(960.0, 680.0),
            min_size: Size::new(320.0, 320.0),
            title_bar: TitleBarStyle::default(),
            resizable: true,
        }
    }
}
impl WindowOptions {
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into().replace('\0', "�");
        self
    }
    pub fn with_size(mut self, width: f32, height: f32) -> Self {
        self.size = Size::new(width, height);
        self
    }
    pub fn with_title_bar(mut self, style: TitleBarStyle) -> Self {
        self.title_bar = style;
        self
    }
    pub fn validate(&self) -> Result<(), Error> {
        if [self.size, self.min_size].iter().any(|s| {
            !s.width.is_finite()
                || !s.height.is_finite()
                || s.width < 1.0
                || s.height < 1.0
                || s.width > 32768.0
                || s.height > 32768.0
        }) {
            return Err(Error(
                "window dimensions must be finite and between 1 and 32768 logical pixels".into(),
            ));
        }
        if self.title.contains('\0') {
            return Err(Error("window title contains NUL".into()));
        }
        Ok(())
    }
}

/// Shared scheduling and lifecycle; hosts sleep unless invalidated or animating.
pub struct Engine<A: Application> {
    pub app: A,
    context: Context,
    viewport: Viewport,
    scene: Scene,
    suspended: bool,
    last_frame: Option<f64>,
}
impl<A: Application> Engine<A> {
    pub fn pop_native_request(&mut self) -> Option<(native::RequestId, native::NativeRequest)> {
        self.context.native_requests.pop_front()
    }
    pub fn new(app: A) -> Self {
        let options = app.window_options();
        Self::with_options(app, options)
    }
    pub fn with_options(app: A, options: WindowOptions) -> Self {
        let mut engine = Self {
            app,
            context: Context {
                window: options,
                window_revision: 1,
                ..Context::default()
            },
            viewport: Viewport::default(),
            scene: Scene::default(),
            suspended: false,
            last_frame: None,
        };
        engine.context.redraw = true;
        engine.dispatch(Event::Resume);
        engine
    }
    pub fn window_options(&self) -> &WindowOptions {
        &self.context.window
    }
    pub fn window_revision(&self) -> u32 {
        self.context.window_revision
    }
    #[cfg(all(windows, feature = "native"))]
    pub(crate) fn drawing_parts(&mut self) -> (&mut A, &Scene) {
        (&mut self.app, &self.scene)
    }
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }
    pub fn dispatch(&mut self, event: Event) {
        match &event {
            Event::Resize(v) => {
                if !v.valid() {
                    return;
                }
                self.viewport = *v;
                self.context.redraw = true;
            }
            Event::Suspend => {
                self.suspended = true;
                self.last_frame = None;
            }
            Event::Resume => {
                self.suspended = false;
                self.last_frame = None;
                self.context.redraw = true;
            }
            _ => {}
        }
        self.app.event(event, &mut self.context);
    }
    pub fn needs_redraw(&self) -> bool {
        !self.suspended
            && (self.context.redraw || !self.context.native_requests.is_empty())
            && !self.context.exit
    }
    pub fn is_animating(&self) -> bool {
        self.context.animate && !self.suspended && !self.context.exit
    }
    pub fn should_exit(&self) -> bool {
        self.context.exit
    }
    pub fn frame(&mut self, elapsed: f64) -> Result<&Scene, Error> {
        if !elapsed.is_finite() || elapsed < 0.0 {
            return Err(Error("invalid frame timestamp".into()));
        }
        self.context.redraw = false;
        if self.is_animating() {
            let delta = self
                .last_frame
                .map(|t| (elapsed - t).clamp(0.0, 0.1))
                .unwrap_or(0.0);
            self.dispatch(Event::Frame { elapsed, delta });
        }
        self.last_frame = Some(elapsed);
        let mut canvas = Canvas::new(&mut self.scene);
        self.app.draw(&mut canvas, self.viewport);
        canvas.finish()?;
        Ok(&self.scene)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct App {
        deltas: Vec<f64>,
    }
    impl Application for App {
        fn event(&mut self, e: Event, c: &mut Context) {
            match e {
                Event::Key { .. } => c.set_animation(true),
                Event::Frame { delta, .. } => self.deltas.push(delta),
                _ => {}
            }
        }
        fn draw(&mut self, c: &mut Canvas<'_>, _: Viewport) {
            c.clear(Color::WHITE);
        }
    }
    #[test]
    fn idle_and_lifecycle() {
        let mut e = Engine::new(App::default());
        assert!(e.needs_redraw());
        e.frame(0.0).unwrap();
        assert!(!e.needs_redraw());
        e.dispatch(Event::Key {
            key: Key::Space,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        });
        e.frame(1.0).unwrap();
        assert_eq!(e.app.deltas, [0.1]);
        e.dispatch(Event::Suspend);
        assert!(!e.is_animating());
        assert!(!e.needs_redraw());
        e.dispatch(Event::Resume);
        e.frame(999.0).unwrap();
        assert_eq!(e.app.deltas, [0.1, 0.0]);
    }
    #[test]
    fn chrome_updates_invalidate_once_and_sanitize_titles() {
        let mut c = Context::default();
        let style = TitleBarStyle::dark();
        c.set_title_bar(style);
        assert!(c.redraw);
        let revision = c.window_revision;
        c.redraw = false;
        c.set_title_bar(style);
        assert!(!c.redraw);
        assert_eq!(revision, c.window_revision);
        c.set_title("Hello\0world");
        assert_eq!(c.window.title, "Hello�world");
        assert!(c.window.validate().is_ok());
        c.window.min_size.width = f32::NAN;
        assert!(c.window.validate().is_err());
    }
}
