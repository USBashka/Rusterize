# WGSL для элементов Canvas

Шейдеры включаются явно. Naga работает только при сборке; приложение содержит переведённый GLSL для своей платформы и небольшой исполнитель поверх системного OpenGL. `wgpu`, Skia и компилятор WGSL в дистрибутив не входят. Обычные приложения без feature `shaders` не получают `glow` и не создают GPU-контекст шейдеров.

Обычный `cargo build` в workspace также не собирает шейдерный пример и Naga: они исключены из default-members. Для них явно выбери `-p rusterize-shaders` либо `--workspace`.

## Подключение

Пакеты пока используются из checkout фреймворка:

```toml
[dependencies]
rusterize = { path = "../Rusterize/crates/rusterize", features = ["shaders"] }

[build-dependencies]
rusterize-shader-build = { path = "../Rusterize/crates/rusterize-shader-build" }
```

В `build.rs`:

```rust
fn main() {
    rusterize_shader_build::compile_file("shaders/wave.wgsl", "wave.rs").unwrap();
}
```

В `shaders/wave.wgsl`:

```wgsl
fn shade(uv: vec2f) -> vec4f {
    let wave = sin(uv.x * 12.0 + params.time * 2.0) * 0.5 + 0.5;
    let color = mix(vec3f(0.1, 0.4, 0.9), vec3f(0.9, 0.2, 0.6), wave);
    return vec4f(color, 1.0);
}
```

Рисование в Rust:

```rust,ignore
use rusterize::{shader::*, *};

static WAVE: ShaderSource = include!(concat!(env!("OUT_DIR"), "/wave.rs"));

// Создай один renderer на UI-потоке и сохрани в состоянии приложения.
let mut renderer = ShaderRenderer::new()?;
let shader = WAVE.shader(ShaderParams { time: elapsed, ..Default::default() });

canvas.with_save(|canvas| {
    canvas.clip(Shape::RoundedRect(bounds, 16.0));
    canvas.shader(&mut renderer, bounds, viewport.scale, &shader);
});
```

`Canvas::shader` на всех платформах записывает отложенную команду Shader. Эффект исполняется при композиции кадра с сохранением порядка рисования, clip, transform, текста поверх и прозрачности. Неверные размеры и параметры отклоняет `Canvas::finish`; ошибки драйвера или компиляции отложенной команды возвращает рендерер. Нулевой размер области ничего не рисует.

Для повторного использования результата вызови `renderer.render(&shader, [width, height], scale)?` и сохрани возвращённый `Image`. Клонирование изображения дёшево. `ShaderRenderer` принадлежит создавшему его потоку, не реализует Send/Sync и восстанавливает предыдущий текущий OpenGL-контекст после работы.

## Контракт WGSL

Автор определяет `fn shade(uv: vec2f) -> vec4f` и дополнительные функции/константы. Вершинную и фрагментную точки входа предоставляет сборочный пакет. WGSL проходит проверку Naga, затем преобразуется в GLSL 330 либо GLSL ES 300. Драйвер компилирует GLSL при первом использовании; это не готовый машинный код для конкретной GPU.

| Значение | Смысл |
| --- | --- |
| `uv` | Нормированные координаты элемента: (0,0) вверху слева, (1,1) внизу справа |
| `params.resolution` | Размер результата в физических пикселях |
| `params.time` | Переданное приложением время в секундах |
| `params.scale` | Переданный масштаб DPI |
| `params.data[0..4]` | Четыре vec4f пользовательских параметров |
| `sample_image(uv)` | Выборка из входного изображения; линейная фильтрация, clamp-to-edge |
| результат | Straight-alpha RGBA в диапазоне 0…1; компоненты RGB трактуются как sRGB |

Входная текстура подключается через `WAVE.shader(params).with_image(&image)`. Без неё используется белый непрозрачный пиксель. Выборка возвращает straight-alpha RGBA; при написании фильтров учитывай обработку прозрачных цветов. Системное преобразование изображений в premultiplied alpha выполняется далее штатным Canvas.

Доступны математика, векторы, матрицы, функции, локальные массивы, условия и циклы в пределах поддерживаемого Naga профиля. Дополнительные bindings, storage buffers, compute/собственные vertex entry points, overrides и возможности вне выбранного профиля отклоняются при сборке. Это API двумерных элементов, а не общий вычислительный GPU API. WGSL-код — доверенная часть приложения; произвольные загруженные из сети шейдеры не являются поддерживаемым сценарием.

## Системные требования

| ОС | Исполнение |
| --- | --- |
| Windows | WGL, OpenGL 3.3 core, системный `opengl32.dll` и установленный драйвер GPU |
| Android 8+ | EGL и OpenGL ES 3.0; прямая композиция с Android 10/API 29 через HardwareBuffer/EGLImage и аппаратный Canvas |
| Linux | GtkGLArea, GLES 3.0 и GL-рендерер GTK; для сборки нужны EGL и epoxy, например `libegl1-mesa-dev libepoxy-dev` |
| macOS | Системный CGL/NSOpenGLView, OpenGL 4.1 core; Apple объявила OpenGL устаревшим |
| Веб | WebGL 2, GLSL ES 300; raw WASM ABI без glow/wasm-bindgen в runtime |

При отсутствии подходящего драйвера `ShaderRenderer::new()` возвращает ошибку. Приложение выбирает обычную заливку либо показывает ошибку. Автоматического скрытого CPU-интерпретатора WGSL нет. Mesa может предоставлять программную реализацию OpenGL; фактический исполнитель доступен через `renderer_name()`.

На Windows шейдер по-прежнему исполняется в OpenGL, а совместная текстура D3D11 передаётся Direct2D через `WGL_NV_DX_interop2`. Если совместимость драйвера или target не позволяет этот путь, используется readback. macOS пока не использует Metal, Android — AGSL.

## Стоимость и ограничения

`Canvas::shader` в вебе рисует в WebGL canvas и сразу передаёт его в `CanvasRenderingContext2D.drawImage`. На пути кадра нет `readPixels`, RGBA-ответа в WASM и обратной сериализации результата. Конкретный браузер выбирает внутренний способ копирования; это не гарантия zero-copy на любом устройстве. Входные изображения и исходники загружаются по ресурсным ID один раз за время присутствия в сцене. После восстановления WebGL-контекста сохранённые ресурсы автоматически загружаются повторно.

На Windows оконный renderer использует D3D11/DXGI и Direct2D. При доступном GL/D3D interop эффект пишет прямо в совместную текстуру; отдельные поверхности команд сохраняются до завершения кадра. Это устраняет чтение GPU → CPU при обычной композиции. При неподдерживаемом драйвере и для несовместимых внешних Direct2D targets сохраняется путь через Image.

Для внешнего D3D11 target есть `D2dRenderer::new_gpu(factory, context, device)`. `gpu_shader_draws()` возвращает число команд последнего кадра, которые прошли без readback. Кэш ограничен 32 совместными поверхностями; следующие Shader-команды кадра используют Image. Неиспользованные поверхности освобождаются после EndDraw, используемые сохраняются между кадрами.

На Android ES3 пишет в HardwareBuffer через EGLImage. После `glFinish` оболочка передаёт Canvas неизменяемый аппаратный Bitmap. Его буфер не перезаписывается, пока Canvas может хранить ссылку. Неизменившиеся результаты кэшируются: максимум 16 записей и 64 МиБ. Новый анимированный результат требует нового буфера; синхронный `glFinish` также имеет стоимость. На API 26–28, программном Canvas или при невозможности создать EGLImage используется readback. Bitmap-ресурсы не вызывают преждевременный `recycle`. Suspend очищает кэш оболочки и Encoder, Resume загружает ресурсы заново.

На Linux системные Cairo/Pango рисуют обычные участки кадра, а GtkGLArea смешивает их с выходами шейдеров. Произвольный clip сохраняется через маску покрытия Cairo, transform — через обратную матрицу. Выход эффекта остаётся в GPU-текстуре. Shader-оболочка выбирает `GSK_RENDERER=gl` и проверяет фактический GL-рендерер GTK, чтобы исключить скрытое скачивание GL-текстуры программным рендерером. При недоступной GL-композиции возвращается ошибка. `unrealize` удаляет кэш и сбрасывает Encoder.

На macOS CoreGraphics/AppKit рисуют обычные участки и маски, NSOpenGLView смешивает их с GPU-текстурами шейдеров и показывает итог. Нативные дочерние WebView остаются над Canvas. Входные текстуры и программы кэшируются по RZ03; `resetResources` очищает обе стороны протокола.

Linux/macOS загружают обычные слои и маски из RAM на GPU. Это удаление readback **результата шейдера**, а не zero-copy всего кадра и не обещание ускорения без замеров. Нативная сборка и запуск этих двух новых оболочек пока не подтверждены; см. [VALIDATION.md](VALIDATION.md).

`ShaderRenderer::render` явно возвращает CPU Image через readback: он подходит для экспорта, снимков и кэширования статического результата. `--snapshot` и `--gpu-self-check` Linux/macOS читают только итоговый framebuffer для сохранения PNG; обычный показ кадра этого не делает.

Renderer сохраняет контекст, до 32 программ и поверхности. У `render` один последний CPU-результат кэшируется по программе, размерам, всем параметрам, масштабу и ID изображения. Изменение любого из них вызывает новую отрисовку. Статические результаты нескольких элементов можно сохранять как Image. Параметры проверяются на конечность, выход ограничен 4096 пикселями по стороне и 4 194 304 пикселями суммарно, дополнительно учитывается предел устройства.

Этот API поддерживает процедурные заливки и обработку переданного Image. Автоматического захвата уже нарисованной группы Canvas, backdrop blur и произвольной цепочки проходов пока нет. Масштаб дополнительных Canvas transforms не входит автоматически в разрешение offscreen-текстуры; при сильном увеличении передай соответствующее значение `scale`.

## Пример и проверки

```sh
python scripts/build.py --platform windows --package rusterize-shaders
python scripts/build.py --platform web --package rusterize-shaders
python scripts/build.py --platform android --package rusterize-shaders --abi arm64-v8a --abi x86_64
cargo test -p rusterize-shader-build
cargo test -p rusterize-shaders --test gpu -- --ignored --nocapture
cargo run -p rusterize-shaders --release -- --bench
```

GPU-тесты запускаются явно, поскольку требуют подходящего драйвера. Они проверяют координаты, ориентацию текстуры, uniform ABI, прозрачность, анимацию, кэш, размеры, ошибки и повторное создание контекстов; Windows дополнительно проверяет композицию Direct2D. Пример поддерживает паузу по нажатию/пробелу, resize и DPI. `--features self-check` добавляет пиксельную самопроверку перед показом примера, в том числе для APK. Бенчмарк измеряет исполнение эффекта вместе с readback и созданием Image, но не весь нативный кадр. Проверенные сборки и измеренные размеры — в [VALIDATION.md](VALIDATION.md).

Общая сцена `composition-check` рисует четыре эффекта, обычные слои до и после них, входную текстуру, alpha, произвольный clip и поворот. Проверка PNG сравнивает 13 контрольных пикселей. Нативный `--gpu-self-check` дополнительно воспроизводит несколько кадров, меняет размеры, очищает кэши и проверяет повторную загрузку; macOS также меняет scale. Это сброс ресурсов оболочки, а не искусственный сбой физического драйвера.

```sh
# На Linux с GTK/EGL/epoxy и доступным дисплеем (либо Xvfb):
python scripts/build.py --platform linux --package rusterize-shaders --features composition-check --out target/composition-check
xvfb-run -a target/composition-check/rusterize-shaders --gpu-self-check target/composition-linux.png
python scripts/check_gpu_snapshot.py target/composition-linux.png

# На macOS:
python scripts/build.py --platform macos --package rusterize-shaders --features composition-check --out target/composition-check
target/composition-check/rusterize-shaders.app/Contents/MacOS/rusterize-shaders --gpu-self-check target/composition-macos.png
python scripts/check_gpu_snapshot.py target/composition-macos.png

# На явно выбранном Android-эмуляторе API 29+:
python scripts/build.py --platform android --package rusterize-shaders --features composition-check --abi x86_64 --app-id dev.rusterize.gpucomposition --out target/android-composition
adb -s emulator-5554 install -r target/android-composition/rusterize-shaders.apk
python scripts/check_android_gpu_composition.py --serial emulator-5554
```

Android instrumentation проверяет аппаратный тип Bitmap, счётчики прямых отрисовок/readback, ресурсный кэш, освобождение, неизменяемость ранее переданных Bitmap после eviction и уничтожения EGL-контекста. Отдельно проверяется программный fallback. Чтение пикселей в самом тесте и снимки экрана не относятся к измеряемому пути композиции.

Без Linux/macOS можно проверить формулы композиторов через Playwright/WebGL 2: `node hosts/linux/check-compositor.mjs` и `node hosts/macos/check-compositor.mjs`. Linux-тест извлекает точный GLES-код, macOS-тест меняет только профиль GLSL и precision. `NODE_PATH` указывает на установленный Playwright, `WEB_CHANNEL=msedge` выбирает локальный Edge. Эти тесты не заменяют нативную компиляцию и проверку оболочек.
