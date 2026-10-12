import { start } from './rusterize.js';

const canvas = document.querySelector('#app');
const root = document.documentElement;
const controls = [...document.querySelectorAll('[data-control]')];
const reduced = matchMedia('(prefers-reduced-motion: reduce)');
const systemDark = matchMedia('(prefers-color-scheme: dark)');
let savedTheme;
try { savedTheme = localStorage.getItem('rusterize.theme'); } catch { /* Storage is optional. */ }
let dark = savedTheme ? savedTheme === 'dark' : systemDark.matches;
let paused = reduced.matches;
let app, raf = 0, last = 0, elapsed = 0, noticeTimer, demoY = 0, startY = 0;
let currentMode = 0;

function theme() {
  root.dataset.theme = dark ? 'dark' : 'light';
  document.querySelector('meta[name="theme-color"]').content = dark ? '#151b17' : '#f8f8f2';
  document.querySelector('#theme').setAttribute('aria-label', dark ? 'Включить светлую тему' : 'Включить тёмную тему');
  for (const button of document.querySelectorAll('#motion, #demo-motion')) {
    button.setAttribute('aria-pressed', String(paused));
    button.textContent = paused ? 'Включить движение' : 'Остановить движение';
  }
}
theme();
function notice(message) {
  const element = document.querySelector('#notice');
  element.textContent = message;
  element.hidden = false;
  clearTimeout(noticeTimer);
  noticeTimer = setTimeout(() => { element.hidden = true; }, 3200);
}
function environment() {
  if (app && !app.stopped) app.callExport('app_environment', scrollY, elapsed, +dark, +paused);
}
function visibleMotion() { return scrollY < demoY + 620 && !document.hidden && !paused; }
function tick(now) {
  raf = 0;
  if (last && visibleMotion()) elapsed += Math.min((now - last) / 1000, 0.05);
  last = now;
  environment();
  if (visibleMotion()) raf = requestAnimationFrame(tick);
  else last = 0;
}
function wake() { environment(); if (!raf && visibleMotion()) raf = requestAnimationFrame(tick); }
function layout() {
  const width = document.documentElement.clientWidth;
  const metric = (element, component) => app.callExport('app_layout', width, element, component);
  document.body.style.height = `${metric(100, 0)}px`;
  demoY = metric(100, 1); startY = metric(100, 2);
  document.querySelector('#demo').style.cssText = `position:absolute;top:${demoY}px;left:0;width:100%`;
  document.querySelector('#start').style.cssText = `position:absolute;top:${startY}px;left:0;width:100%`;
  for (const element of controls) {
    const id = Number(element.dataset.control);
    const [x, y, w, h] = [0, 1, 2, 3].map(part => metric(id, part));
    element.hidden = w === 0;
    // Controls are rooted in the document; section anchors retain semantic grouping.
    const parentOffset = element.closest('#demo') ? demoY : element.closest('#start') ? startY : 0;
    Object.assign(element.style, { left: `${x}px`, top: `${y - parentOffset}px`, width: `${w}px`, height: `${h}px` });
  }
  wake();
}
function action(kind, value) { app?.callExport('app_action', kind, value); wake(); }
function selectMode(index, focus = false) {
  currentMode = index;
  const tabs = [...document.querySelectorAll('[role="tab"]')];
  tabs.forEach((tab, i) => { tab.setAttribute('aria-selected', String(index === i)); tab.tabIndex = index === i ? 0 : -1; });
  if (focus) tabs[index].focus();
  const panel = document.querySelector('#demo-description');
  panel.textContent = ['Скругление, градиент и отсечение.', 'Перетащите центр орбит. Скорость меняется ползунком.', 'Unicode и переносы строк. Меняйте ширину текстового блока.', 'Живой WGSL-шейдер, выполненный через WebGL 2.'][index];
  panel.setAttribute('aria-labelledby', `tab-${index}`);
  document.querySelector('#amount').setAttribute('aria-label', ['Скругление', 'Скорость движения', 'Ширина текста', 'Частота волн'][index]);
  action(0, index);
}
document.querySelectorAll('[role="tab"]').forEach((tab, index) => {
  tab.addEventListener('click', () => selectMode(index));
  tab.addEventListener('keydown', event => {
    let next;
    if (event.key === 'ArrowRight') next = (currentMode + 1) % 4;
    if (event.key === 'ArrowLeft') next = (currentMode + 3) % 4;
    if (event.key === 'Home') next = 0;
    if (event.key === 'End') next = 3;
    if (next !== undefined) { event.preventDefault(); selectMode(next, true); }
  });
});
document.querySelector('#amount').addEventListener('input', event => action(1, Number(event.target.value) / 100));
document.querySelectorAll('.color').forEach((button, index, buttons) => button.addEventListener('click', () => {
  buttons.forEach((b, i) => b.setAttribute('aria-pressed', String(i === index)));
  action(2, index);
}));
document.querySelector('#theme').addEventListener('click', () => {
  dark = !dark; savedTheme = dark ? 'dark' : 'light';
  try { localStorage.setItem('rusterize.theme', savedTheme); } catch { /* Storage is optional. */ }
  theme(); wake();
});
document.querySelectorAll('#motion, #demo-motion').forEach(button => button.addEventListener('click', () => { paused = !paused; theme(); wake(); }));
systemDark.addEventListener('change', event => { if (!savedTheme) { dark = event.matches; theme(); wake(); } });
reduced.addEventListener('change', event => { paused = event.matches; theme(); wake(); });
addEventListener('storage', event => {
  if (event.key === 'rusterize.theme') { savedTheme = event.newValue; dark = savedTheme ? savedTheme === 'dark' : systemDark.matches; theme(); wake(); }
});
const code = `use rusterize::*;

#[derive(Default)]
pub struct App;

impl Application for App {
    fn draw(&mut self, c: &mut Canvas<'_>, _: Viewport) {
        c.clear(Color::hex(0xf8f8f2));
        c.circle((120., 120.), 48., Color::hex(0xe65e38));
    }
}
rusterize::export_app!(App);`;
document.querySelector('#copy').addEventListener('click', async () => {
  try { await navigator.clipboard.writeText(code); notice('Пример скопирован'); }
  catch { notice('Не удалось скопировать. Пример доступен в репозитории.'); }
});
// Native document scroll and tab order stay with the browser.
canvas.addEventListener('wheel', event => event.stopImmediatePropagation(), { capture: true, passive: true });
canvas.addEventListener('keydown', event => event.stopImmediatePropagation(), true);
function fail(error) {
  console.error(error);
  if (raf) cancelAnimationFrame(raf);
  app?.destroy();
  root.classList.remove('ready');
  document.body.style.height = '';
  document.querySelectorAll('#demo, #start').forEach(el => el.removeAttribute('style'));
  controls.forEach(el => { el.removeAttribute('style'); el.hidden = false; });
  const loading = document.querySelector('#loading');
  loading.textContent = 'Демонстрация недоступна. Описание и исходный код — на странице.';
  loading.hidden = false;
}
try {
  document.querySelector('#loading').hidden = false;
  await Promise.all([document.fonts.load('400 16px Manrope'), document.fonts.load('700 16px Manrope')]);
  root.classList.add('ready');
  app = await start(canvas, new URL('./app.wasm', import.meta.url), { onError: fail });
  canvas.style.touchAction = 'pan-y';
  canvas.tabIndex = -1;
  window.rusterize = app;
  layout();
  document.querySelector('#loading').hidden = true;
  addEventListener('resize', layout);
  addEventListener('scroll', wake, { passive: true });
  document.addEventListener('visibilitychange', () => { last = 0; wake(); });
  if (location.hash === '#demo' || location.hash === '#start') document.querySelector(location.hash).scrollIntoView();
} catch (error) { fail(error); }
