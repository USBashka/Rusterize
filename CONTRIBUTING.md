# Разработка

Сначала воспроизведите проблему на минимальном `Application`. Сохраняйте общий контракт Canvas и указывайте, какие платформы были действительно проверены.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python -m unittest discover -s scripts -p "test_*.py"
```

Windows требует MSVC Build Tools, Linux — `libgtk-4-dev`, Pango и `pkg-config`, macOS — Xcode Command Line Tools. Android собирается отдельным сценарием при наличии SDK/NDK/JDK. Оболочки лежат в `hosts`; их код нужен для связи с системными API, прикладная логика остаётся в Rust.

Изменение бинарного протокола требует обновить все декодеры, golden-тест и [спецификацию](docs/protocol.md). Для ошибок владения, DPI, клиппинга и жизненного цикла добавляйте тест на наблюдаемое поведение.

CI собирает нативные артефакты для четырёх платформ, проверяет сгенерированный проект и сохраняет результаты в Actions artifacts. Эти APK имеют отладочную подпись. Для изменения API обновите rustdoc, руководство и SKILL.md.

После сборки примера WebView запустите `python scripts/check_native_webview.py <путь-к-приложению>`. Проверка требует маркер `WEBVIEW_SMOKE_OK` и успешное завершение за 45 секунд; лимит меняется через `--timeout`. На Linux нужен графический сеанс, в CI используются Xvfb и D-Bus. Браузерные сценарии запускаются отдельно для Chromium, Firefox и WebKit через `WEB_BROWSER`.

Не добавляйте `target`, `dist`, SDK/NDK, ключи подписи или локальные пути к инструментам в Git. Публичные API версии 0.1 ещё могут меняться. Вклад принимается под MIT OR Apache-2.0.
