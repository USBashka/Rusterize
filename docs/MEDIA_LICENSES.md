# Зависимости медиамодулей

Версии закреплены в Cargo.lock. Таблица относится к новым прямым зависимостям; полный граф включает также их зависимости и существующий Windows/JNI стек.

| Компонент | Версия | Лицензия из Cargo.toml | Назначение и исходники |
| --- | --- | --- | --- |
| CPAL | 0.16.0 | Apache-2.0 | Нативный вывод: [исходники пакета](https://docs.rs/crate/cpal/0.16.0/source/) |
| Symphonia | 0.5.5 | MPL-2.0 | MP3, WAV, PCM: [исходники пакета и зависимостей](https://docs.rs/crate/symphonia/0.5.5/source/) |
| image | 0.25.6 | MIT OR Apache-2.0 | PNG/JPEG и EXIF orientation: [исходники пакета](https://docs.rs/crate/image/0.25.6/source/) |
| ndk-context | 0.1.1 | MIT OR Apache-2.0 | Application context для Android: [исходники пакета](https://docs.rs/crate/ndk-context/0.1.1/source/) |
| ndk | 0.9.0 | MIT OR Apache-2.0 | Чтение аппаратной позиции AAudio: [исходники пакета](https://docs.rs/crate/ndk/0.9.0/source/) |

Symphonia состоит из нескольких пакетов той же версии, включая core, metadata, bundle-mp3, codec-pcm и format-riff; их лицензия также MPL-2.0. Исходники этих библиотек в этой работе не изменялись. Выбор ограниченного набора кодеков уменьшает размер и поверхность зависимостей. CPAL/Symphonia отсутствуют в нормальном графе WASM-сборки; браузер предоставляет Web Audio самостоятельно.

Для подготовки дистрибутива выгрузите фактический граф `cargo metadata --locked --format-version 1`, сопоставьте включённые пакеты с `cargo tree -p rusterize-media --target <target> -e normal` и приложите соответствующие тексты лицензий/уведомления. MPL-2.0 и применимые условия распространения описаны в тексте лицензии upstream; эта таблица не заменяет лицензии.

Демонстрационные `demo.mp3` и `cover.png` создаются `scripts/generate_media_fixtures.py`: собственная синтезированная мелодия и процедурная графика, лицензия проекта. Генерация MP3 требует FFmpeg как инструмента разработчика; в готовые приложения он не включается.
