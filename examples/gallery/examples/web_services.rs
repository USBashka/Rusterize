//! Real WASM-to-browser asynchronous service integration fixture.
#[cfg(target_arch = "wasm32")]
mod browser {
    use rusterize::{native::*, *};
    use std::{cell::RefCell, collections::BTreeMap, path::PathBuf};
    const CONTENT: &str = "Ёж идёт по лесу 🦔";
    thread_local! { static METRICS: RefCell<[u32;16]> = const { RefCell::new([0;16]) }; }
    fn record(index: usize) {
        METRICS.with(|m| m.borrow_mut()[index] += 1);
    }
    #[no_mangle]
    pub extern "C" fn web_service_metric(index: u32) -> u32 {
        METRICS.with(|m| m.borrow()[index as usize])
    }
    #[derive(Default)]
    pub struct App {
        pending: BTreeMap<u64, u32>,
        file: Option<PathBuf>,
    }
    impl Application for App {
        fn event(&mut self, event: Event, c: &mut Context) {
            match event {
                Event::Key {
                    key: Key::Unknown(key),
                    pressed: true,
                    ..
                } => {
                    let request = match key {
                        49 => c.read_clipboard(),
                        50 => c.write_clipboard(CONTENT),
                        51 => c.message_dialog("Проверка", CONTENT),
                        52..=54 => c.file_dialog(FileDialog {
                            save: key == 53,
                            directory: key == 54,
                            suggested_name: "ёж.txt".into(),
                            ..Default::default()
                        }),
                        55 => c.set_window_state(WindowState::Fullscreen),
                        56 => c.set_window_state(WindowState::Normal),
                        57 => c.open_uri("javascript:alert(1)"),
                        48 => web::release_file(c, self.file.clone().unwrap()),
                        _ => return,
                    };
                    self.pending.insert(request.0, key - 48);
                }
                Event::NativeResult { id, result } => {
                    let action = self.pending.remove(&id.0).unwrap();
                    let follow = match (action, result) {
                        (1, Ok(NativeResponse::Text(text))) => {
                            assert_eq!(text, CONTENT);
                            record(1);
                            None
                        }
                        (4, Ok(NativeResponse::Paths(paths))) => {
                            self.file = Some(paths[0].clone());
                            Some((web::read_file(c, paths[0].clone()), 10))
                        }
                        (5, Ok(NativeResponse::Paths(paths))) => Some((
                            web::write_file(c, paths[0].clone(), CONTENT.as_bytes().to_vec()),
                            5,
                        )),
                        (6, Ok(NativeResponse::Paths(paths))) => {
                            Some((web::list_directory(c, paths[0].clone()), 11))
                        }
                        (10, Ok(NativeResponse::Bytes(bytes))) => {
                            assert_eq!(bytes, CONTENT.as_bytes());
                            record(4);
                            None
                        }
                        (11, Ok(NativeResponse::Paths(paths))) => {
                            record(if paths.is_empty() { 14 } else { 6 });
                            None
                        }
                        (0, Ok(NativeResponse::Done)) => {
                            Some((web::read_file(c, self.file.clone().unwrap()), 12))
                        }
                        (9 | 12, Err(_)) => {
                            record(action as usize);
                            None
                        }
                        (_, Ok(NativeResponse::Done)) => {
                            record(action as usize);
                            None
                        }
                        (_, Ok(NativeResponse::Cancelled)) => {
                            record(13);
                            None
                        }
                        (action, result) => panic!("Unexpected action {action} result: {result:?}"),
                    };
                    if let Some((id, action)) = follow {
                        self.pending.insert(id.0, action);
                    }
                }
                _ => {}
            }
        }
        fn draw(&mut self, c: &mut Canvas<'_>, _: Viewport) {
            c.clear(Color::WHITE);
        }
    }
    rusterize::export_app!(App);
}
