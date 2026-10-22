import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile, mkdir } from 'node:fs/promises';
import { resolve, extname, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
const { chromium, firefox, webkit } = createRequire(import.meta.url)('playwright');
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const out = resolve(root, 'target/website-check');
await mkdir(out, { recursive: true });
const base = resolve(root, 'dist/web/rusterize-website');
const server = createServer(async (req, res) => {
  try {
    const path = decodeURIComponent(new URL(req.url, 'http://localhost').pathname).replace(/^\/Rusterize\//, '');
    const file = resolve(base, path || 'index.html');
    if (!file.startsWith(base + '\\') && !file.startsWith(base + '/')) { res.writeHead(404).end(); return; }
    res.setHeader('Content-Type', { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript', '.wasm': 'application/wasm', '.css': 'text/css', '.svg': 'image/svg+xml', '.ttf': 'font/ttf' }[extname(file)] ?? 'application/octet-stream');
    // Match the production policy, including the explicitly scoped WASM permission.
    res.setHeader('Content-Security-Policy', "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'");
    res.end(await readFile(file));
  } catch { res.writeHead(404).end(); }
});
await new Promise(done => server.listen(0, '127.0.0.1', done));
const url = process.env.WEBSITE_URL || `http://127.0.0.1:${server.address().port}/Rusterize/`;
const browserType = { chromium, firefox, webkit }[process.env.WEB_BROWSER || 'chromium'];
let browser;
try {
  browser = await browserType.launch({ headless: true, ...(process.env.WEB_CHANNEL ? { channel: process.env.WEB_CHANNEL } : {}) });
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 }, deviceScaleFactor: 1, reducedMotion: 'reduce', colorScheme: 'light' });
  page.setDefaultTimeout(15000);
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
  await page.goto(url);
  await page.waitForFunction(() => window.rusterize?.frameCount > 0);
  assert.equal(await page.locator('#loading').isHidden(), true);
  await page.screenshot({ path: resolve(out, 'desktop-light.png') });
  let frames = await page.evaluate(() => window.rusterize.frameCount);
  await page.waitForTimeout(250);
  assert.equal(await page.evaluate(() => window.rusterize.frameCount), frames, 'Reduced motion must idle');
  await page.locator('[data-control="4"]').click();
  await page.waitForTimeout(250);
  assert.ok(await page.evaluate(() => scrollY > 500));
  await page.screenshot({ path: resolve(out, 'demo-graphics.png') });
  const canvasBefore = await page.locator('canvas').screenshot();
  await page.locator('#amount').fill('95');
  await page.locator('#amount').dispatchEvent('input');
  await page.waitForTimeout(80);
  assert.notDeepEqual(await page.locator('canvas').screenshot(), canvasBefore, 'Slider must change Rust rendering');
  await page.getByRole('tab', { name: 'Движение', exact: true }).click();
  const beforeDrag = await page.locator('canvas').screenshot();
  const stage = await page.locator('#tab-1').boundingBox();
  await page.mouse.move(720, stage.y + 170);
  await page.mouse.down();
  await page.mouse.move(790, stage.y + 190, { steps: 5 });
  await page.mouse.up();
  await page.waitForTimeout(80);
  assert.notDeepEqual(await page.locator('canvas').screenshot(), beforeDrag, 'Dragging moves the orbit center');
  for (const [mode, title] of ['Графика', 'Движение', 'Текст', 'WGSL'].entries()) {
    await page.getByRole('tab', { name: title, exact: true }).click();
    await page.waitForTimeout(100);
    assert.equal(await page.locator(`#tab-${mode}`).getAttribute('aria-selected'), 'true');
    assert.equal(await page.evaluate(() => window.rusterize.stopped), false);
    await page.screenshot({ path: resolve(out, `demo-${mode}.png`) });
  }
  const beforeColor = await page.locator('canvas').screenshot();
  await page.getByRole('button', { name: 'Сиреневый', exact: true }).click();
  await page.waitForTimeout(80);
  assert.notDeepEqual(await page.locator('canvas').screenshot(), beforeColor, 'Palette changes the WGSL demo');
  await page.getByRole('tab', { name: 'WGSL', exact: true }).press('ArrowLeft');
  assert.equal(await page.locator('#tab-2').getAttribute('aria-selected'), 'true');
  await page.evaluate(() => { window.clipboardText = ''; Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async value => { window.clipboardText = value; } } }); });
  await page.locator('#copy').click();
  assert.match(await page.evaluate(() => window.clipboardText), /export_app!\(App\)/);
  await page.screenshot({ path: resolve(out, 'start.png') });
  await page.evaluate(() => scrollTo({ top: 0, behavior: 'instant' }));
  await page.locator('#theme').click();
  assert.equal(await page.locator('html').getAttribute('data-theme'), 'dark');
  await page.screenshot({ path: resolve(out, 'desktop-dark.png') });
  await page.reload();
  await page.waitForFunction(() => window.rusterize?.frameCount > 0);
  assert.equal(await page.locator('html').getAttribute('data-theme'), 'dark');
  await page.locator('#motion').click();
  frames = await page.evaluate(() => window.rusterize.frameCount);
  await page.waitForTimeout(250);
  assert.ok(await page.evaluate(() => window.rusterize.frameCount) > frames + 1, 'Motion must animate');
  await page.locator('#motion').click();
  for (const width of [320, 390, 759, 760, 1024]) {
    await page.setViewportSize({ width, height: 844 });
    await page.waitForTimeout(100);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, `Overflow at ${width}`);
    for (const theme of ['light', 'dark']) {
      if (await page.locator('html').getAttribute('data-theme') !== theme) await page.locator('#theme').click();
      await page.evaluate(() => scrollTo({ top: 0, behavior: 'instant' }));
      await page.waitForTimeout(60);
      await page.screenshot({ path: resolve(out, `${width}-${theme}.png`) });
    }
  }
  await page.setViewportSize({ width: 390, height: 844 });
  await page.locator('[data-control="4"]').click();
  await page.getByRole('tab', { name: 'Текст', exact: true }).click();
  await page.screenshot({ path: resolve(out, 'mobile-demo.png') });
  const denied = await page.evaluate(() => {
    try { window.rusterize.callExport('rusterize_destroy', 0); return false; } catch { return true; }
  });
  assert.ok(denied);
  await page.evaluate(() => window.rusterize.destroy());
  assert.ok(await page.evaluate(() => { try { window.rusterize.callExport('app_action', 0, 0); return false; } catch { return true; } }));
  const fallback = await browser.newPage({ javaScriptEnabled: false });
  fallback.setDefaultTimeout(15000);
  await fallback.goto(url);
  assert.ok(await fallback.getByRole('heading', { level: 1 }).isVisible());
  assert.ok(await fallback.getByText('git clone', { exact: false }).isVisible());
  const mobile = await browser.newPage({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true, reducedMotion: 'reduce' });
  mobile.setDefaultTimeout(15000);
  mobile.on('pageerror', e => errors.push(e.message));
  mobile.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
  await mobile.goto(url);
  await mobile.waitForFunction(() => window.rusterize?.frameCount > 0);
  await mobile.locator('[data-control="4"]').tap();
  assert.ok(await mobile.evaluate(() => scrollY > 500));
  await mobile.getByRole('tab', { name: 'WGSL', exact: true }).tap();
  await mobile.screenshot({ path: resolve(out, 'mobile-2x.png') });
  assert.equal(await mobile.evaluate(() => window.rusterize.stopped), false);
  assert.deepEqual(errors, [], 'Browser errors');
  console.log(`WEBSITE_OK ${url}; responsive, themes, tabs, slider, WGSL, clipboard, motion, export isolation, no-JS`);
} finally {
  await browser?.close();
  await new Promise(done => server.close(done));
}
