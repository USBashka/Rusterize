# RZ01 и host ABI

Все числа little-endian, без padding. Буфер целиком принадлежит Rust. Это доверенный внутрипроцессный транспорт, не формат открытия произвольных файлов.

Заголовок: 4 байта ASCII `RZ01`, затем `u32` — число команд. Каждая команда начинается с `u32` opcode.

| Opcode | Поля |
| --- | --- |
| 1 Clear | RGBA8 |
| 2 Save | — |
| 3 Restore | — |
| 4 Transform | 6 × f32: a,b,c,d,e,f |
| 5 Clip | Shape |
| 6 Fill | Shape, Paint |
| 7 Stroke | Shape, Paint, f32 width |
| 8 Text | f32 x,y,size; RGBA8; u32 family; u32 bold; u32 byte length; UTF-8 bytes |
| 9 Image | u64 ID; u32 width,height; Rect; f32 opacity; u32 byte length; RGBA8 pixels |

Point = 2 × f32, Rect = 4 × f32 (x,y,width,height), RGBA8 = четыре байта r,g,b,a.

Shape: `u32 kind`, затем: 0 → Rect; 1 → Rect + f32 radius; 2 → Rect; 3 → u32 segment count + segments. Radius сериализатор ограничивает половиной меньшего измерения. Сегмент: u32 opcode, 0 Move → Point, 1 Line → Point, 2 Cubic → Point control1, control2, end, 3 Close → без полей.

Paint: `u32 kind`, 0 Solid → RGBA8; 1 Linear → Point from, Point to, RGBA8 start, RGBA8 end. Шрифты: 0 Sans, 1 Serif, 2 Monospace; bold 0/1. Images содержат straight-alpha RGBA8, верхняя строка первая.

Clear может быть только первой командой; он заменяет весь Canvas. Кадр начинается с пустого состояния и прозрачной поверхности. Save/Restore сохраняют трансформацию и clip. Матрицы умножаются справа. Clip остаётся в пространстве, актуальном в момент его установки. Fill: winding; Stroke: butt/miter/10. Рендерер восстанавливает внешнее состояние после кадра.

## C ABI

`title(id)` возвращает UTF-8 название окна. `window_option(id, option)` возвращает u32: 0…3 — битовое представление f32 width,height,min_width,min_height; 4…6 — RGB 0x00RRGGBB для фона, текста и рамки заголовка (0xFFFFFFFF — системный цвет); 7 — resizable; 8 — revision. После смены revision оболочка перечитывает title и цвета. Эти настройки передаются отдельно от display list и не изменяют RZ01.

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
