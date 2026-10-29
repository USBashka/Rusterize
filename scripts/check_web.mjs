// Run after building gallery, native-api and the web_checks cdylib for WASM.
// npm install --no-save playwright; npx playwright install chromium
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile, mkdir } from 'node:fs/promises';
import { resolve, dirname, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
const { chromium, firefox, webkit } = createRequire(import.meta.url)('playwright');
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const out = resolve(root, 'target/web-check');
await mkdir(out, { recursive: true });
const fixture = resolve(root, 'target/wasm32-unknown-unknown/release/examples/web_checks.wasm');
const server = createServer(async (req, res) => {
  try {
    const pathname = new URL(req.url, 'http://localhost').pathname;
    let file;
    if (pathname === '/checks.wasm') file = fixture;
    else if (pathname === '/checks.html') {
      res.setHeader('Content-Type', 'text/html');
      res.end('<!doctype html><meta charset="utf-8"><style>body{margin:0}canvas{display:block;width:100vw;height:100vh}</style><canvas id="app" tabindex="0"></canvas>');
      return;
    } else {
      const path = pathname.endsWith('/') ? `${pathname}index.html` : pathname;
      if (!/^\/rusterize-(gallery|native-api)\/(index\.html|[a-z]+\.js|app\.wasm)$/.test(path)) {
        res.writeHead(404); res.end(); return;
      }
      file = resolve(root, 'dist/web', path.slice(1));
    }
    res.setHeader('Content-Type', { '.wasm': 'application/wasm', '.js': 'text/javascript', '.html': 'text/html; charset=utf-8' }[extname(file)]);
    res.end(await readFile(file));
  } catch (error) { res.writeHead(500); res.end(error.message); }
});
await new Promise(done => server.listen(0, '127.0.0.1', done));
const url = `http://127.0.0.1:${server.address().port}`;
const engine = process.env.WEB_BROWSER || 'chromium';
const browser = await ({ chromium, firefox, webkit }[engine]).launch({
  headless: true, ...(process.env.WEB_CHANNEL ? { channel: process.env.WEB_CHANNEL } : {}),
});
const errors = [];
async function ready(page, name = 'rusterize') {
  await page.bringToFront();
  try {
    await page.waitForFunction(name => window[name]?.frameCount >= 1 || window[name]?.stopped ||
      (document.querySelector('#error') && !document.querySelector('#error').hidden), name, { polling: 50, timeout: 15000 });
    const state = await page.evaluate(name => ({ frames: window[name]?.frameCount, stopped: window[name]?.stopped,
      error: document.querySelector('#error')?.textContent }), name);
    assert.ok(state.frames >= 1 && !state.stopped, JSON.stringify(state));
  } catch (error) {
    console.error('Startup diagnostics:', await page.evaluate(name => ({ visibility: document.visibilityState,
      readyState: document.readyState, frames: window[name]?.frameCount, stopped: window[name]?.stopped,
      error: document.querySelector('#error')?.textContent }), name));
    throw error;
  }
}
try {
  const context = await browser.newContext({ viewport: { width: 960, height: 680 } });
  context.setDefaultTimeout(15000);
  const page = await context.newPage();
  page.on('pageerror', error => errors.push(String(error)));
  page.on('console', message => { if (message.type() === 'error' && !message.text().includes('404')) errors.push(message.text()); });
  // Capture instantiated exports only in the test harness, not in the public API.
  await page.addInitScript(() => {
    // Regression: RAF's origin must not be mixed with performance.now() at mount.
    // The browser can deliver a first timestamp slightly earlier than mount.
    const raf = window.requestAnimationFrame.bind(window);
    window.requestAnimationFrame = callback => raf(time => callback(time - 1000));
    const instantiate = WebAssembly.instantiate;
    WebAssembly.instantiate = async (...args) => {
      const result = await instantiate(...args);
      window.testExports = (result.instance ?? result).exports;
      return result;
    };
    const fillText = CanvasRenderingContext2D.prototype.fillText;
    window.drawnText = [];
    CanvasRenderingContext2D.prototype.fillText = function (text, ...args) {
      window.drawnText.push(text);
      return fillText.call(this, text, ...args);
    };
  });
  await page.goto(`${url}/rusterize-gallery/`);
  await ready(page);
  assert.match(await page.title(), /Студия форм/);
  const initial = await page.evaluate(() => window.rusterize.frameCount);
  await page.waitForTimeout(180);
  assert.equal(await page.evaluate(() => window.rusterize.frameCount), initial, 'idle must not render');
  await page.mouse.click(820, 475);
  await page.waitForFunction(() => window.drawnText.includes('Нажатия: 1'));
  await page.keyboard.press('Space');
  await page.waitForFunction(() => window.drawnText.includes('Нажатия: 2'));
  await page.mouse.click(785, 185);
  await page.mouse.move(300, 300); await page.mouse.down(); await page.mouse.move(420, 350); await page.mouse.up();
  await page.mouse.click(85, 98);
  const running = await page.evaluate(() => window.rusterize.frameCount);
  await page.waitForFunction(n => window.rusterize.frameCount > n + 3, running);
  await page.mouse.click(85, 98);
  await page.waitForTimeout(80);
  const paused = await page.evaluate(() => window.rusterize.frameCount);
  await page.waitForTimeout(150);
  assert.equal(await page.evaluate(() => window.rusterize.frameCount), paused);
  await page.screenshot({ path: resolve(out, `gallery-${engine}.png`) });
  await page.setViewportSize({ width: 393, height: 852 });
  await page.waitForFunction(() => document.querySelector('canvas').width === 393);
  await page.screenshot({ path: resolve(out, `gallery-mobile-${engine}.png`) });
  await page.goto(`${url}/rusterize-native-api/`);
  await ready(page);
  assert.ok(await page.evaluate(() => window.drawnText.some(t => t.includes('Ёж идёт'))));
  await page.screenshot({ path: resolve(out, `text-${engine}.png`) });

  await page.setViewportSize({ width: 960, height: 680 });
  await page.goto(`${url}/checks.html`);
  await page.evaluate(async () => {
    const { start } = await import('/rusterize-gallery/rusterize.js');
    window.failures = [];
    // Drag outside the canvas, but keep the pointer inside the browser viewport.
    document.querySelector('canvas').style.width = '320px';
    window.app = await start(document.querySelector('canvas'), '/checks.wasm', { onError: e => window.failures.push(e.message) });
  });
  await ready(page, 'app');
  const metric = n => page.evaluate(n => window.testExports.web_check_metric(n), n);
  assert.equal(await metric(0), 1, 'Rust text/capability checks completed');
  assert.equal(await page.title(), 'Rusterize Web checks — Ё');
  const pixels = await page.evaluate(() => {
    const ctx = document.querySelector('canvas').getContext('2d');
    return [[5,5],[35,5],[45,5],[70,10],[60,0],[100,10],[126,8],[152,8],[1,40],[18,40],[40,40]]
      .map(([x,y]) => [...ctx.getImageData(x,y,1,1).data]);
  });
  assert.deepEqual(pixels.slice(0,8), [[255,0,0,255],[0,255,0,255],[0,0,0,0],[0,0,255,255],[0,0,0,0],[255,255,0,255],[0,255,255,255],[0,0,0,255]]);
  assert.ok(pixels[8][0] < 40 && pixels[9][0] > 220, 'gradient endpoints');
  assert.deepEqual(pixels[10], [255,0,255,128], 'RGBA and opacity');
  const beforeReset=await page.evaluate(()=>window.app.frameCount);
  await page.evaluate(()=>window.app.resetResources());
  await page.waitForFunction(n=>window.app.frameCount>n,beforeReset);
  assert.deepEqual(await page.evaluate(()=>[...document.querySelector('canvas').getContext('2d').getImageData(40,40,1,1).data]),[255,0,255,128],'reset reloads bitmap resources');
  await page.mouse.move(10, 10); await page.mouse.down(); await page.mouse.move(450, 20); await page.mouse.up();
  assert.equal(await metric(5), 1); assert.equal(await metric(7), 1);
  assert.equal(await metric(9), 0); assert.equal(await metric(10), 450);
  await page.keyboard.press('Shift+ArrowRight');
  await page.keyboard.press('Space');
  await page.waitForFunction(() => window.testExports.web_check_metric(20) === 1);
  assert.equal(await metric(19), 1, 'cursor request succeeded');
  assert.equal(await page.locator('canvas').evaluate(c => c.style.cursor), 'pointer');
  await page.locator('canvas').dispatchEvent('keydown', { key: 'Ё', code: 'KeyT', repeat: true, shiftKey: true });
  assert.equal(await metric(16), 'Ё'.codePointAt(0));
  assert.equal(await metric(14), 1); assert.equal(await metric(15), 1);
  await page.locator('canvas').dispatchEvent('wheel', { deltaY: 3, deltaMode: 1 });
  assert.equal(await metric(17), 120);
  await page.locator('canvas').evaluate(canvas => { canvas.style.width = '100vw'; });
  await page.setViewportSize({ width: 700, height: 500 });
  await page.waitForFunction(() => window.testExports.web_check_metric(2) === 700);
  await page.keyboard.press('Enter');
  await page.waitForFunction(() => window.testExports.web_check_metric(21) > 3);
  await page.evaluate(() => {
    Object.defineProperty(document, 'hidden', { configurable: true, value: true });
    document.dispatchEvent(new Event('visibilitychange'));
  });
  assert.equal(await metric(1), 1);
  const suspended = await metric(23);
  await page.waitForTimeout(120);
  assert.equal(await metric(23), suspended);
  await page.evaluate(() => {
    Object.defineProperty(document, 'hidden', { configurable: true, value: false });
    document.dispatchEvent(new Event('visibilitychange'));
  });
  await page.waitForFunction(() => window.testExports.web_check_metric(0) === 2 && window.testExports.web_check_metric(23) > 1);
  await page.keyboard.press('Tab');
  await page.mouse.move(10,10); await page.mouse.down();
  await page.evaluate(() => window.dispatchEvent(new Event('blur')));
  await page.mouse.up();
  assert.ok(await metric(8) >= 1, 'blur cancels active pointer');
  await page.keyboard.press('Escape');
  assert.equal(await page.evaluate(() => window.app.stopped), true);
  const keyCount = await metric(12);
  await page.keyboard.press('Space');
  assert.equal(await metric(12), keyCount, 'destroy removes listeners');

  // A second independent instance can mount on the same canvas after destroy.
  await page.evaluate(async () => {
    const { start } = await import('/rusterize-gallery/rusterize.js');
    window.app = await start(document.querySelector('canvas'), '/checks.wasm', { onError: e => window.failures.push(e.message) });
  });
  await ready(page, 'app');
  await page.keyboard.press('Delete');
  await page.waitForFunction(() => window.app.stopped);
  assert.equal(await page.evaluate(() => window.failures.length), 1, 'invalid Rust frame stops host');

  // Decoder must reject version/opcode/truncation errors, not silently skip them.
  const rejected = await page.evaluate(async () => {
    const { Renderer } = await import('/rusterize-gallery/rusterize.js');
    const r = new Renderer(document.createElement('canvas'));
    return [[82,90,48,49,0,0,0,0],[82,90,48,50,1,0,0,0,255,0,0,0],[82,90,48,50,1,0,0,0,1,0,0,0]].map(data => {
      try { r.render(new Uint8Array(data)); return false; } catch { return true; }
    });
  });
  assert.deepEqual(rejected, [true,true,true]);
  await context.close();

  const mobile = await browser.newContext({ viewport: { width: 393, height: 852 }, deviceScaleFactor: 2, hasTouch: true });
  mobile.setDefaultTimeout(15000);
  const touch = await mobile.newPage();
  touch.on('pageerror', e => errors.push(String(e)));
  await touch.goto(`${url}/rusterize-gallery/`);
  await ready(touch);
  assert.equal(await touch.locator('canvas').getAttribute('width'), '786');
  await touch.touchscreen.tap(309, 598);
  await touch.screenshot({ path: resolve(out, `gallery-dpi2-${engine}.png`) });
  await mobile.close();
  assert.deepEqual(errors, []);
  console.log(`WEB_SMOKE_OK ${process.env.WEB_CHANNEL || engine} ${browser.version()}: Rust/WASM, pixels, text, input, resize, DPI, lifecycle, failures, cleanup`);
} catch (error) {
  console.error('Browser diagnostics:', errors);
  throw error;
} finally {
  await browser.close();
  await new Promise(done => server.close(done));
}
