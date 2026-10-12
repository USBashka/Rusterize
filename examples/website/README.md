# Сайт Rusterize на Rusterize

Работающая страница: [GitHub Pages](https://usbashka.github.io/Rusterize/) и [usbashka.ru/rusterize](https://usbashka.ru/rusterize).

`src/lib.rs` рисует всю страницу через Rusterize: заголовки, карточки, код, скульптуру из путей и интерактивные примеры. Геометрия ссылок, кнопок и адаптивных секций тоже рассчитывается в Rust. Демонстрации показывают градиенты, отсечение, преобразования, перетаскивание, TextLayout с Unicode и WGSL через WebGL 2.

`web/` — небольшая браузерная оболочка: обычная прокрутка документа, ссылки, клавиатурный фокус, доступные элементы управления, семантический текст, тема и буфер обмена. Canvas фиксирован и ограничен размером экрана. Анимация останавливается при скрытии вкладки, вне анимированных секций, по кнопке и при `prefers-reduced-motion`. Без JavaScript или при ошибке WASM остаются текст, инструкция запуска и ссылки. Страница требует браузерный хост; нативные демонстрации находятся в соседних примерах.

## Сборка и проверка

```sh
rustup target add wasm32-unknown-unknown
python scripts/build_website.py
python -m http.server 8000 --bind 127.0.0.1 --directory dist/web/rusterize-website
```

Из корня репозитория, с Playwright в `NODE_PATH`:

```sh
node scripts/check_website.mjs
```

Для установленного Edge: `WEB_CHANNEL=msedge`. Для другого движка: `WEB_BROWSER=firefox|webkit`. Для публичной страницы: `WEBSITE_URL=https://usbashka.ru/rusterize`. Проверка открывает отдельный временный профиль, подменяет только буфер обмена, проверяет обе темы, ширины 320–1440, вкладки, ползунок, анимацию, клавиатуру, WASM bridge и fallback без JS. Снимки — `target/website-check/`.

## Публикация

`.github/workflows/pages.yml` собирает Rust/WASM, проверяет страницу в Chromium и развёртывает её в GitHub Pages после push в main. Источник Pages в настройках репозитория — GitHub Actions.

Для usbashka.ru скопируйте содержимое `dist/web/rusterize-website` в `web/rusterize` репозитория сайта. В его HTML относительные `href="./…"` и `src="./…"` заменяются на `/static/rusterize/…`, поскольку канонический маршрут `/rusterize` не имеет завершающего слеша. Остальные ресурсы не меняются. `version.json` содержит commit исходников. WASM требует MIME `application/wasm` и `script-src 'self' 'wasm-unsafe-eval'` только для этой страницы. Обычный `unsafe-eval` не нужен.

Шрифт Manrope хранится локально вместе с лицензией SIL OFL. Браузерная оболочка не обращается к CDN и не содержит аналитики.
