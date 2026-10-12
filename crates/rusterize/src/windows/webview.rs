use super::*;
use std::{collections::VecDeque, rc::Rc};
use webview2_com::{
    CoTaskMemPWSTR, CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, ExecuteScriptCompletedHandler,
    Microsoft::Web::WebView2::Win32::*, NavigationCompletedEventHandler,
    NavigationStartingEventHandler, NewWindowRequestedEventHandler,
};

pub(super) const WAKE: u32 = WM_APP + 0x52;
struct View {
    owner: HWND,
    id: u32,
    bounds: Cell<Rect>,
    visible: Cell<bool>,
    javascript: bool,
    html_pending: Cell<bool>,
    html_uris: RefCell<std::collections::HashSet<String>>,
    controller: RefCell<Option<ICoreWebView2Controller>>,
}
impl Drop for View {
    fn drop(&mut self) {
        if let Some(c) = self.controller.get_mut().take() {
            unsafe {
                let _ = c.Close();
            }
        }
    }
}
thread_local! {
    static VIEWS: RefCell<HashMap<(isize,u32),Rc<View>>> = RefCell::new(HashMap::new());
    static EVENTS: RefCell<VecDeque<(isize,u32,Vec<u8>)>> = const { RefCell::new(VecDeque::new()) };
}
fn emit(view: &View, kind: u32, token: u32, value: &str) {
    let mut w = crate::protocol::Writer(vec![]);
    w.u32(kind);
    w.u32(view.id);
    w.u32(token);
    w.bytes(value.as_bytes());
    EVENTS.with(|q| {
        q.borrow_mut()
            .push_back((view.owner.0 as isize, view.id, w.0))
    });
    unsafe {
        let _ = PostMessageW(Some(view.owner), WAKE, WPARAM(0), LPARAM(0));
    }
}
fn layout(view: &View, controller: &ICoreWebView2Controller) -> ::windows::core::Result<()> {
    let r = view.bounds.get();
    unsafe {
        let scale = GetDpiForWindow(view.owner).max(96) as f32 / 96.0;
        controller.SetBounds(RECT {
            left: (r.x * scale).round() as i32,
            top: (r.y * scale).round() as i32,
            right: ((r.x + r.width) * scale).round() as i32,
            bottom: ((r.y + r.height) * scale).round() as i32,
        })?;
        controller.SetIsVisible(view.visible.get())
    }
}
pub(super) fn update_bounds(owner: HWND) {
    let views: Vec<_> = VIEWS.with(|v| {
        v.borrow()
            .values()
            .filter(|v| v.owner == owner)
            .cloned()
            .collect()
    });
    for view in views {
        let controller = view.controller.borrow().clone();
        if let Some(c) = controller {
            if let Err(e) = layout(&view, &c) {
                emit(&view, 3, 0, &e.to_string());
            }
        }
    }
}
pub(super) fn forget(owner: HWND) {
    let removed: Vec<_> = VIEWS.with(|v| {
        let mut v = v.borrow_mut();
        let keys: Vec<_> = v
            .keys()
            .filter(|k| k.0 == owner.0 as isize)
            .copied()
            .collect();
        keys.into_iter().filter_map(|k| v.remove(&k)).collect()
    });
    drop(removed);
    EVENTS.with(|q| q.borrow_mut().retain(|e| e.0 != owner.0 as isize));
}
fn read_bounds(r: &mut native::Reader<'_>) -> Result<Rect, Error> {
    Ok(Rect::new(r.f32()?, r.f32()?, r.f32()?, r.f32()?))
}
unsafe fn initialize(
    view: &Rc<View>,
    controller: ICoreWebView2Controller,
) -> ::windows::core::Result<()> {
    // Store first so every subsequent failure is cleaned up by Destroy/window close.
    *view.controller.borrow_mut() = Some(controller.clone());
    layout(view, &controller)?;
    let web = controller.CoreWebView2()?;
    let settings = web.Settings()?;
    settings.SetIsScriptEnabled(view.javascript)?;
    settings.SetIsWebMessageEnabled(false)?;
    settings.SetAreDevToolsEnabled(false)?;
    let weak = Rc::downgrade(view);
    let mut token = 0;
    web.add_NavigationStarting(
        &NavigationStartingEventHandler::create(Box::new(move |_, args| {
            if let (Some(view), Some(args)) = (weak.upgrade(), args) {
                let mut uri = ::windows::core::PWSTR::null();
                args.Uri(&mut uri)?;
                let uri = CoTaskMemPWSTR::from(uri).to_string();
                // NavigateToString raises NavigationStarting with an internal data URI,
                // although the committed document's Source is about:blank.
                if view.html_pending.replace(false)
                    && uri.starts_with("data:text/html;charset=utf-8;base64,")
                {
                    view.html_uris.borrow_mut().insert(uri.clone());
                }
                if !crate::webview::valid_url(&uri) && !view.html_uris.borrow().contains(&uri) {
                    args.SetCancel(true)?;
                    emit(
                        &view,
                        3,
                        0,
                        "Only HTTP(S), about:blank and host-loaded HTML navigation is allowed",
                    );
                }
            }
            Ok(())
        })),
        &mut token,
    )?;
    let weak = Rc::downgrade(view);
    web.add_NavigationCompleted(
        &NavigationCompletedEventHandler::create(Box::new(move |sender, args| {
            if let (Some(view), Some(sender), Some(args)) = (weak.upgrade(), sender, args) {
                let mut success = ::windows::core::BOOL(0);
                args.IsSuccess(&mut success)?;
                if success.as_bool() {
                    let mut uri = ::windows::core::PWSTR::null();
                    sender.Source(&mut uri)?;
                    emit(&view, 2, 0, &CoTaskMemPWSTR::from(uri).to_string());
                } else {
                    let mut status = COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN;
                    args.WebErrorStatus(&mut status)?;
                    emit(
                        &view,
                        3,
                        0,
                        &format!("WebView navigation failed: {}", status.0),
                    );
                }
            }
            Ok(())
        })),
        &mut token,
    )?;
    web.add_NewWindowRequested(
        &NewWindowRequestedEventHandler::create(Box::new(|_, args| {
            if let Some(args) = args {
                args.SetHandled(true)?;
            }
            Ok(())
        })),
        &mut token,
    )?;
    emit(view, 1, 0, "");
    Ok(())
}
fn create(view: &Rc<View>) -> Result<(), Error> {
    let local =
        std::env::var_os("LOCALAPPDATA").ok_or_else(|| Error("LOCALAPPDATA is not set".into()))?;
    let executable = std::env::current_exe().map_err(|e| Error(e.to_string()))?;
    let name = executable.file_stem().unwrap_or_default();
    let profile = std::path::PathBuf::from(local)
        .join("Rusterize")
        .join(name)
        .join("WebView2");
    let profile = ::windows::core::HSTRING::from(profile.as_os_str());
    let weak = Rc::downgrade(view);
    unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            &profile,
            None,
            &CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
                move |result, environment| {
                    let Some(view) = weak.upgrade() else {
                        return Ok(());
                    };
                    let result = (|| -> ::windows::core::Result<()> {
                        result?;
                        let environment =
                            environment.ok_or_else(|| ::windows::core::Error::from(E_POINTER))?;
                        let weak = Rc::downgrade(&view);
                        environment.CreateCoreWebView2Controller(
                            view.owner,
                            &CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
                                move |result, controller| {
                                    let Some(view) = weak.upgrade() else {
                                        if let Some(controller) = controller {
                                            let _ = controller.Close();
                                        }
                                        return Ok(());
                                    };
                                    let result = result.and_then(|_| {
                                        let controller = controller.ok_or_else(|| {
                                            ::windows::core::Error::from(E_POINTER)
                                        })?;
                                        initialize(&view, controller)
                                    });
                                    if let Err(e) = result {
                                        emit(
                                            &view,
                                            3,
                                            0,
                                            &format!("WebView2 initialization failed: {e}"),
                                        );
                                    }
                                    Ok(())
                                },
                            )),
                        )
                    })();
                    if let Err(e) = result {
                        emit(
                            &view,
                            3,
                            0,
                            &format!("WebView2 Runtime is unavailable or could not start: {e}"),
                        );
                    }
                    Ok(())
                },
            )),
        )?;
    }
    Ok(())
}
pub(super) fn query(owner: HWND, op: u32, r: &mut native::Reader<'_>) -> Result<Vec<u8>, Error> {
    if op == 21 {
        return Ok(EVENTS.with(|q| {
            let mut q = q.borrow_mut();
            q.iter()
                .position(|e| e.0 == owner.0 as isize)
                .and_then(|i| q.remove(i))
                .map(|e| e.2)
                .unwrap_or_else(|| 0u32.to_le_bytes().to_vec())
        }));
    }
    let id = r.u32()?;
    let action = r.u32()?;
    let key = (owner.0 as isize, id);
    if action == 1 {
        if VIEWS.with(|v| v.borrow().contains_key(&key)) {
            return Err(Error("WebView ID already exists".into()));
        }
        let view = Rc::new(View {
            owner,
            id,
            bounds: Cell::new(read_bounds(r)?),
            visible: Cell::new(r.u32()? != 0),
            javascript: r.u32()? != 0,
            html_pending: Cell::new(false),
            html_uris: RefCell::new(std::collections::HashSet::new()),
            controller: RefCell::new(None),
        });
        VIEWS.with(|v| v.borrow_mut().insert(key, view.clone()));
        if let Err(e) = create(&view) {
            VIEWS.with(|v| v.borrow_mut().remove(&key));
            return Err(Error(format!("Cannot start WebView2 Runtime: {e}")));
        }
        return Ok(vec![]);
    }
    if action == 2 {
        let removed = VIEWS.with(|v| v.borrow_mut().remove(&key));
        if removed.is_none() {
            return Err(Error("unknown WebView ID".into()));
        }
        drop(removed);
        EVENTS.with(|q| q.borrow_mut().retain(|e| (e.0, e.1) != key));
        return Ok(vec![]);
    }
    let view = VIEWS
        .with(|v| v.borrow().get(&key).cloned())
        .ok_or_else(|| Error("unknown WebView ID".into()))?;
    let controller = view.controller.borrow().clone();
    // Geometry and visibility can be updated while the asynchronous creation runs.
    if action == 3 {
        view.bounds.set(read_bounds(r)?);
        if let Some(c) = controller {
            layout(&view, &c)?;
        }
        return Ok(vec![]);
    }
    if action == 4 {
        view.visible.set(r.u32()? != 0);
        if let Some(c) = controller {
            layout(&view, &c)?;
        }
        return Ok(vec![]);
    }
    let controller = controller.ok_or_else(|| Error("WebView is not ready".into()))?;
    unsafe {
        let web = controller.CoreWebView2()?;
        match action {
            5 => web.Navigate(&::windows::core::HSTRING::from(r.string()?))?,
            6 => {
                let html = ::windows::core::HSTRING::from(r.string()?);
                view.html_pending.set(true);
                if let Err(error) = web.NavigateToString(&html) {
                    view.html_pending.set(false);
                    return Err(error.into());
                }
            }
            7 => {
                if !view.javascript {
                    return Err(Error("JavaScript is disabled".into()));
                }
                let token = r.u32()?;
                let script = r.string()?;
                let weak = Rc::downgrade(&view);
                web.ExecuteScript(
                    &::windows::core::HSTRING::from(script),
                    &ExecuteScriptCompletedHandler::create(Box::new(move |result, json| {
                        if let Some(view) = weak.upgrade() {
                            match result {
                                Ok(()) => emit(&view, 4, token, &json),
                                Err(e) => emit(&view, 5, token, &e.to_string()),
                            }
                        }
                        Ok(())
                    })),
                )?;
            }
            8 => controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC)?,
            9 => web.Reload()?,
            10 => web.GoBack()?,
            11 => web.GoForward()?,
            _ => return Err(Error("unknown WebView command".into())),
        }
    }
    Ok(vec![])
}
