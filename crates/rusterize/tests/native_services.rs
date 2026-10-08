use rusterize::{native::*, *};
use std::cell::{Cell, RefCell};

thread_local! {
    static RESULTS: RefCell<Vec<(RequestId,Result<NativeResponse,Error>)>> = const {RefCell::new(Vec::new())};
    static HANDLE: Cell<u64> = const {Cell::new(0)};
}
#[derive(Default)]
struct App;
impl Application for App {
    fn event(&mut self, event: Event, context: &mut Context) {
        match event {
            Event::Resume => {
                context.read_clipboard();
                context.file_dialog(FileDialog::default());
                context.set_always_on_top(true);
            }
            Event::NativeResult { id, result } => {
                RESULTS.with(|r| r.borrow_mut().push((id, result)))
            }
            _ => {}
        }
    }
    fn draw(&mut self, _: &mut Canvas<'_>, _: Viewport) {}
}
rusterize::export_app!(App);

#[test]
fn host_requests_release_application_borrows_before_calling_native_code() {
    set_service(|bytes| {
        let id = HANDLE.with(Cell::get);
        assert_ne!(rusterize_status(id), host::FAILED);
        // A modal loop can reenter the host; nested polling must not execute twice.
        assert_ne!(rusterize_poll_native(id), host::FAILED);
        match u32::from_le_bytes(bytes[..4].try_into().unwrap()) {
            3 => Ok("Ёж 👩‍💻".as_bytes().to_vec()),
            10 => Ok(0u32.to_le_bytes().to_vec()),
            _ => Err(Error("unsupported".into())),
        }
    });
    let id = rusterize_create();
    HANDLE.with(|h| h.set(id));
    rusterize_poll_native(id);
    let result = RESULTS.with(|r| r.take());
    assert_eq!(
        result,
        vec![
            (RequestId(1), Ok(NativeResponse::Text("Ёж 👩‍💻".into()))),
            (RequestId(2), Ok(NativeResponse::Cancelled)),
            (RequestId(3), Err(Error("unsupported".into()))),
        ]
    );
    rusterize_poll_native(id);
    assert!(RESULTS.with(|r| r.borrow().is_empty()));
    rusterize_destroy(id);
    clear_service();
}

#[test]
fn invalid_native_arguments_never_reach_the_backend() {
    set_service(|_| panic!("invalid argument reached the native backend"));
    for request in [
        NativeRequest::SetWindowSize(Size::new(f32::INFINITY, 1.0)),
        NativeRequest::WriteClipboard("bad\0text".into()),
        NativeRequest::OpenUri("https://example.com\n".into()),
        NativeRequest::FileDialog(FileDialog {
            save: true,
            directory: true,
            ..Default::default()
        }),
    ] {
        assert!(execute_host_request(&request).is_err());
    }
    assert!(measure_text("bad\0text", &TextStyle::new(20.0, Color::BLACK)).is_err());
    clear_service();
}

#[test]
fn malformed_native_metrics_are_rejected() {
    set_service(|_| Ok(vec![0; 3]));
    assert!(measure_text("Ё", &TextStyle::new(20.0, Color::BLACK)).is_err());
    set_service(|_| {
        let mut bytes = Vec::new();
        for f in [12.0f32, 12.0, 24.0, 18.0] {
            bytes.extend(f.to_le_bytes());
        }
        for n in [1u32, 0, 1] {
            bytes.extend(n.to_le_bytes());
        } // Splits Ё's UTF-8 encoding.
        for f in [0.0f32, 24.0, 18.0] {
            bytes.extend(f.to_le_bytes());
        }
        Ok(bytes)
    });
    assert!(measure_text("Ё", &TextStyle::new(20.0, Color::BLACK)).is_err());
    clear_service();
}
