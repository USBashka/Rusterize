# Нативный API

Rusterize должен открывать возможности ОС, а не ограничивать приложение небольшим набором команд Canvas. Для совместимых операций есть общий Rust API. Для специфических возможностей доступны исходные объекты платформы. Это два дополняющих друг друга уровня; наличие низкоуровневого доступа не означает, что все SDK уже получили удобные переносимые обёртки.

## Текст и пагинация

```rust
use rusterize::*;

let style = TextStyle::new(24.0, Color::BLACK)
    .family(FontFamily::Serif)
    .italic();
let metrics = measure_text("Ёж идёт по лесу", &style)?;
let layout = TextLayout::new(
    "Первый абзац.\nВторой абзац с переносами по ширине.",
    style,
    TextOptions::wrap(480.0),
)?;
canvas.text_layout(&layout, (24.0, 24.0));
let end = layout.fitting_prefix(640.0);
let page_text = &layout.text()[..end];
# Ok::<(), rusterize::Error>(())
```

`measure_text` и `Canvas::measure_text` возвращают `Result<TextMetrics, Error>`. Метрики получены из системного движка, включая подбор отсутствующих глифов. Вызов не требует окна на Windows; C/JNI-оболочки устанавливают сервис до создания приложения. В ядре без сервиса измерение возвращает ошибку, а не приблизительные значения.

Нативные контракты описаны в документации [DirectWrite](https://learn.microsoft.com/en-us/windows/win32/api/dwrite/nn-dwrite-idwritetextlayout), [PangoLayout](https://docs.gtk.org/Pango/class.Layout.html), [Android StaticLayout](https://developer.android.com/reference/android/text/StaticLayout) и [AppKit NSLayoutManager](https://developer.apple.com/documentation/appkit/nslayoutmanager).

`TextOptions` задаёт ширину, `TextWrap::{None, Word, Character}`, физическое выравнивание `Left/Center/Right` и базовое направление `LeftToRight/RightToLeft`. Направление абзаца не отключает системный bidi-анализ. Явные переводы строк учитываются и при `None`. `Word` допускает аварийный разрыв длинного слова по правилам движка. Android StaticLayout пока не предоставляет в адаптере режим `Character`: такой запрос возвращает ошибку. Ширина Android layout округляется вверх до целого логического пикселя — этого требует StaticLayout.

`TextMetrics` содержит логическую ширину, ширину с завершающими пробелами, высоту и первую базовую линию. Это не границы закрашенных пикселей: курсив и диакритика могут выходить за логические границы. DirectWrite и Android возвращают отдельные ширины; текущие Pango/AppKit-адаптеры возвращают одну нативную логическую ширину в обоих полях. Для ограничения по ширине используй layout с нужной шириной, а не обрезку текста по метрике.

`TextLine` содержит диапазон **байтов UTF-8**, top, height и абсолютную baseline относительно начала layout. Диапазоны покрывают исходный текст, включая разделители строк; возможна последняя пустая строка. `fitting_prefix(height)` возвращает конец последней целиком помещающейся строки, 0 — если не помещается первая. Метод не выполняет повторную раскладку страницы: для повторного рендера части документа в новый прямоугольник нужно создать layout этой части с нужной шириной. Высота и ширина — логические пиксели, DPI повторно не умножается.

`TextStyle::font("Segoe UI")` выбирает установленное именованное семейство. Есть `bold`, `italic`, `underline`, `strikethrough`. Подмена отсутствующего семейства и глифов остаётся системной; произвольный шрифт не включается в дистрибутив автоматически. Храни `TextLayout` до изменения текста, стиля или ширины. Windows кэширует нативные layouts при рисовании; другие оболочки пока пересоздают layout, а не передают нативный ресурс между вызовами.

## Системные действия

```rust
use rusterize::{native::*, *};

fn event(&mut self, event: Event, context: &mut Context) {
    match event {
        Event::Key { key: Key::Enter, pressed: true, repeat: false, .. } => {
            context.file_dialog(FileDialog {
                title: "Открыть документ".into(),
                ..FileDialog::default()
            });
        }
        Event::NativeResult { id, result } => {
            // Сопоставь id с сохранённым RequestId и обработай result.
        }
        _ => {}
    }
}
```

Каждый метод возвращает `RequestId`. `Event::NativeResult` содержит `Result<NativeResponse, Error>`; ответ — `Done`, `Text`, `Paths` или `Cancelled`. Отмена диалога не является ошибкой. Выбор имени в Save-диалоге **не записывает файл**. Эти операции выполняются после выхода из обработчика приложения; вложенный цикл диалога может обрабатывать события окна. Не запускай модальные диалоги напрямую в `draw`, `native_draw` или `native_event`.

| Методы Context | Windows | GTK | AppKit | Android |
| --- | --- | --- | --- | --- |
| `read_clipboard`, `write_clipboard` | Unicode clipboard | GdkClipboard | NSPasteboard | ClipboardManager |
| `set_cursor` | Win32 | GDK | NSCursor | PointerIcon |
| `set_window_state` | Normal / Minimized / Maximized / Fullscreen | Те же состояния | Те же состояния | Ошибка Unsupported |
| `set_window_size` | Размер клиентской области | Запрос размера окна | Размер content view | Ошибка Unsupported |
| `set_window_position` | Координаты рабочего стола в физических пикселях | Ошибка: GTK4/Wayland не даёт общего API | Логические координаты, начало сверху основного экрана | Ошибка Unsupported |
| `set_always_on_top` | Win32 topmost | Ошибка: нет общего API GTK4/Wayland | Уровень floating | Ошибка Unsupported |
| `file_dialog` | IFileDialog: открыть, сохранить, выбрать каталог | GtkFileChooserNative | NSOpenPanel / NSSavePanel | Пока не подключён Activity Result API |
| `message_dialog` | MessageBox | GtkMessageDialog | NSAlert | AlertDialog |
| `open_uri` | ShellExecute | GAppInfo | NSWorkspace | ACTION_VIEW |

`native::capabilities().supports(NativeCapabilities::FILE_DIALOG)` проверяет адаптер до запроса. Возможность не гарантирует успех каждой операции: права, политика ОС, оконный менеджер или отсутствие обработчика URI могут привести к `Err`. Android `message_dialog` возвращает `Done` после показа; настольные модальные диалоги — после закрытия. Размер и состояние окна являются запросом ОС; окончательный размер приходит в `Event::Resize`. Общий API пока работает с одним главным окном.

## Прямые нативные расширения

`Application::native_draw` вызывается после display list с исходной системой логических координат. Его аргумент зависит от платформы:

| Платформа | Доступные объекты |
| --- | --- |
| Windows | `windows::NativeCanvas`: `ID2D1Factory`, `ID2D1RenderTarget`, `IDWriteFactory` |
| Android | `android::NativeCanvas`: `JNIEnv`, Java `Canvas` и `View`; через View доступен Context/Activity |
| Linux | `native::HostCanvas`: `cairo_t*`, `GtkWidget*`, `GtkWindow*` |
| macOS | `native::HostCanvas`: `CGContext`, `NSView`, `NSWindow` как указатели объектов |

Это реальные заимствованные системные объекты на UI-потоке. Не освобождай их и не используй после callback без собственного корректного нативного удержания. Offscreen-кадр может не иметь окна. Собственные save/restore, clips и layers нужно балансировать; `BeginDraw/EndDraw` и жизненный цикл окна принадлежат оболочке. Операции выполняются поверх общего кадра; вставка нативного callback между отдельными командами display list пока не реализована.

На Windows `windows::current_window()` возвращает HWND текущего callback, `Application::native_event(WindowMessage)` получает сообщения Win32. Верни `Some(LRESULT)` только для обработанного сообщения; `None` продолжает стандартную обработку. Создание, уничтожение и WM_PAINT зарезервированы оболочкой. Это позволяет подключать дополнительные системные сообщения, например собственный UI Automation provider; готового accessibility tree фреймворк пока не строит.

`windows::create_text_layout` возвращает настоящий `IDWriteTextLayout`: доступны cluster/overhang metrics, `HitTestPoint`, `HitTestTextPosition`, `HitTestTextRange`, форматирование диапазонов, typography и inline objects. **Индексы DirectWrite — UTF-16**, в отличие от общего `TextLine::range`. Для рисования такого изменённого layout используй `NativeCanvas.target.DrawTextLayout`.

`windows::api` реэкспортирует crate `windows` 0.62. Для дополнительных разделов Win32 можно включить нужные features той же зависимости в приложении. GTK/AppKit вызываются через выбранные приложением системные FFI bindings; Java — через реэкспортированный `rusterize::jni`. Поддержка конкретного нативного интерфейса зависит от версии ОС и типа render target.

## Что ещё не обёрнуто

Готового переносимого API для меню, tray, нескольких окон, уведомлений, drag-and-drop, IME/composition, экранной клавиатуры, accessibility semantics, печати, сенсоров, камеры и произвольных системных разрешений пока нет. Полноценные text hit testing/rich text в общем API тоже ещё не реализованы; DirectWrite доступен напрямую. Общий Canvas пока не получил все варианты blend modes, слоёв, gradient stops, dash/cap/join, фильтров и геометрических операций. Их можно использовать через нативный callback, но это платформенный код.

Дальнейшее расширение должно добавлять реальные модули с capabilities и проверками на соответствующей ОС. Полный реестр каждого метода Win32/Android/GTK/AppKit не заменяется небольшим списком обёрток или заявлением о полном покрытии.

При обновлении старых приложений обнови скопированные оболочки: wire-протокол изменён с RZ01 на RZ02. В TextStyle добавлены поля; для устойчивых инициализаций используй `TextStyle::new` и методы настройки.

## Пример и проверка

```powershell
cargo run -p rusterize-native-api --bin native-api
python scripts/build.py --platform windows --package rusterize-native-api
python scripts/build.py --platform android --package rusterize-native-api --abi x86_64
```

`examples/native-api` показывает настоящую раскладку Unicode, чтение clipboard, выбор файлов и fullscreen. Недоступные действия отключаются по capabilities. При создании layout пример проверяет пропорциональные метрики, пустые строки, emoji, комбинируемые символы и границы UTF-8. `native-api.exe target/native-api.bgra` сохраняет Windows-кадр 800×600 без окна. Точные результаты сборки и исполнения — в [VALIDATION.md](VALIDATION.md).
