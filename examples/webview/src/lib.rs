use rusterize::{webview::*, *};

const VIEW: WebViewId = WebViewId(1);
const HTML: &str = include_str!("page.html");
fn report(message: &str) {
    println!("{message}");
    #[cfg(target_os = "android")]
    {
        #[link(name = "log")]
        extern "C" {
            fn __android_log_write(
                priority: i32,
                tag: *const std::ffi::c_char,
                text: *const std::ffi::c_char,
            ) -> i32;
        }
        if let Ok(text) = std::ffi::CString::new(message) {
            unsafe {
                __android_log_write(4, c"RusterizeWebView".as_ptr(), text.as_ptr());
            }
        }
    }
}
pub struct App {
    created: bool,
    bounds: Rect,
    status: String,
    smoke: bool,
    step: u32,
    expected_error: Option<native::RequestId>,
    saw_disabled_error: bool,
    elapsed: f64,
}
impl Default for App {
    fn default() -> Self {
        Self {
            created: false,
            bounds: Rect::new(20.0, 66.0, 760.0, 474.0),
            status: "Загрузка…".into(),
            smoke: cfg!(feature = "self-check")
                || (!cfg!(target_arch = "wasm32") && std::env::args().any(|a| a == "--smoke")),
            step: 0,
            expected_error: None,
            saw_disabled_error: false,
            elapsed: 0.0,
        }
    }
}
impl App {
    fn create(&self, context: &mut Context) {
        context.webview(
            VIEW,
            WebViewCommand::Create(WebViewOptions::new(self.bounds).with_javascript(true)),
        );
    }
    fn fail(&mut self, message: String, context: &mut Context) {
        self.status = message.clone();
        context.request_redraw();
        if self.smoke {
            report(&format!("WEBVIEW_SMOKE_FAILED: {message}"));
            eprintln!("WebView smoke failed: {message}");
            #[cfg(not(target_arch = "wasm32"))]
            std::process::exit(1);
            #[cfg(target_arch = "wasm32")]
            {
                context.set_title(format!("WEBVIEW_SMOKE_FAILED: {message}"));
                context.exit();
            }
        }
    }
}
impl Application for App {
    fn window_options(&self) -> WindowOptions {
        WindowOptions::default()
            .with_title("Rusterize · WebView")
            .with_size(800.0, 560.0)
            .with_title_bar(TitleBarStyle::dark())
    }
    fn event(&mut self, event: Event, context: &mut Context) {
        match event {
            Event::Resume if !self.created => {
                self.created = true;
                self.create(context);
                if self.smoke {
                    context.set_animation(true);
                    // Cancel pending asynchronous creation and immediately reuse the ID.
                    context.webview(
                        WebViewId(2),
                        WebViewCommand::Create(WebViewOptions::new(self.bounds)),
                    );
                    context.webview(WebViewId(2), WebViewCommand::Destroy);
                    context.webview(
                        WebViewId(2),
                        WebViewCommand::Create(WebViewOptions::new(self.bounds)),
                    );
                }
            }
            Event::Resize(v) => {
                self.bounds = Rect::new(
                    20.0,
                    66.0,
                    (v.size.width - 40.0).max(1.0),
                    (v.size.height - 86.0).max(1.0),
                );
                if self.created {
                    context.webview(VIEW, WebViewCommand::SetBounds(self.bounds));
                }
            }
            Event::WebView {
                id,
                event: WebViewEvent::Ready,
            } if id == VIEW => {
                context.webview(VIEW, WebViewCommand::LoadHtml(HTML.into()));
            }
            Event::WebView {
                id: WebViewId(2),
                event: WebViewEvent::Ready,
            } if self.smoke => {
                self.expected_error = Some(context.webview(
                    WebViewId(2),
                    WebViewCommand::Evaluate {
                        script: "1".into(),
                        token: 99,
                    },
                ));
                context.webview(WebViewId(2), WebViewCommand::Destroy);
            }
            Event::WebView {
                id,
                event: WebViewEvent::Loaded { .. },
            } if id == VIEW => {
                self.status = "WebView готов".into();
                context.request_redraw();
                if self.smoke {
                    let script = if self.step == 1 {
                        "document.URL"
                    } else {
                        "document.querySelector('button').click(); Number(document.getElementById('count').textContent)"
                    };
                    context.webview(
                        VIEW,
                        WebViewCommand::Evaluate {
                            script: script.into(),
                            token: self.step,
                        },
                    );
                }
            }
            Event::WebView {
                id,
                event: WebViewEvent::ScriptResult { token, result },
            } if id == VIEW && self.smoke => {
                let expected = if token == 1 {
                    if cfg!(target_arch = "wasm32") {
                        "\"about:srcdoc\""
                    } else {
                        "\"about:blank\""
                    }
                } else {
                    "1"
                };
                if result.as_deref() != Ok(expected) {
                    self.fail(format!("Unexpected script result: {result:?}"), context);
                    return;
                }
                self.step += 1;
                match self.step {
                    1 => {
                        context.webview(VIEW, WebViewCommand::SetVisible(false));
                        context.webview(
                            VIEW,
                            WebViewCommand::SetBounds(Rect::new(30.0, 70.0, 600.0, 350.0)),
                        );
                        context.webview(VIEW, WebViewCommand::SetVisible(true));
                        context.webview(VIEW, WebViewCommand::Focus);
                        context.webview(VIEW, WebViewCommand::Navigate("about:blank".into()));
                    }
                    2 => {
                        context.webview(VIEW, WebViewCommand::Destroy);
                        self.create(context);
                    }
                    _ => {
                        if !self.saw_disabled_error {
                            self.fail("Missing disabled-JavaScript error".into(), context);
                            return;
                        }
                        report("WEBVIEW_SMOKE_OK: HTML, DOM click, JS result, bounds, visibility, focus, navigation, destroy/recreate, cancelled creation, disabled JS");
                        context.set_title("WEBVIEW_SMOKE_OK");
                        context.exit();
                    }
                }
            }
            Event::WebView {
                event: WebViewEvent::Error(error),
                ..
            } => self.fail(error, context),
            Event::NativeResult {
                id,
                result: Err(error),
            } => {
                if Some(id) == self.expected_error {
                    self.saw_disabled_error = true;
                } else {
                    self.fail(error.to_string(), context);
                }
            }
            Event::Frame { delta, .. } if self.smoke => {
                self.elapsed += delta;
                if self.elapsed > 30.0 {
                    self.fail("Timed out".into(), context);
                }
            }
            _ => {}
        }
    }
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: Viewport) {
        canvas.clear(Color::hex(0x101923));
        canvas.text(
            &self.status,
            (22.0, 40.0),
            TextStyle::new(19.0, Color::WHITE),
        );
    }
}
rusterize::export_app!(App);
