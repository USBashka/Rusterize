//! Optional native child views, layered above Canvas. No browser engine is bundled.
//!
//! Create once, wait for `WebViewEvent::Ready`, then load HTML or an HTTP(S) URL.
//! Bounds use logical pixels, relative to the client area. Canvas transforms and
//! clipping do not apply. JavaScript is disabled unless explicitly enabled.
use crate::{native, protocol::Writer, Error, Event, Rect};

/// Application-chosen, nonzero identifier, unique among the window's live views.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WebViewId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct WebViewOptions {
    pub bounds: Rect,
    pub visible: bool,
    pub javascript: bool,
}
impl WebViewOptions {
    pub fn new(bounds: Rect) -> Self {
        Self {
            bounds,
            visible: true,
            javascript: false,
        }
    }
    pub fn with_javascript(mut self, enabled: bool) -> Self {
        self.javascript = enabled;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum WebViewCommand {
    Create(WebViewOptions),
    Destroy,
    SetBounds(Rect),
    SetVisible(bool),
    Navigate(String),
    /// A local document with no privileged Rust bridge and no local file access.
    LoadHtml(String),
    /// A JavaScript expression; `token` correlates the asynchronous JSON result.
    Evaluate {
        script: String,
        token: u32,
    },
    Focus,
    Reload,
    GoBack,
    GoForward,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebViewEvent {
    Ready,
    Loaded {
        url: String,
    },
    /// Creation or main-frame navigation failed. Destroy before retrying creation.
    Error(String),
    /// JSON-encoded value (undefined is normalized to `null`).
    ScriptResult {
        token: u32,
        result: Result<String, Error>,
    },
}

pub(crate) fn valid_url(url: &str) -> bool {
    !url.chars().any(char::is_control)
        && (url.starts_with("https://") || url.starts_with("http://") || url == "about:blank")
}

fn bounds(r: Rect, w: &mut Writer) -> Result<(), Error> {
    if ![r.x, r.y, r.width, r.height]
        .iter()
        .all(|n| n.is_finite() && n.abs() <= 32768.0)
        || r.width < 1.0
        || r.height < 1.0
    {
        return Err(Error(
            "invalid WebView bounds (dimensions must be 1..=32768)".into(),
        ));
    }
    for n in [r.x, r.y, r.width, r.height] {
        w.f32(n);
    }
    Ok(())
}

pub(crate) fn encode(id: WebViewId, command: &WebViewCommand, w: &mut Writer) -> Result<(), Error> {
    if id.0 == 0 {
        return Err(Error("WebView ID must be nonzero".into()));
    }
    w.u32(20);
    w.u32(id.0);
    match command {
        WebViewCommand::Create(o) => {
            w.u32(1);
            bounds(o.bounds, w)?;
            w.u32(u32::from(o.visible));
            w.u32(u32::from(o.javascript));
        }
        WebViewCommand::Destroy => w.u32(2),
        WebViewCommand::SetBounds(r) => {
            w.u32(3);
            bounds(*r, w)?;
        }
        WebViewCommand::SetVisible(v) => {
            w.u32(4);
            w.u32(u32::from(*v));
        }
        WebViewCommand::Navigate(url) => {
            native::check_string(url)?;
            if !valid_url(url) {
                return Err(Error("WebView URL must be HTTP(S) or about:blank".into()));
            }
            w.u32(5);
            w.bytes(url.as_bytes());
        }
        WebViewCommand::LoadHtml(html) => {
            native::check_string(html)?;
            w.u32(6);
            w.bytes(html.as_bytes());
        }
        WebViewCommand::Evaluate { script, token } => {
            native::check_string(script)?;
            w.u32(7);
            w.u32(*token);
            w.bytes(script.as_bytes());
        }
        WebViewCommand::Focus => w.u32(8),
        WebViewCommand::Reload => w.u32(9),
        WebViewCommand::GoBack => w.u32(10),
        WebViewCommand::GoForward => w.u32(11),
    }
    Ok(())
}

pub(crate) fn poll() -> Option<Event> {
    let bytes = native::query(&21u32.to_le_bytes()).ok()?;
    decode_event(&bytes).ok().flatten()
}

fn decode_event(bytes: &[u8]) -> Result<Option<Event>, Error> {
    let mut r = native::Reader::new(bytes);
    let kind = r.u32()?;
    if kind == 0 && r.done() {
        return Ok(None);
    }
    let id = WebViewId(r.u32()?);
    let token = r.u32()?;
    let value = r.string()?;
    if id.0 == 0 || !r.done() {
        return Err(Error("invalid WebView event".into()));
    }
    let event = match kind {
        1 => WebViewEvent::Ready,
        2 => WebViewEvent::Loaded { url: value },
        3 => WebViewEvent::Error(value),
        4 => WebViewEvent::ScriptResult {
            token,
            result: Ok(value),
        },
        5 => WebViewEvent::ScriptResult {
            token,
            result: Err(Error(value)),
        },
        _ => return Err(Error("unknown WebView event".into())),
    };
    Ok(Some(Event::WebView { id, event }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid_input_before_host_call() {
        for command in [
            WebViewCommand::SetBounds(Rect::new(0.0, 0.0, f32::NAN, 40.0)),
            WebViewCommand::SetBounds(Rect::new(0.0, 0.0, 0.0, 40.0)),
            WebViewCommand::Navigate("javascript:alert(1)".into()),
            WebViewCommand::Navigate("file:///secret".into()),
            WebViewCommand::LoadHtml("bad\0html".into()),
        ] {
            assert!(encode(WebViewId(1), &command, &mut Writer(vec![])).is_err());
        }
        assert!(encode(WebViewId(0), &WebViewCommand::Destroy, &mut Writer(vec![])).is_err());
    }
    #[test]
    fn create_wire_contract() {
        let mut w = Writer(vec![]);
        encode(
            WebViewId(7),
            &WebViewCommand::Create(WebViewOptions::new(Rect::new(4.0, 8.0, 320.0, 200.0))),
            &mut w,
        )
        .unwrap();
        let expected: Vec<u8> = [
            20u32,
            7,
            1,
            4f32.to_bits(),
            8f32.to_bits(),
            320f32.to_bits(),
            200f32.to_bits(),
            1,
            0,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
        assert_eq!(w.0, expected);
    }
    #[test]
    fn asynchronous_script_results_keep_id_token_and_errors() {
        let mut w = Writer(vec![]);
        for n in [5, 7, 42] {
            w.u32(n);
        }
        w.bytes(b"script failed");
        assert_eq!(
            decode_event(&w.0).unwrap(),
            Some(Event::WebView {
                id: WebViewId(7),
                event: WebViewEvent::ScriptResult {
                    token: 42,
                    result: Err(Error("script failed".into()))
                }
            })
        );
        for length in 0..w.0.len() {
            assert!(decode_event(&w.0[..length]).is_err());
        }
        w.0.push(0);
        assert!(decode_event(&w.0).is_err());
    }
}
