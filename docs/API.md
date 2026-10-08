# API

## Окно и заголовок

`Application::window_options()` возвращает начальные параметры для всех оболочек. `run_app(app)` использует их на Windows; `run(app, options)` позволяет явно переопределить. В `WindowOptions` доступны title, size, min_size, title_bar и resizable. Размеры — логические пиксели, допустимый диапазон 1…32768. Android выбирает размер из доступной области экрана и игнорирует desktop resize-параметры.

`TitleBarStyle { background, foreground, border }` содержит `Option<Color>`. `None` означает системное оформление; alpha игнорируется. Есть готовые `dark()`, `light()`, конструктор `colors(bg,fg)` и `with_border(color)`. `Context::set_title` и `set_title_bar` применяют изменения без пересоздания окна; неизменившиеся значения не вызывают лишней перерисовки.

| Оболочка | Фон | Текст / значки | Рамка |
| --- | --- | --- | --- |
| Windows 11, build 22000+ | DWM caption color | DWM text color | DWM border color |
| Windows 10 | Системный fallback | Системный fallback | Системный fallback |
| GTK 4 | Нативный HeaderBar | Цвет текста и кнопок HeaderBar | Оформление оконного менеджера |
| macOS | Прозрачный нативный titlebar над window background | Светлая/тёмная системная тема по яркости foreground | Системная |
| Android | Фон системных панелей и области insets, если ОС разрешает | Светлые/тёмные системные значки | Отсутствует |

AppKit и Android не обещают произвольный цвет системного текста/значков. На современных Android часть цвета системных панелей контролирует edge-to-edge policy ОС. Rusterize не скрывает системные кнопки и не имитирует заголовок рисованными кнопками. Неподдерживаемые атрибуты безопасно сохраняют системное поведение. На Windows `windows::set_title_bar(hwnd, style)` возвращает `TitleBarSupport` с фактически принятыми DWM-атрибутами.

## Приложение и кадры

`Application` содержит `event(Event, &mut Context)` и `draw(&mut Canvas, Viewport)`. `Engine` объединяет их, проверяет viewport, вызывает `Canvas::finish` и повторно использует буфер `Scene`. `draw` должен формировать полный кадр. Нативная поверхность очищается перед его исполнением.

`Context::request_redraw()` запрашивает один кадр. `set_animation(bool)` управляет периодическими `Event::Frame`; `exit()` завершает приложение. `Frame.elapsed` — монотонное время от запуска оболочки, `delta` ограничен 100 мс; первый кадр после resume получает нулевой delta. Для физики используй delta, для абсолютной фазы — elapsed. Нативный цикл не опрашивает состояние в простое.

`Viewport.size` — логический размер доступного Canvas без системных панелей. `scale` переводит логические пиксели в физические. Ось Y направлена вниз. `Rect::contains` включает левую/верхнюю и исключает правую/нижнюю границу.

## Рисование

```rust
use rusterize::*;
# let mut scene = Scene::default();
# let mut canvas = Canvas::new(&mut scene);
canvas.clear(Color::hex(0x101923));
canvas.rounded_rect(Rect::new(20.0, 20.0, 200.0, 80.0), 12.0, Color::WHITE);
canvas.with_save(|c| {
    c.clip(Rect::new(20.0, 20.0, 200.0, 80.0));
    c.translate(80.0, 60.0);
    c.transform(Transform::rotate(0.25));
    c.circle((0.0, 0.0), 32.0, Color::hex(0x79e2c0));
});
# canvas.finish().unwrap();
```

`fill`, `stroke` и `clip` принимают `Shape`: `Rect`, `RoundedRect`, `Ellipse`, `Path`. Удобные методы `fill_rect`, `rounded_rect`, `ellipse`, `circle`, `line` создают соответствующие команды. Толщина обводки положительна; концы butt, соединения miter, предел miter 10.

`Path::builder()` поддерживает `move_to`, `line_to`, `cubic_to`, `close`, `build`. Каждая новая фигура начинается с `move_to`; после `close` нужна новая `move_to`. Заливка использует non-zero winding. Незамкнутый контур при заливке неявно замыкается системным Canvas.

`Paint::Solid(Color)` и `Paint::Linear { from, to, start, end }`. `Color` — sRGB, каналы RGBA по 8 бит, обычная прозрачность. Градиент имеет два различных конца и продолжается крайним цветом за пределами отрезка. Интерполяция выполняется нативным API и может немного отличаться между платформами.

`Transform([a,b,c,d,e,f])`: `x' = ax + cy + e`, `y' = bx + dy + f`. `concat` и `Canvas::transform` умножают текущую матрицу справа. Вызовы translate, затем scale сначала масштабируют фигуру и затем смещают её. Невырожденность и конечность составной матрицы проверяются до передачи в ОС.

`text(string, baseline, TextStyle)` рисует одну строку UTF-8. Размер шрифта — в логических пикселях. `FontFamily::{Sans,Serif,Monospace}`, `TextStyle::font("имя")`, `bold()`, `italic()`, `underline()` и `strikethrough()`. Системная замена отсутствующих глифов зависит от ОС. `measure_text` возвращает нативные метрики; `TextLayout::new` создаёт многострочную раскладку, а `Canvas::text_layout` рисует её от верхнего левого угла. Подробности и примеры — в [нативном API](NATIVE_API.md).

`Image::rgba(width,height,pixels)` проверяет точное число байтов и лимит 64 МиБ. Ресурс неизменяемый, `Clone` разделяет данные. `image(&image, destination, opacity)` масштабирует полное изображение с линейной интерполяцией. Декодирование PNG/JPEG не входит в ядро: декодируй подходящей библиотекой в RGBA8 один раз при загрузке.

## События

`Pointer` содержит `id`, `phase`, `position`, `buttons`. Фазы: Down, Move, Up, Cancel. ID мыши — 0. Биты кнопок: левая 1, правая 2, средняя 4. Android передаёт устойчивые ID касаний; Linux/macOS в этой версии передают мышь или основной эмулированный указатель.

`Scroll.delta` положителен вправо/вниз. Дискретное деление колеса нормализуется примерно к 40 логическим пикселям; чувствительность точных трекпадов отличается.

`Key` содержит pressed, repeat и modifiers. Общие клавиши представлены enum; `Unknown(code)` — платформенный код, его нельзя переносимо трактовать. `Text(String)` содержит вводимые символы. Полноценный IME, экранная клавиатура, composition/preedit и accessibility tree в 0.1 не реализованы.

При `Suspend` движок останавливает запросы анимации, при `Resume` сбрасывает delta. Состояние сохраняется в памяти, пока жива оболочка. Сохранение после уничтожения процесса/Activity должно реализовываться приложением; сериализации состояния в ядре пока нет.

## Нативный код

`Application::native_draw` даёт доступ к исходным объектам каждой ОС. Системные действия запрашиваются через `Context` и возвращаются как `Event::NativeResult`. Поддержка перечислена в [таблице](NATIVE_API.md).

`Renderer::render(&Scene, Viewport)` — интерфейс нового Rust-адаптера. `windows::D2dRenderer::new(factory,target)` позволяет использовать готовый Direct2D target. `windows::render_offscreen` возвращает premultiplied BGRA8 и используется для тестов без окна.

Feature `shaders` добавляет `shader::{ShaderSource, ShaderParams, ShaderRenderer}` и `Canvas::shader`. WGSL переводится Naga при сборке приложения; результат исполняется системным OpenGL и становится обычным Image в display list. Подключение, текстурный вход, кэш и ограничения readback описаны в [SHADERS.md](SHADERS.md).

`export_app!` создаёт C ABI и, на Android, JNI. Внешние оболочки получают проверенный display list через бинарный [протокол](protocol.md). Ошибка кадра очищает его целиком; host переходит в состояние FAILED. Release-профиль abort завершает процесс при panic приложения — это не механизм восстановления ошибок.
