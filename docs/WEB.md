# Веб: WebAssembly и Canvas 2D

Общий `Application` и `export_app!(App)` работают в браузере. Rust компилируется в `wasm32-unknown-unknown`; JavaScript-оболочка исполняет RZ02 через Canvas 2D. npm, wasm-bindgen, WebView и отдельный графический движок для сборки не нужны. Требуется современный браузер с WebAssembly, BigInt, Canvas 2D `roundRect`, Pointer Events, ResizeObserver и `Intl.Segmenter`.

```sh
rustup target add wasm32-unknown-unknown
python scripts/build.py --platform web
python -m http.server 8000 --bind 127.0.0.1 --directory dist/web/rusterize-gallery
```

Открой `http://127.0.0.1:8000`. Можно указать `--package rusterize-native-api` или пакет своего приложения. Генератор `cargo run -p rusterize-cli -- new <directory>` уже включает веб-оболочку. Библиотека приложения должна иметь `crate-type = ["cdylib", ...]`. `main.rs` для браузера не используется.

Результат — `dist/web/<package>/index.html`, `app.wasm` и JavaScript-модули оболочки. Размещай **всю папку** на статическом HTTP(S)-хостинге в любой подпапке. Открытие через `file://` не поддерживается. Сборщик ничего не публикует и не запускает сервер автоматически.

## Рисование и текст

Поддержаны все команды RZ02: очистка, save/restore, преобразования, отсечение, прямоугольники, скругления, эллипсы, кубические пути, заливки, обводки, двухцветные градиенты, RGBA-изображения, однострочный текст и `TextLayout`. Изображения кэшируются по ID и удаляются из кэша, когда исчезают из кадра. Неизвестная команда, повреждённый буфер или ошибка Rust останавливает экземпляр и передаётся `onError`.

`measure_text` и `TextLayout::new` синхронно обращаются к Canvas `measureText`. Измерение и рисование используют одну раскладку. Переносы Word/Character сохраняют графемы (включая составные emoji), UTF-8 диапазоны и переводы строк CRLF/LF/CR. Слишком длинное слово переносится по графемам; одна графема может выходить за заданную ширину. Canvas выполняет shaping и bidi каждой строки. Выравнивание Left/Center/Right физическое. Подчёркивание и зачёркивание рисуются оболочкой.

Это не полная реализация Unicode Line Breaking Algorithm или типографики DirectWrite/TextKit: словарная сегментация, высоты строк и bidi на границах переноса могут отличаться. Глифы, сглаживание и метрики зависят от браузера и ОС. Пользовательские веб-шрифты следует загрузить через `document.fonts.load()` **до** `start`; не заменяй их во время жизни закэшированных Rust `TextLayout`.

## Ввод и жизненный цикл

- Мышь имеет ID 0; перо и касания — ID браузерного PointerEvent. Захват указателя поддерживает перетаскивание за пределами canvas. Потеря захвата, фокуса и скрытие вкладки отменяют активные контакты.
- Передаются клавиши, модификаторы, repeat, обычный символьный ввод и колесо. Tab и стрелки внутри сфокусированного canvas отдаются приложению. IME/preedit, экранная клавиатура и accessibility tree пока отсутствуют, как и в общем API версии 0.1.
- Размер — CSS-пиксели; backing store учитывает `devicePixelRatio`. ResizeObserver отслеживает размер встраиваемого элемента, media query — смену DPI.
- `requestAnimationFrame` работает только при изменении или включённой анимации. В простое нет постоянного опроса. Скрытие вкладки вызывает Suspend, возвращение — Resume. `Context::exit()` освобождает экземпляр и оставляет последний кадр, не закрывая вкладку.
- `Context::set_title` меняет заголовок страницы. Размер, минимальный размер и оформление нативного окна в браузере определяются встраивающей страницей, а не `WindowOptions`.

## Системные возможности

`Context` сохраняет общий API. Запросы выполняются асинхронно, результат приходит через `Event::NativeResult` с исходным `RequestId`. Очередь ограничена 1024 незавершёнными запросами; ожидание разрешений не блокирует анимацию. После `destroy()` ответы не доставляются.

| Возможность | Браузерная реализация |
| --- | --- |
| TEXT_LAYOUT, CURSOR | Canvas `measureText`, CSS-курсор |
| CLIPBOARD | `navigator.clipboard.readText/writeText`, когда API доступен |
| WINDOW_STATE | Fullscreen и Normal; полноэкранный режим всей страницы с Canvas и WebView |
| FILE_DIALOG | Выбор файла/каталога, сохранение; браузерные токены вместо системных путей |
| MESSAGE_DIALOG | Модальный HTML `dialog`, закрытие кнопкой или Escape |
| OPEN_URI | HTTP(S), mailto, tel; новая вкладка без доступа к opener |
| WEBVIEW | Изолированный iframe поверх Canvas |

Capability означает наличие API, а не разрешение пользователя. Запускай clipboard, picker, fullscreen и открытие URI из обработчика пользовательского действия. Запрос передаётся браузеру в том же стеке событий; результат приходит позже. HTTPS/localhost, разрешения, политика встраивания и блокировка popup определяются браузером. Отказ возвращается как `Err`; отмена выбора файла — `NativeResponse::Cancelled`. Размер/позиция нативного окна, always-on-top, minimize/maximize недоступны и возвращают явную ошибку.

Файловый диалог возвращает `NativeResponse::Paths`, содержащий непрозрачные `rusterize-file:…` токены. В WASM используй `rusterize::web::read_file`, `write_file`, `list_directory`, `release_file`. Эти функции принимают `&mut Context` и токен, возвращают `RequestId`. Чтение возвращает `NativeResponse::Bytes(Vec<u8>)`, список прямых детей — `Paths` (включая пустой список), запись/освобождение — `Done`. Токены действуют только до освобождения или закрытия экземпляра; токен каталога освобождает и выданные через него дочерние токены. Предел — 4096 удерживаемых токенов, 1024 элемента списка и 32 МиБ на чтение/запись.

При наличии File System Access используются браузерные handles. В остальных браузерах открытие идёт через `<input type=file>`, каталог — через `webkitdirectory`; сохранение спрашивает имя и скачивает файл при `write_file`. Файлы, выбранные через input, доступны только для чтения. В fallback-каталоге видны только выбранные файлы и содержащие их папки; пустые папки браузер не передаёт. Системный путь, произвольный доступ к диску и автоматическое сохранение разрешений не предоставляются.

## WGSL и WebView

```sh
python scripts/build.py --platform web --package rusterize-shaders
python scripts/build.py --platform web --package rusterize-webview
```

Feature `shaders` использует прежние `ShaderRenderer`, `ShaderParams`, `Canvas::shader` и `Image`. Naga переводит WGSL в GLSL ES 300 при сборке, браузер исполняет его через WebGL 2. Проверяются размеры и параметры; сохраняются программы, текстура и последний результат. RGBA8 с straight alpha и координатами сверху вниз возвращается через readback в обычный Canvas. Это синхронное GPU → CPU копирование; WebGPU и zero-copy здесь не используются. При недоступном WebGL 2 возвращается ошибка. На экземпляр разрешено до восьми renderers; остальные ограничения совпадают с [SHADERS.md](SHADERS.md).

Feature `webview` поддерживает Create/Destroy, LoadHtml/Navigate, bounds/visibility/focus, Reload/GoBack/GoForward и Evaluate. Iframe ограничен прямоугольником Canvas; его содержимое не входит в Canvas-снимок, transform или clip. JavaScript выключен по умолчанию. При включении применяется только `sandbox="allow-scripts"`, без `allow-same-origin`; дочерний документ не получает доступ к DOM родителя или системным сервисам приложения. `Evaluate` доступен только для LoadHtml и about:blank, через непривилегированный postMessage-мост. Результат сериализуется в JSON; Promise ожидается, исключения и тайм-аут 10 секунд возвращаются в ScriptResult. Предел — 32 WebView, 64 незавершённых скрипта на представление, 1 МиБ на результат.

Внешние HTTP(S)-страницы могут запрещать iframe через CSP/X-Frame-Options. Доступ к их DOM и Evaluate запрещён, браузер также не позволяет надёжно определить отказ загрузки. `Loaded` отражает событие iframe, а не проверку HTTP-статуса; его URL — адрес, запрошенный приложением. История содержит последние 64 навигации, выполненные через Rust API; внутренние переходы страницы браузер не раскрывает. Для about:blank с мостом `document.URL` равен `about:srcdoc`. Навигация отменяет незавершённые Evaluate с ошибкой, Destroy удаляет все старые события перед повторным использованием ID.

Контракты браузера: [Clipboard API](https://developer.mozilla.org/en-US/docs/Web/API/Clipboard_API), [выбор файлов](https://developer.mozilla.org/en-US/docs/Web/API/Window/showOpenFilePicker), [sandbox iframe](https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/iframe).

Код приложения и его зависимости тоже должны поддерживать браузерный WASM. `std::fs`, системные сокеты и потоки ОС не превращаются в браузерные API автоматически. Доступ к хранилищу и сети требует отдельной интеграции с браузером. `native_draw` для браузера пока не предоставлен.

## Встраивание

```html
<canvas id="app" style="width:100%;height:600px" tabindex="0"></canvas>
<script type="module">
  import { start } from './rusterize.js';
  const app = await start(document.getElementById('app'), './app.wasm', {
    onError: error => console.error(error),
    onExit: () => console.log('Приложение завершено'),
  });
  // При удалении компонента со страницы: app.destroy().
</script>
```

Каждый `start` создаёт отдельный WASM instance. Не запускай два экземпляра на одном canvas. Оболочка эксклюзивно владеет его 2D-контекстом. `destroy()` идемпотентен, удаляет обработчики/observer/RAF и Rust handle. `requestRedraw()` запрашивает кадр; `frameCount` и `stopped` доступны для диагностики.

Для интеграции с HTML-оболочкой приложение может экспортировать собственные функции с префиксом `app_`: `#[no_mangle] pub extern "C" fn app_action(value: f32)`. Метод `app.callExport('app_action', 0.5)` вызывает такую функцию и запрашивает перерисовку. Принимаются только конечные числа; вызовы системных `rusterize_*`, отсутствующих функций и уничтоженного экземпляра отклоняются. Это доступ к собственному доверенному WASM-коду, а не механизм изоляции. Пример — [сайт фреймворка](../examples/website): Rust задаёт геометрию и рисует весь экран, HTML сохраняет обычные ссылки, прокрутку и доступные с клавиатуры элементы управления.

## Проверки браузерной оболочки

Из корня фреймворка:

```sh
python scripts/build.py --platform web
python scripts/build.py --platform web --package rusterize-native-api
cargo build --release --target wasm32-unknown-unknown -p rusterize-gallery --example web_checks
cargo build --release --target wasm32-unknown-unknown -p rusterize-gallery --example web_services
python scripts/build.py --platform web --package rusterize-shaders --features self-check
python scripts/build.py --platform web --package rusterize-webview --features self-check
npm install --prefix target/web-tools --no-save playwright@1.62.1
npx --prefix target/web-tools playwright install chromium
```

Затем установи `NODE_PATH` в абсолютный путь `target/web-tools/node_modules` и выполни `node scripts/check_web.mjs`. Для другого движка установи `WEB_BROWSER=firefox` или `webkit` и установи соответствующий браузер Playwright. Для уже установленного Edge можно указать `WEB_CHANNEL=msedge`, Chrome — `WEB_CHANNEL=chrome`. Playwright нужен только тестам, не сборке и не приложению. Скриншоты сохраняются в `target/web-check`.

Также выполни `node scripts/check_web_features.mjs`: пиксельные самопроверки WGSL, жизненный цикл WebView, изоляция iframe, история, отмена скриптов, асинхронные результаты Rust, файлы и fullscreen. Clipboard и File System Access handles в тестах подменяются; выбор через input и скачивание проверяются настоящими браузерными механизмами. Пользовательский буфер обмена не изменяется. Обычный WebView-пример собирай без `self-check`: самопроверка закрывает экземпляр после успеха.

Основные проверки используют собранный Rust fixture: пиксели RZ02, реальные браузерные события, метрики/диапазоны Unicode, idle/animation, resize/DPI, Suspend/Resume, обработку ошибки кадра и очистку обработчиков. В CI предусмотрены Chromium, Firefox и WebKit; подтверждённые локальные результаты — в [VALIDATION.md](VALIDATION.md).
