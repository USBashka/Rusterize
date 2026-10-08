//! Win32 + Direct2D/DirectWrite. No bundled renderer or windowing runtime.
use crate::*;
use ::windows::{
    core::{w, Interface, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
            Dwm::*,
            Dxgi::Common::*,
            Gdi::*,
            Imaging::*,
        },
        System::{Com::*, LibraryLoader::*},
        UI::{HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    mem::ManuallyDrop,
    time::Instant,
};
use windows_numerics::{Matrix3x2, Vector2};

impl From<::windows::core::Error> for Error {
    fn from(e: ::windows::core::Error) -> Self {
        Self(e.to_string())
    }
}

/// Reports which native DWM colour attributes the current OS accepted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TitleBarSupport {
    pub background: bool,
    pub foreground: bool,
    pub border: bool,
}
fn colorref(color: Option<Color>) -> u32 {
    color
        .map(|c| c.r as u32 | ((c.g as u32) << 8) | ((c.b as u32) << 16))
        .unwrap_or(DWMWA_COLOR_DEFAULT)
}
/// Set colours on an existing HWND. Windows 10 gracefully keeps its native defaults;
/// the explicit colour attributes are supported by Windows 11 build 22000+.
pub fn set_title_bar(hwnd: HWND, style: TitleBarStyle) -> TitleBarSupport {
    unsafe {
        let set = |attribute, value: u32| {
            DwmSetWindowAttribute(hwnd, attribute, (&value as *const u32).cast(), 4).is_ok()
        };
        let dark = style
            .background
            .is_some_and(|c| (c.r as u32 * 299 + c.g as u32 * 587 + c.b as u32 * 114) < 128000);
        let _ = set(DWMWA_USE_IMMERSIVE_DARK_MODE, u32::from(dark));
        TitleBarSupport {
            background: set(DWMWA_CAPTION_COLOR, colorref(style.background)),
            foreground: set(DWMWA_TEXT_COLOR, colorref(style.foreground)),
            border: set(DWMWA_BORDER_COLOR, colorref(style.border)),
        }
    }
}
fn window_style(options: &WindowOptions) -> WINDOW_STYLE {
    if options.resizable {
        WS_OVERLAPPEDWINDOW
    } else {
        WS_OVERLAPPEDWINDOW & !WS_THICKFRAME & !WS_MAXIMIZEBOX
    }
}
fn color(c: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: c.r as f32 / 255.0,
        g: c.g as f32 / 255.0,
        b: c.b as f32 / 255.0,
        a: c.a as f32 / 255.0,
    }
}
fn point(p: Point) -> Vector2 {
    Vector2 { X: p.x, Y: p.y }
}
fn rect(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F {
        left: r.x,
        top: r.y,
        right: r.x + r.width,
        bottom: r.y + r.height,
    }
}
fn matrix(t: Transform) -> Matrix3x2 {
    let [a, b, c, d, e, f] = t.0;
    Matrix3x2 {
        M11: a,
        M12: b,
        M21: c,
        M22: d,
        M31: e,
        M32: f,
    }
}
fn rounded(r: Rect, radius: f32) -> D2D1_ROUNDED_RECT {
    let radius = radius.min(r.width / 2.0).min(r.height / 2.0);
    D2D1_ROUNDED_RECT {
        rect: rect(r),
        radiusX: radius,
        radiusY: radius,
    }
}
fn ellipse(r: Rect) -> D2D1_ELLIPSE {
    D2D1_ELLIPSE {
        point: point(Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0)),
        radiusX: r.width / 2.0,
        radiusY: r.height / 2.0,
    }
}

/// Owns device-bound resources. Recreate after `D2DERR_RECREATE_TARGET`.
pub struct D2dRenderer {
    factory: ID2D1Factory,
    write: IDWriteFactory,
    target: ID2D1RenderTarget,
    solid: ID2D1SolidColorBrush,
    stroke: ID2D1StrokeStyle,
    images: HashMap<u64, ID2D1Bitmap>,
    text: HashMap<(String, u32, u8, bool), (IDWriteTextLayout, f32)>,
    lost: bool,
}
impl D2dRenderer {
    /// Wrap an existing Direct2D target on its owning thread.
    pub fn new(factory: ID2D1Factory, target: ID2D1RenderTarget) -> Result<Self, Error> {
        unsafe {
            let write = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let solid = target.CreateSolidColorBrush(&color(Color::BLACK), None)?;
            let stroke = factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    miterLimit: 10.0,
                    ..Default::default()
                },
                None,
            )?;
            Ok(Self {
                factory,
                write,
                target,
                solid,
                stroke,
                images: HashMap::new(),
                text: HashMap::new(),
                lost: false,
            })
        }
    }
    fn brush(&self, paint: &Paint) -> Result<ID2D1Brush, Error> {
        unsafe {
            Ok(match paint {
                Paint::Solid(c) => {
                    self.solid.SetColor(&color(*c));
                    self.solid.cast()?
                }
                Paint::Linear {
                    from,
                    to,
                    start,
                    end,
                } => {
                    let stops = self.target.CreateGradientStopCollection(
                        &[
                            D2D1_GRADIENT_STOP {
                                position: 0.0,
                                color: color(*start),
                            },
                            D2D1_GRADIENT_STOP {
                                position: 1.0,
                                color: color(*end),
                            },
                        ],
                        D2D1_GAMMA_2_2,
                        D2D1_EXTEND_MODE_CLAMP,
                    )?;
                    self.target
                        .CreateLinearGradientBrush(
                            &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                                startPoint: point(*from),
                                endPoint: point(*to),
                            },
                            None,
                            &stops,
                        )?
                        .cast()?
                }
            })
        }
    }
    fn geometry(&self, shape: &Shape) -> Result<ID2D1Geometry, Error> {
        unsafe {
            Ok(match shape {
                Shape::Rect(r) => self.factory.CreateRectangleGeometry(&rect(*r))?.cast()?,
                Shape::RoundedRect(r, v) => self
                    .factory
                    .CreateRoundedRectangleGeometry(&rounded(*r, *v))?
                    .cast()?,
                Shape::Ellipse(r) => self.factory.CreateEllipseGeometry(&ellipse(*r))?.cast()?,
                Shape::Path(path) => {
                    let geometry = self.factory.CreatePathGeometry()?;
                    let sink = geometry.Open()?;
                    sink.SetFillMode(D2D1_FILL_MODE_WINDING);
                    let mut open = false;
                    for segment in path.segments() {
                        match *segment {
                            Segment::Move(p) => {
                                if open {
                                    sink.EndFigure(D2D1_FIGURE_END_OPEN);
                                }
                                sink.BeginFigure(point(p), D2D1_FIGURE_BEGIN_FILLED);
                                open = true;
                            }
                            Segment::Line(p) => sink.AddLine(point(p)),
                            Segment::Cubic(a, b, c) => sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                                point1: point(a),
                                point2: point(b),
                                point3: point(c),
                            }),
                            Segment::Close => {
                                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                                open = false;
                            }
                        }
                    }
                    if open {
                        sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    }
                    sink.Close()?;
                    geometry.cast()?
                }
            })
        }
    }
    fn shape(&self, shape: &Shape, paint: &Paint, width: Option<f32>) -> Result<(), Error> {
        let b = self.brush(paint)?;
        unsafe {
            match (shape, width) {
                (Shape::Rect(r), None) => self.target.FillRectangle(&rect(*r), &b),
                (Shape::Rect(r), Some(w)) => {
                    self.target.DrawRectangle(&rect(*r), &b, w, &self.stroke)
                }
                (Shape::RoundedRect(r, v), None) => {
                    self.target.FillRoundedRectangle(&rounded(*r, *v), &b)
                }
                (Shape::RoundedRect(r, v), Some(w)) => {
                    self.target
                        .DrawRoundedRectangle(&rounded(*r, *v), &b, w, &self.stroke)
                }
                (Shape::Ellipse(r), None) => self.target.FillEllipse(&ellipse(*r), &b),
                (Shape::Ellipse(r), Some(w)) => {
                    self.target.DrawEllipse(&ellipse(*r), &b, w, &self.stroke)
                }
                (_, None) => self.target.FillGeometry(&self.geometry(shape)?, &b, None),
                (_, Some(w)) => {
                    self.target
                        .DrawGeometry(&self.geometry(shape)?, &b, w, &self.stroke)
                }
            }
        }
        Ok(())
    }
    fn text(&mut self, text: &str, baseline: Point, style: &TextStyle) -> Result<(), Error> {
        let key = (
            text.to_owned(),
            style.size.to_bits(),
            style.family as u8,
            style.bold,
        );
        if !self.text.contains_key(&key) {
            if self.text.len() >= 256 {
                self.text.clear();
            }
            unsafe {
                let font = match style.family {
                    FontFamily::Sans => w!("Segoe UI"),
                    FontFamily::Serif => w!("Georgia"),
                    FontFamily::Monospace => w!("Consolas"),
                };
                let format = self.write.CreateTextFormat(
                    font,
                    None,
                    if style.bold {
                        DWRITE_FONT_WEIGHT_BOLD
                    } else {
                        DWRITE_FONT_WEIGHT_NORMAL
                    },
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    style.size,
                    w!(""),
                )?;
                format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                let layout = self.write.CreateTextLayout(
                    &text.encode_utf16().collect::<Vec<_>>(),
                    &format,
                    1_000_000.0,
                    1_000_000.0,
                )?;
                let mut metrics = [DWRITE_LINE_METRICS::default()];
                let mut count = 0;
                layout.GetLineMetrics(Some(&mut metrics), &mut count)?;
                self.text.insert(key.clone(), (layout, metrics[0].baseline));
            }
        }
        let (layout, offset) = &self.text[&key];
        unsafe {
            self.solid.SetColor(&color(style.color));
            self.target.DrawTextLayout(
                point(Point::new(baseline.x, baseline.y - offset)),
                layout,
                &self.solid,
                D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
            );
        }
        Ok(())
    }
    fn image(&mut self, image: &Image, destination: Rect, opacity: f32) -> Result<(), Error> {
        if !self.images.contains_key(&image.id()) {
            if self.images.len() >= 128 {
                self.images.clear();
            }
            let mut pixels = Vec::with_capacity(image.pixels().len());
            for p in image.pixels().chunks_exact(4) {
                let a = p[3] as u32;
                let pm = |c: u8| ((c as u32 * a + 127) / 255) as u8;
                pixels.extend_from_slice(&[pm(p[2]), pm(p[1]), pm(p[0]), p[3]]);
            }
            let bitmap = unsafe {
                self.target.CreateBitmap(
                    D2D_SIZE_U {
                        width: image.width(),
                        height: image.height(),
                    },
                    Some(pixels.as_ptr().cast()),
                    image.width() * 4,
                    &D2D1_BITMAP_PROPERTIES {
                        pixelFormat: D2D1_PIXEL_FORMAT {
                            format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                        },
                        dpiX: 96.0,
                        dpiY: 96.0,
                    },
                )?
            };
            self.images.insert(image.id(), bitmap);
        }
        unsafe {
            self.target.DrawBitmap(
                &self.images[&image.id()],
                Some(&rect(destination)),
                opacity,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                None,
            );
        }
        Ok(())
    }
}
impl Renderer for D2dRenderer {
    fn render(&mut self, scene: &Scene, viewport: Viewport) -> Result<(), Error> {
        let mut transform = Transform::IDENTITY;
        let mut stack = Vec::new();
        let mut layers = 0usize;
        unsafe {
            self.target
                .SetDpi(viewport.scale * 96.0, viewport.scale * 96.0);
            self.target.SetTransform(&matrix(transform));
            self.target
                .SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            self.target.BeginDraw();
            self.target.Clear(Some(&color(Color::TRANSPARENT)));
        }
        let result = (|| {
            for command in scene.commands() {
                match command {
                    Command::Clear(c) => unsafe {
                        self.target.Clear(Some(&color(*c)));
                    },
                    Command::Save => stack.push((transform, layers)),
                    Command::Restore => {
                        let (t, n) = stack
                            .pop()
                            .ok_or_else(|| Error("unbalanced scene".into()))?;
                        unsafe {
                            while layers > n {
                                self.target.PopLayer();
                                layers -= 1;
                            }
                            self.target.SetTransform(&matrix(t));
                        }
                        transform = t;
                    }
                    Command::Transform(t) => {
                        transform = transform.concat(*t);
                        unsafe {
                            self.target.SetTransform(&matrix(transform));
                        }
                    }
                    Command::Clip(s) => {
                        let geometry = self.geometry(s)?;
                        let mut params = D2D1_LAYER_PARAMETERS {
                            contentBounds: D2D_RECT_F {
                                left: -f32::MAX,
                                top: -f32::MAX,
                                right: f32::MAX,
                                bottom: f32::MAX,
                            },
                            geometricMask: ManuallyDrop::new(Some(geometry)),
                            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                            maskTransform: Matrix3x2::identity(),
                            opacity: 1.0,
                            ..Default::default()
                        };
                        unsafe {
                            self.target.PushLayer(&params, None);
                            ManuallyDrop::drop(&mut params.geometricMask);
                        }
                        layers += 1;
                    }
                    Command::Fill(s, p) => self.shape(s, p, None)?,
                    Command::Stroke(s, p, w) => self.shape(s, p, Some(*w))?,
                    Command::Text {
                        text,
                        baseline,
                        style,
                    } => self.text(text, *baseline, style)?,
                    Command::Image {
                        image,
                        destination,
                        opacity,
                    } => self.image(image, *destination, *opacity)?,
                }
            }
            Ok(())
        })();
        unsafe {
            while layers > 0 {
                self.target.PopLayer();
                layers -= 1;
            }
        }
        let end = unsafe { self.target.EndDraw(None, None) };
        self.lost = end
            .as_ref()
            .is_err_and(|e| e.code() == D2DERR_RECREATE_TARGET);
        let end = end.map_err(Error::from);
        let used: HashSet<_> = scene
            .commands()
            .iter()
            .filter_map(|c| {
                if let Command::Image { image, .. } = c {
                    Some(image.id())
                } else {
                    None
                }
            })
            .collect();
        self.images.retain(|id, _| used.contains(id));
        result.and(end)
    }
}

struct Com(bool);
impl Com {
    fn new() -> Result<Self, Error> {
        unsafe {
            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            hr.ok()?;
            Ok(Self(true))
        }
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

/// Offscreen native rendering, returning premultiplied BGRA8 pixels for tests/export.
pub fn render_offscreen(
    scene: &Scene,
    width: u32,
    height: u32,
    scale: f32,
) -> Result<Vec<u8>, Error> {
    if width == 0
        || height == 0
        || (width as u64) * (height as u64) > 16_777_216
        || !scale.is_finite()
        || scale <= 0.0
    {
        return Err(Error("invalid offscreen dimensions".into()));
    }
    let _com = Com::new()?;
    unsafe {
        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let bitmap = wic.CreateBitmap(
            width,
            height,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapCacheOnLoad,
        )?;
        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let target = factory.CreateWicBitmapRenderTarget(
            &bitmap,
            &D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                ..Default::default()
            },
        )?;
        D2dRenderer::new(factory, target)?.render(
            scene,
            Viewport::new(width as f32 / scale, height as f32 / scale, scale),
        )?;
        let mut bytes = vec![0u8; (width * height * 4) as usize];
        bitmap.CopyPixels(std::ptr::null(), width * 4, &mut bytes)?;
        Ok(bytes)
    }
}

struct State<A: Application> {
    engine: RefCell<Engine<A>>,
    device: RefCell<Option<(ID2D1HwndRenderTarget, D2dRenderer)>>,
    error: RefCell<Option<Error>>,
    start: Instant,
    options: WindowOptions,
    timer: Cell<bool>,
    minimized: Cell<bool>,
    captured: Cell<bool>,
    high_surrogate: Cell<Option<u16>>,
    window_revision: Cell<u32>,
}
impl<A: Application> State<A> {
    fn event(&self, event: Event) {
        if let Ok(mut e) = self.engine.try_borrow_mut() {
            e.dispatch(event);
        }
    }
    unsafe fn schedule(&self, hwnd: HWND) {
        let Ok(engine) = self.engine.try_borrow() else {
            return;
        };
        let (exit, redraw, animate) = (
            engine.should_exit(),
            engine.needs_redraw(),
            engine.is_animating(),
        );
        let update = if self.window_revision.get() != engine.window_revision() {
            self.window_revision.set(engine.window_revision());
            Some(engine.window_options().clone())
        } else {
            None
        };
        drop(engine);
        if let Some(options) = update {
            let title: Vec<u16> = options.title.encode_utf16().chain(Some(0)).collect();
            let _ = SetWindowTextW(hwnd, PCWSTR(title.as_ptr()));
            set_title_bar(hwnd, options.title_bar);
        }
        if exit {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            return;
        }
        if redraw {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
        if animate != self.timer.get() {
            if animate {
                self.timer.set(SetTimer(Some(hwnd), 1, 16, None) != 0);
            } else {
                let _ = KillTimer(Some(hwnd), 1);
                self.timer.set(false);
            }
        }
    }
    unsafe fn paint(&self, hwnd: HWND) -> Result<(), Error> {
        let mut bounds = RECT::default();
        GetClientRect(hwnd, &mut bounds)?;
        let width = (bounds.right - bounds.left).max(0) as u32;
        let height = (bounds.bottom - bounds.top).max(0) as u32;
        if width == 0 || height == 0 {
            return Ok(());
        }
        let scale = (GetDpiForWindow(hwnd).max(96) as f32) / 96.0;
        let viewport = Viewport::new(width as f32 / scale, height as f32 / scale, scale);
        let mut engine = self
            .engine
            .try_borrow_mut()
            .map_err(|_| Error("reentrant paint".into()))?;
        if engine.viewport() != viewport {
            engine.dispatch(Event::Resize(viewport));
        }
        let scene = engine.frame(self.start.elapsed().as_secs_f64())?;
        let mut device = self.device.borrow_mut();
        if device.is_none() {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let target = factory.CreateHwndRenderTarget(
                &D2D1_RENDER_TARGET_PROPERTIES {
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_IGNORE,
                    },
                    ..Default::default()
                },
                &D2D1_HWND_RENDER_TARGET_PROPERTIES {
                    hwnd,
                    pixelSize: D2D_SIZE_U { width, height },
                    presentOptions: D2D1_PRESENT_OPTIONS_NONE,
                },
            )?;
            let renderer = D2dRenderer::new(factory, target.cast()?)?;
            *device = Some((target, renderer));
        }
        let (target, renderer) = device.as_mut().unwrap();
        let size = target.GetPixelSize();
        if size.width != width || size.height != height {
            target.Resize(&D2D_SIZE_U { width, height })?;
        }
        // Device removal is recoverable: release all target-bound resources and retry next paint.
        match renderer.render(scene, viewport) {
            Ok(()) => Ok(()),
            Err(error) => {
                let lost = renderer.lost;
                *device = None;
                if lost {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    Ok(())
                } else {
                    Err(error)
                }
            }
        }
    }
}

pub fn run<A: Application + 'static>(app: A, options: WindowOptions) -> Result<(), Error> {
    options.validate()?;
    let _com = Com::new()?;
    let state = Box::new(State {
        engine: RefCell::new(Engine::with_options(app, options.clone())),
        device: RefCell::new(None),
        error: RefCell::new(None),
        start: Instant::now(),
        options,
        timer: Cell::new(false),
        minimized: Cell::new(false),
        captured: Cell::new(false),
        high_surrogate: Cell::new(None),
        window_revision: Cell::new(0),
    });
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = GetModuleHandleW(None)?;
        let class_name: Vec<u16> = format!("Rusterize-{:p}\0", wndproc::<A> as *const ())
            .encode_utf16()
            .collect();
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc::<A>),
            hInstance: instance.into(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(::windows::core::Error::from_thread().into());
        }
        let dpi = GetDpiForSystem();
        let scale = dpi as f32 / 96.0;
        let mut bounds = RECT {
            left: 0,
            top: 0,
            right: (state.options.size.width * scale) as i32,
            bottom: (state.options.size.height * scale) as i32,
        };
        AdjustWindowRectExForDpi(
            &mut bounds,
            window_style(&state.options),
            false,
            WINDOW_EX_STYLE::default(),
            dpi,
        )?;
        let title: Vec<u16> = state.options.title.encode_utf16().chain(Some(0)).collect();
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            window_style(&state.options),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
            None,
            None,
            Some(instance.into()),
            Some((&*state as *const State<A>).cast()),
        )?;
        state.schedule(hwnd);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let mut message = MSG::default();
        loop {
            let result = GetMessageW(&mut message, None, 0, 0).0;
            if result == 0 {
                break;
            }
            if result == -1 {
                *state.error.borrow_mut() = Some(::windows::core::Error::from_thread().into());
                break;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        if IsWindow(Some(hwnd)).as_bool() {
            let _ = DestroyWindow(hwnd);
        }
        let _ = UnregisterClassW(PCWSTR(class_name.as_ptr()), Some(instance.into()));
    }
    let error = state.error.borrow_mut().take();
    if let Some(error) = error {
        Err(error)
    } else {
        Ok(())
    }
}

unsafe extern "system" fn wndproc<A: Application>(
    hwnd: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
) -> LRESULT {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        window_message::<A>(hwnd, message, w, l)
    }));
    match result {
        Ok(result) => result,
        Err(_) => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State<A>;
            if let Some(state) = ptr.as_ref() {
                if let Ok(mut error) = state.error.try_borrow_mut() {
                    *error = Some(Error("application panicked in native callback".into()));
                }
            }
            PostQuitMessage(1);
            LRESULT(0)
        }
    }
}
unsafe fn window_message<A: Application>(
    hwnd: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(l.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State<A>;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, message, w, l);
    }
    let state = &*ptr;
    let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
    let position = Point::new(
        (l.0 as u16 as i16) as f32 / scale,
        ((l.0 >> 16) as u16 as i16) as f32 / scale,
    );
    match message {
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut ps);
            let result = state.paint(hwnd);
            let _ = EndPaint(hwnd, &ps);
            if let Err(error) = result {
                *state.error.borrow_mut() = Some(error);
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        WM_TIMER => {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
        WM_SIZE => {
            let minimized = w.0 == SIZE_MINIMIZED as usize;
            if state.minimized.replace(minimized) != minimized {
                state.event(if minimized {
                    Event::Suspend
                } else {
                    Event::Resume
                });
            }
            if !minimized {
                state.event(Event::Resize(Viewport::new(
                    (l.0 as u16) as f32 / scale,
                    ((l.0 >> 16) as u16) as f32 / scale,
                    scale,
                )));
            }
        }
        WM_DPICHANGED => {
            let r = &*(l.0 as *const RECT);
            let _ = SetWindowPos(
                hwnd,
                None,
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        WM_GETMINMAXINFO => {
            let info = &mut *(l.0 as *mut MINMAXINFO);
            let mut r = RECT {
                left: 0,
                top: 0,
                right: (state.options.min_size.width * scale) as i32,
                bottom: (state.options.min_size.height * scale) as i32,
            };
            let _ = AdjustWindowRectExForDpi(
                &mut r,
                window_style(&state.options),
                false,
                WINDOW_EX_STYLE::default(),
                (scale * 96.0) as u32,
            );
            info.ptMinTrackSize = POINT {
                x: r.right - r.left,
                y: r.bottom - r.top,
            };
        }
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONUP
        | WM_MBUTTONUP | WM_MOUSEMOVE => {
            let phase = match message {
                WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => PointerPhase::Down,
                WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => PointerPhase::Up,
                _ => PointerPhase::Move,
            };
            let buttons = (w.0 as u32 & 1) | (w.0 as u32 & 2) | ((w.0 as u32 & 16) >> 2);
            if phase == PointerPhase::Down {
                state.captured.set(true);
                SetCapture(hwnd);
                let _ = SetFocus(Some(hwnd));
            }
            if phase == PointerPhase::Up && buttons == 0 {
                state.captured.set(false);
                let _ = ReleaseCapture();
            }
            state.event(Event::Pointer {
                id: 0,
                phase,
                position,
                buttons,
            });
        }
        WM_CAPTURECHANGED => {
            if state.captured.replace(false) {
                state.event(Event::Pointer {
                    id: 0,
                    phase: PointerPhase::Cancel,
                    position: Point::default(),
                    buttons: 0,
                });
            }
        }
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
            let mut p = POINT {
                x: l.0 as u16 as i16 as i32,
                y: (l.0 >> 16) as u16 as i16 as i32,
            };
            let _ = ScreenToClient(hwnd, &mut p);
            let delta = ((w.0 >> 16) as u16 as i16) as f32 / 120.0 * 40.0;
            state.event(Event::Scroll {
                position: Point::new(p.x as f32 / scale, p.y as f32 / scale),
                delta: if message == WM_MOUSEWHEEL {
                    Point::new(0.0, -delta)
                } else {
                    Point::new(delta, 0.0)
                },
            });
        }
        WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP => {
            let pressed = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
            state.event(Event::Key {
                key: host::decode_key(w.0 as u32),
                pressed,
                repeat: pressed && (l.0 & (1 << 30)) != 0,
                modifiers: Modifiers {
                    shift: GetKeyState(0x10) < 0,
                    control: GetKeyState(0x11) < 0,
                    alt: GetKeyState(0x12) < 0,
                    meta: GetKeyState(0x5b) < 0 || GetKeyState(0x5c) < 0,
                },
            });
            state.schedule(hwnd);
            return DefWindowProcW(hwnd, message, w, l);
        }
        WM_CHAR => {
            let u = w.0 as u16;
            if (0xD800..=0xDBFF).contains(&u) {
                state.high_surrogate.set(Some(u));
            } else {
                let code = if (0xDC00..=0xDFFF).contains(&u) {
                    state
                        .high_surrogate
                        .take()
                        .map(|h| 0x10000 + ((h as u32 - 0xD800) << 10) + (u as u32 - 0xDC00))
                } else {
                    state.high_surrogate.set(None);
                    Some(u as u32)
                };
                if let Some(c) = code.and_then(char::from_u32).filter(|c| !c.is_control()) {
                    state.event(Event::Text(c.to_string()));
                }
            }
        }
        WM_SETFOCUS | WM_KILLFOCUS => {
            state.event(Event::Focus(message == WM_SETFOCUS));
        }
        WM_THEMECHANGED | WM_DWMCOMPOSITIONCHANGED => {
            state.window_revision.set(0);
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            return LRESULT(0);
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            return LRESULT(0);
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            return DefWindowProcW(hwnd, message, w, l);
        }
        _ => return DefWindowProcW(hwnd, message, w, l),
    }
    state.schedule(hwnd);
    LRESULT(0)
}
