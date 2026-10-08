# RZ02 и host ABI

Все числа little-endian, без padding. Буфер целиком принадлежит Rust. Это доверенный внутрипроцессный транспорт, не формат открытия произвольных файлов.

Заголовок: 4 байта ASCII `RZ02`, затем `u32` — число команд. Каждая команда начинается с `u32` opcode.

| Opcode | Поля |
| --- | --- |
| 1 Clear | RGBA8 |
| 2 Save | — |
| 3 Restore | — |
| 4 Transform | 6 × f32: a,b,c,d,e,f |
| 5 Clip | Shape |
| 6 Fill | Shape, Paint |
| 7 Stroke | Shape, Paint, f32 width |
| 8 Text | Point baseline; TextDescriptor |
| 10 TextLayout | Point top-left; TextDescriptor |
| 9 Image | u64 ID; u32 width,height; Rect; f32 opacity; u32 byte length; RGBA8 pixels |

Point = 2 × f32, Rect = 4 × f32 (x,y,width,height), RGBA8 = четыре байта r,g,b,a.

Shape: `u32 kind`, затем: 0 → Rect; 1 → Rect + f32 radius; 2 → Rect; 3 → u32 segment count + segments. Radius сериализатор ограничивает половиной меньшего измерения. Сегмент: u32 opcode, 0 Move → Point, 1 Line → Point, 2 Cubic → Point control1, control2, end, 3 Close → без полей.

Paint: `u32 kind`, 0 Solid → RGBA8; 1 Linear → Point from, Point to, RGBA8 start, RGBA8 end. Шрифты: 0 Sans, 1 Serif, 2 Monospace. Images содержат straight-alpha RGBA8, верхняя строка первая.

Clear может быть только первой командой; он заменяет весь Canvas. Кадр начинается с пустого состояния и прозрачной поверхности. Save/Restore сохраняют трансформацию и clip. Матрицы умножаются справа. Clip остаётся в пространстве, актуальном в момент его установки. Fill: winding; Stroke: butt/miter/10. Рендерер восстанавливает внешнее состояние после кадра.

## C ABI

`title(id)` возвращает UTF-8 название окна. `window_option(id, option)` возвращает u32: 0…3 — битовое представление f32 width,height,min_width,min_height; 4…6 — RGB 0x00RRGGBB для фона, текста и рамки заголовка (0xFFFFFFFF — системный цвет); 7 — resizable; 8 — revision. После смены revision оболочка перечитывает title и цвета. Эти настройки передаются отдельно от display list и не изменяют RZ02.

Объявления: `hosts/rusterize.h`. `create` выдаёт ненулевой handle на текущем потоке. `destroy` идемпотентен для отсутствующего handle. `frame` обновляет viewport, вызывает приложение и сохраняет новый буфер. `data`/`len` возвращают его заимствование. `error` — NUL-terminated строка, её нельзя освобождать. Буфер и ошибка действительны до следующего изменяющего вызова для этого handle. Один handle обслуживается только UI-потоком.

Статус — битовая маска: 1 REDRAW (запрошен кадр), 2 ANIMATE (нужны периодические кадры), 4 EXIT, 0x80000000 FAILED. При FAILED запроси `error`, покажи/запиши ошибку и закрой оболочку. Не продолжай бесконечно запрашивать кадры.

`event(id, kind, x,y,dx,dy,detail,flags)`:

| kind | Смысл | detail | flags |
| --- | --- | --- | --- |
| 1,2,3,4 | Pointer Down,Move,Up,Cancel | ID контакта | buttons |
| 5 | Scroll | 0 | 0 |
| 6,7 | Key down/up | код клавиши | shift=1,ctrl=2,alt=4,meta=8,repeat=16 |
| 8 | Text | Unicode scalar value | 0 |
| 9 | Focus | 0/1 | 0 |
| 10,11 | Suspend/Resume | 0 | 0 |

Коды общих клавиш: Enter=13, Space=32, Escape=27, Tab=9, Backspace=8, Delete=46, Left=37, Up=38, Right=39, Down=40, Home=36, End=35. Другие коды принадлежат платформе. Контрольные Unicode-символы отфильтровываются. Неизвестный event kind игнорируется; неизвестные рисующие команды являются ошибкой версии.

При изменении раскладки команд обнови magic/version и каждый декодер. Дополнение события не требует менять рисующий протокол, если сохраняется прежняя семантика остальных событий.


## Текстовый дескриптор RZ02

`f32 size; RGBA8; u32 family; u32 flags; String font_name; f32 width; u32 wrap; u32 align; u32 direction; String text`. String = u32 byte length + UTF-8, без NUL. Пустое font_name использует generic family. Flags: bold=1, italic=2, underline=4, strikethrough=8. Wrap: none=0, word=1, character=2. Align: left=0, center=1, right=2. Direction: left-to-right=0, right-to-left=1. Для opcode 8 options равны width=1000000, wrap=0, align=0, direction=0. Opcode 8 остаётся однострочным.

RZ02 несовместим с RZ01: при обновлении зависимости нужно обновить скопированные `hosts` в существующих приложениях. Генератор копирует актуальные оболочки и документацию автоматически.

## Обратный сервис оболочки

`rusterize_set_native_service(callback)` регистрирует callback на UI-потоке до `create`. Callback получает заимствованные request bytes и length, возвращает pointer и заполняет output length. Буфер ответа принадлежит оболочке и живёт до следующего вызова callback. Начало C/JNI-ответа: u32 status (0 успех; 1 ошибка), затем данные или UTF-8 ошибки без length. Rust сразу копирует ответ, проверяет длину и метрики. Максимальный ответ — 64 МиБ. Callback не должен unwinding пересекать ABI. В Rust `native::set_service` принимает эквивалентную closure, но возвращает Result с payload без status.

| Op | Запрос после u32 op | Payload успешного ответа |
| --- | --- | --- |
| 1 | TextDescriptor | 4×f32 width, width_with_trailing, height, baseline; u32 count; count×Line |
| 2 | — | u32 capability mask |
| 3 | — | UTF-8 clipboard без length |
| 4 | String clipboard | Пусто |
| 5 | u32 cursor | Пусто |
| 6 | u32 window_state | Пусто |
| 7 | f32 width,height | Пусто |
| 8 | f32 x,y | Пусто |
| 9 | u32 topmost | Пусто |
| 10 | u32 save,directory; String title; String suggested_name | u32 count; count×String path; 0 означает отмену |
| 11 | String title; String message | Пусто |
| 12 | String URI | Пусто |

Line = u32 UTF-8 start,end; f32 top,height,baseline. Диапазоны последовательны, полностью покрывают исходную строку и не разрезают UTF-8. Baseline абсолютна относительно layout. Capability bits: text=1, clipboard=2, cursor=4, window state=8, size=16, position=32, topmost=64, file dialog=128, message dialog=256, URI=512. Неизвестная операция возвращает ошибку.

Cursor: Arrow=0, Text=1, Hand=2, Crosshair=3, Move=4, ResizeHorizontal=5, ResizeVertical=6, Hidden=7. WindowState: Normal=0, Minimized=1, Maximized=2, Fullscreen=3.

`rusterize_poll_native(id)` вызывается оболочкой при планировании кадра/после события. Он освобождает заимствование Host до вызова сервиса и доставляет результат как Event::NativeResult. Повторный вход не исполняет запросы повторно. Оболочка не должна выполнять poll между получением pointer display list и его чтением: вложенный цикл событий может заменить Rust-буфер.

`rusterize_native_draw(id, context, view, window)` — callback GTK/AppKit после replay, внутри нативного кадра. Указатели действительны только на время вызова; offscreen допускает null window/view. JNI использует аналогичный nativeDraw с Java-объектами, Windows передаёт COM-интерфейсы напрямую. Это доверенный FFI; указатели не сериализуются. Ошибка callback возвращает FAILED. Исходные состояния render context должны быть восстановлены перед callback; изменения самого приложения также должны быть сбалансированы.
