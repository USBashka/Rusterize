# Проверка медиасервисов — 28 октября 2026

Эта проверка относится к медиамодулям текущего рабочего дерева. Исходные незакоммиченные изменения GPU/ресурсов сохранены. Локальные результаты и обновлённая конфигурация CI — разные виды доказательств; CI этой ревизии в GitHub из данной сессии не запускался.

## Матрица

| Цель | cargo check / Clippy | Полная release-сборка | Реальный запуск |
| --- | --- | --- | --- |
| Windows x64 MSVC | Да | EXE со статическим CRT | Да: окно примера, WASAPI и автоматические тесты |
| WASM wasm32-unknown-unknown | Да | app.wasm + JS/HTML | Да: Edge и Chrome на Windows, настоящий WASM/Web Audio/Worker/IndexedDB |
| Android ARM64 | Да | APK с ARM64 и x86-64, подпись v2/v3 проверена | Физический ARM64-телефон не подключался |
| Android x86-64 | Rust-код собран с NDK | Та же APK | Android 16.1 / API 36.1 emulator, SwiftShader |
| Linux | Добавлено в CI с ALSA development libraries | Добавлено в CI | Локальная Linux-среда недоступна; не заявляется |
| macOS | Добавлено в CI | Добавлено в CI, CoreAudio frameworks | macOS/Xcode недоступны; не заявляется |
| Firefox / WebKit / Safari | Для Firefox/WebKit добавлен браузерный CI-тест | Общий web-пакет | Локально не запускались; WebKit CI не равнозначен Safari на устройстве |

## Что проверяется

Итог локального прогона: **47** workspace unit/integration/doc tests прошли; 7 аппаратных тестов отмечены ignored в обычном запуске (6 существующих GPU и 1 аудио). Аудиотест отдельно запущен с реальным Windows-выходом и прошёл. Ядро без default features: **19** тестов прошли. Workspace/WASM/Android Clippy с `-D warnings`, отдельные сборки features files/image-codecs/audio, rustfmt, rustdoc, 5 Python-тестов и `git diff --check` прошли. В окружении встречались предупреждения hard-link incremental cache; Cargo переходил на копирование. Первую полную Windows-линковку ограничила песочница MSVC; повтор с доступом к существующему target прошёл.

- Unit-тесты: MP3 и WAV float PCM, длительность/анализ, повреждённые и обрезанные данные, лимит PCM-копии, NaN; PNG alpha, JPEG EXIF-поворот, чрезмерные размеры до распаковки, отсутствие повторной декодировки при clone.
- Контракт аудиочасов: интерполяция только внутри отправленного PCM, отложенное применение seek/pause на выходной шкале, игнорирование незавершённого временного сегмента и защита от завершения старой команды после новой.
- Файлы: типы path/URI/token, имена/переходы по каталогам, отсутствующее и пустое значение, атомарная замена и сохранность предыдущего файла при отказе. Worker: результаты, ID/порядок, ошибка и пробуждение неанимирующего Engine.
- Windows audio test с реальным устройством: два одновременных плеера, ход позиции, стабильная пауза, seek, конец/однократное событие, stop, interruption и невалидные параметры. Это программное чтение часов, не микрофонное измерение задержки.
- `scripts/check_web_media.mjs`: реальный пример, разблокировка нажатием, музыка/спектр, выбор MP3/PNG через file input, download и проверка байтов, восстановление настроек после reload; Web Audio clock/pause/seek/end/interruption; Worker и ошибка декодера; IndexedDB, лимит размера, принудительно прерванная настоящая транзакция и сохранность прежнего значения, два host clock. Переполнение реальной квоты диска не провоцировалось.
- `scripts/check_android_media.py`: изменение спектра, pause/seek, эффект поверх музыки, HOME/возврат без автозапуска, сохранение громкости через force-stop/restart, системный picker и отмена, экспорт через CREATE_DOCUMENT с проверкой байтов, fullscreen/normal. Ранее вручную выбран MP3 из Downloads и прочитан через ContentResolver. Подсказка первого fullscreen принадлежит Android и учитывается тестом.

Прогон Android обнаружил несовместимые начала отсчёта callback/playback timestamp в CPAL 0.16 AAudio. Android-подключение теперь читает аппаратный frame_position/CLOCK_MONOTONIC непосредственно через ndk, а callback записывает сегменты по выходным frames. Это проверено повторным запуском, а не только cross-check.

## Воспроизведение

```sh
cargo test --workspace --locked
cargo test -p rusterize --no-default-features --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p rusterize-media --target wasm32-unknown-unknown --locked -- -D warnings
cargo clippy -p rusterize-media --target aarch64-linux-android --locked -- -D warnings
cargo test -p rusterize --features media output_clock_pause_seek_effect_and_completion -- --ignored --nocapture
python -m unittest discover -s scripts -p 'test_*.py'
```

Последний Rust-тест требует реального аудиовыхода и проигрывает короткий тихий звук; обычный cargo test оставляет его ignored. Браузерному скрипту нужен Playwright (`NODE_PATH=.../target/web-tools/node_modules`, `WEB_CHANNEL=msedge` или `chrome`); Android-скрипт требует явный `--serial emulator-...` и отказывается управлять физическим телефоном. PNG/логи локального прогона лежат в `target/media-check`.

Генератор проверен созданием нового проекта: `cargo check` и полная web release-сборка с `--features rusterize/media` прошли, новые JS/Java-модули и руководства копируются.

Проверка MSRV 1.85 медиамодулей добавлена в CI; локально использован установленный stable toolchain. Нативные GPU-тесты исходных пользовательских изменений не заменяют медиапроверки и не включаются в их счёт.

## Размеры

Размеры воспроизводятся скриптом `scripts/measure_media.py` после сборки минимального примера, media с ресурсами и media без default feature bundled-media. Release-профиль: opt-level=s, thin LTO, один codegen unit, strip symbols, panic=abort. Windows — static CRT. Веб измеряется как сумма файлов WASM/JS/HTML и сумма gzip каждого файла (level 9, mtime 0), без HTTP-заголовков; также создаётся отдельный tar.gz для передачи.

Измерено локально с `rustc 1.93.1 (01f6ddf75 2026-02-11)`, в байтах:

| Артефакт | Минимальный | Media без MP3/PNG | Media с MP3/PNG |
| --- | ---: | ---: | ---: |
| Windows x64 EXE | 296 960 | 1 182 208 | 1 341 440 |
| WASM | 98 028 | 483 289 | 642 693 |
| Весь web-пакет, 10 файлов | 165 985 | 551 246 | 710 650 |
| Web, сумма gzip файлов | 59 511 | 208 549 | 366 878 |
| Web tar.gz | 57 260 | 206 984 | 365 199 |
| APK, ARM64 + x86-64 | 545 326 | Не измерено | 1 774 126 |

Встроенные материалы: `demo.mp3` — **72 638**, `cover.png` — **86 495**, вместе **159 133 байта**. Разница готовых сборок: EXE **159 232**, WASM/web **159 404**, web gzip **158 329 байт**; небольшое отличие от суммы исходников связано с кодом загрузки и размещением в бинарнике. Короткий WAV-эффект синтезируется при старте, отдельного встроенного файла нет. Минимальный пример не содержит медиаресурсов. Размер APK уже включает ZIP-сжатие и две ABI; без подписи/Java/системных библиотек его не сравнивали. Системные Windows DLL, браузерный движок, Android runtime и GTK/ALSA в эти размеры не входят.

Готовые файлы: `dist/media.exe`, `dist/rusterize-media.apk`, `dist/web/rusterize-media/`, `dist/rusterize-media-web.tar.gz`. Полный машинный отчёт с SHA-256 EXE/APK — `dist/media-sizes.json`. JS-оболочка минимального web-пакета содержит общие сервисные модули; их неиспользуемые части пока не удаляются сборщиком. Размеры Linux/macOS здесь не измерены. Сравнение двух разных приложений включает код UI и медиасервисов и не является чистым размером одной Cargo feature. Вариант media без встроенных ресурсов даёт более точную оценку вклада самих материалов.

## Границы

Не измерены физическая акустическая задержка, Bluetooth, длительная работа/энергопотребление на телефоне, смена реального Android-устройства вывода, звонок/audio focus от другого установленного приложения, нехватка реального дискового пространства и аварийное отключение питания. Audio focus обработан кодом; HOME/Suspend и программное interruption проверены отдельно. На Linux/macOS нужны полноценные сборки и запуски в CI/на устройствах. Эти ограничения важны до переноса ритм-игры в production.
