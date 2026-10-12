# Нативный WebView

Feature `webview` добавляет дочерние системные WebView поверх Canvas. Движок браузера не включается в приложение. Без этой feature зависимости WebView2/WebKit не подключаются.

```toml
[dependencies]
rusterize = { path = "../Rusterize/crates/rusterize", features = ["webview"] }
```

| Платформа | Компонент | Зависимость при запуске |
| --- | --- | --- |
| Windows | WebView2, статически связанный загрузчик | Установленный Microsoft Edge WebView2 Runtime |
| macOS | WKWebView | Системный WebKit |
| Android | android.webkit.WebView | Системный провайдер WebView |
| Linux | WebKitGTK 6.0, GTK 4 | WebKitGTK 6.0; для сборки Ubuntu — `libwebkitgtk-6.0-dev` |
| Веб | sandboxed iframe | Браузер; ограничения внешних сайтов на встраивание |

Веб-адаптер использует тот же Rust API. `Evaluate` работает только в LoadHtml/about:blank; внешние страницы изолированы. В браузере Promise ожидается, история включает только навигации из Rust, событие Loaded не гарантирует успешный HTTP-ответ. Полный контракт и ограничения: [WEB.md](WEB.md).

Сборщик `scripts/build.py` определяет feature из графа Cargo, выбирает оболочку и добавляет нужные библиотеки. В Android только для WebView-сборки добавляется разрешение `INTERNET`; запросов разрешений во время запуска оно не требует. HTTPS работает со стандартной политикой ОС. HTTP может блокироваться Android или политикой App Transport Security macOS; глобальное отключение этих проверок не производится.

## API

```rust
use rusterize::{webview::*, *};

const BROWSER: WebViewId = WebViewId(1);

fn handle(event: Event, context: &mut Context) {
    match event {
        Event::Resume => {
            // В приложении сохраняйте признак создания: Resume бывает повторным.
            context.webview(BROWSER, WebViewCommand::Create(
                WebViewOptions::new(Rect::new(20.0, 60.0, 600.0, 400.0))
                    .with_javascript(true),
            ));
        }
        Event::WebView { id: BROWSER, event: WebViewEvent::Ready } => {
            context.webview(BROWSER, WebViewCommand::LoadHtml(
                "<!doctype html><meta charset=utf-8><h1>Привет!</h1>".into(),
            ));
        }
        Event::Resize(viewport) => {
            context.webview(BROWSER, WebViewCommand::SetBounds(Rect::new(
                20.0, 60.0, (viewport.size.width - 40.0).max(1.0),
                (viewport.size.height - 80.0).max(1.0),
            )));
        }
        _ => {}
    }
}
```

`Context::webview` возвращает `RequestId`; `Event::NativeResult` сообщает, принят ли запрос. Создание асинхронное: перед загрузкой документа и выполнением JavaScript дождитесь `Ready`. Ошибки создания и основной навигации приходят как `WebViewEvent::Error`. Для повторного создания после ошибки сначала вызовите `Destroy`. Обрабатывайте ошибки обоих типов.

- `Create(WebViewOptions)`, `Destroy`: жизненный цикл. ID ненулевой, уникальный среди живых представлений окна; после `Destroy` его можно использовать снова. Закрытие окна освобождает представления, запоздалые callbacks уничтоженного экземпляра игнорируются.
- `SetBounds(Rect)`: положение относительно клиентской области, в логических пикселях. Размеры должны быть от 1 до 32768. Подстройку под размер окна задаёт приложение через `Event::Resize`; Windows автоматически пересчитывает физические границы при смене DPI.
- `SetVisible(bool)`, `Focus`: видимость и клавиатурный фокус.
- `Navigate(String)`, `LoadHtml(String)`, `Reload`, `GoBack`, `GoForward`: загрузка и история. Успешная загрузка основного документа сообщает `Loaded { url }`.
- `Evaluate { script, token }`: исполнение JavaScript и `ScriptResult { token, result }`. Значение возвращается JSON-строкой, строковое JS-значение включает JSON-кавычки. `undefined` нормализуется в `null`. WebView2/Android также могут вернуть `null` при JS-исключении или несериализуемом значении; для различения оборачивайте код в `try/catch` и возвращайте объект с результатом/ошибкой. Асинхронные Promise автоматически не ожидаются.

`native::capabilities().supports(NativeCapabilities::WEBVIEW)` означает наличие адаптера в оболочке. Это не проверка установки системного runtime: ошибка запуска возвращается при создании.

## Границы интеграции

WebView — нативный дочерний элемент над Canvas. Его не включают display list, offscreen-снимки Canvas, `Transform` и `Clip`. Перекрывающиеся WebView располагаются в порядке создания. Ввод внутри WebView обрабатывается браузером. Для возврата фокуса Canvas щёлкните по его области; автоматический Tab-переход между обоими типами элементов пока не реализован.

JavaScript по умолчанию отключён. Встроенного доступа JavaScript к Rust, файловой системе или системным командам нет. Для навигации разрешены HTTP(S) и `about:blank`; загрузка локального документа предусмотрена через `LoadHtml`. Строки ограничены 1 МиБ и не могут содержать NUL. Дополнительные окна не создаются. Пользовательские схемы, загрузки файлов, разрешения камеры/микрофона, файловые диалоги HTML и двусторонний IPC не входят в этот API.

Windows хранит профиль в `%LOCALAPPDATA%/Rusterize/<имя-exe>/WebView2`. Приложения с одинаковым именем EXE используют один профиль. На остальных платформах используется стандартное хранилище системного WebView. Приватный профиль и выбор каталога пока не предоставляются.

## Пример и проверка

```sh
python scripts/build.py --platform windows --package rusterize-webview
python scripts/build.py --platform android --package rusterize-webview
python3 scripts/build.py --platform linux --package rusterize-webview
python3 scripts/build.py --platform macos --package rusterize-webview
```

Windows: `dist/webview.exe`. Пример содержит интерактивную HTML-кнопку и поле ввода внутри нативного окна. Режим `--smoke` выполняет проверку DOM/JS, навигации, смены геометрии/видимости, повторного создания и отмены незавершённого создания. Он завершает процесс с ошибкой при сбое или тайм-ауте. Linux/macOS используют этот же Rust-сценарий через свои оболочки. На Android пример запускается интерактивно.

Для автоматической проверки Android соберите отдельный APK с `--features self-check --abi x86_64 --out target/webview-check`, затем запустите `python scripts/check_android_webview.py --serial emulator-XXXX --apk target/webview-check/rusterize-webview.apk` (путь к adb можно задать через `--adb`). Сценарий допускает только явно выбранный эмулятор, устанавливает APK и ждёт маркер успешного завершения в logcat.

## Размер

Пакет содержит только адаптер и загрузчик, а системный runtime поставляется отдельно. Microsoft указывает размер Evergreen Bootstrapper около 2 МБ, а поставка Fixed Version Runtime добавляет более 250 МБ: [документация распространения WebView2](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution). Сборщик не скачивает и не устанавливает runtime автоматически. Небольшой EXE не означает небольшое потребление RAM: браузер создаёт отдельные процессы. Фактические замеры и проверенные платформы указаны в [VALIDATION.md](VALIDATION.md).
