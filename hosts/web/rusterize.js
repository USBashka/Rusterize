// RZ02 browser host. No runtime dependencies or generated JavaScript bindings.
import { Reader, utf8, encoder } from './wire.js';
import { BrowserServices } from './services.js';
const FAILED = 0x80000000;

function textStyle(ctx, t) {
  const family = t.font ? JSON.stringify(t.font) : ['sans-serif', 'serif', 'monospace'][t.family];
  if (!family || t.size <= 0 || t.width <= 0 || t.wrap > 2 || t.align > 2 || t.direction > 1)
    throw new Error('Invalid Rusterize text descriptor');
  ctx.font = `${t.flags & 2 ? 'italic ' : ''}${t.flags & 1 ? 'bold ' : ''}${t.size}px ${family}`;
  ctx.direction = t.direction ? 'rtl' : 'ltr';
  ctx.textAlign = 'left';
  ctx.textBaseline = 'alphabetic';
  ctx.fillStyle = t.color;
}

// Both the synchronous Rust text service and replay use these exact line breaks.
// Canvas shapes each complete line; Segmenter protects grapheme clusters.
export class TextEngine {
  constructor(context) {
    this.context = context;
    this.graphemes = new Intl.Segmenter(undefined, { granularity: 'grapheme' });
    this.words = new Intl.Segmenter(undefined, { granularity: 'word' });
    this.cache = new Map();
  }
  layout(t) {
    const key = JSON.stringify(t);
    if (this.cache.has(key)) return this.cache.get(key);
    const ctx = this.context;
    textStyle(ctx, t);
    const fontMetrics = ctx.measureText('Mg');
    const ascent = fontMetrics.fontBoundingBoxAscent ?? t.size * 0.8;
    const descent = fontMetrics.fontBoundingBoxDescent ?? t.size * 0.2;
    const height = Math.max(t.size, ascent + descent);
    const lines = [];
    let byteOffset = 0;
    const append = (text, suffix = '') => {
      const end = byteOffset + encoder.encode(text + suffix).length;
      const width = ctx.measureText(text.replace(/\s+$/u, '')).width;
      const trailing = ctx.measureText(text).width;
      const top = lines.length * height;
      lines.push({ text, start: byteOffset, end, top, height, baseline: top + ascent, width, trailing });
      byteOffset = end;
    };
    const paragraphs = t.text.split(/(\r\n|[\n\r\u2028\u2029])/u);
    for (let p = 0; p < paragraphs.length; p += 2) {
      const paragraph = paragraphs[p], newline = paragraphs[p + 1] ?? '';
      if (!t.wrap || ctx.measureText(paragraph).width <= t.width || !paragraph) {
        append(paragraph, newline);
        continue;
      }
      const ends = [...this.graphemes.segment(paragraph)].map(s => s.index + s.segment.length);
      const wordEnds = new Set([...this.words.segment(paragraph)].map(s => s.index + s.segment.length));
      let first = 0, start = 0;
      while (first < ends.length) {
        const fits = index => ctx.measureText(paragraph.slice(start, ends[index])).width <= t.width;
        let fit = first, step = 1, high = first;
        // Grow a local search window: measuring half of the remaining document
        // for every short line would make narrow layouts quadratic.
        if (fits(first)) {
          high = first + step;
          while (high < ends.length && fits(high)) {
            fit = high; step *= 2; high = first + step;
          }
        }
        let low = fit + 1;
        high = Math.min(high, ends.length - 1);
        // Always place at least one whole grapheme, even when it exceeds width.
        while (low <= high) {
          const mid = (low + high) >>> 1;
          if (fits(mid)) {
            fit = mid; low = mid + 1;
          } else high = mid - 1;
        }
        if (t.wrap === 1 && fit < ends.length - 1) {
          for (let j = fit; j >= first; j--) {
            if (wordEnds.has(ends[j])) { fit = j; break; }
          }
        }
        const end = ends[fit];
        append(paragraph.slice(start, end), end === paragraph.length ? newline : '');
        start = end;
        first = fit + 1;
      }
    }
    const result = {
      lines, width: lines.reduce((width, line) => Math.max(width, line.width), 0),
      trailing: lines.reduce((width, line) => Math.max(width, line.trailing), 0), height: lines.length * height, baseline: ascent,
    };
    // Bound retained descriptors; very large documents are not cached.
    if (key.length < 32768) {
      if (this.cache.size >= 128) this.cache.delete(this.cache.keys().next().value);
      this.cache.set(key, result);
    }
    return result;
  }
  response(t) {
    const l = this.layout(t);
    const bytes = new Uint8Array(20 + l.lines.length * 20);
    const v = new DataView(bytes.buffer);
    [l.width, l.trailing, l.height, l.baseline].forEach((n, i) => v.setFloat32(i * 4, n, true));
    v.setUint32(16, l.lines.length, true);
    l.lines.forEach((line, i) => {
      const p = 20 + i * 20;
      v.setUint32(p, line.start, true);
      v.setUint32(p + 4, line.end, true);
      [line.top, line.height, line.baseline].forEach((n, j) => v.setFloat32(p + 8 + j * 4, n, true));
    });
    return bytes;
  }
  draw(ctx, t, x, y, multiline) {
    textStyle(ctx, t);
    const lines = multiline ? this.layout(t).lines : [{ text: t.text, width: ctx.measureText(t.text).width, baseline: 0 }];
    for (const line of lines) {
      const dx = !multiline || !t.align ? 0 : (t.width - line.width) / (t.align === 1 ? 2 : 1);
      const baseline = y + line.baseline;
      ctx.fillText(line.text, x + dx, baseline);
      if (t.flags & 4) ctx.fillRect(x + dx, baseline + t.size * 0.1, line.width, Math.max(1, t.size / 16));
      if (t.flags & 8) ctx.fillRect(x + dx, baseline - t.size * 0.3, line.width, Math.max(1, t.size / 16));
    }
  }
}

function shape(r) {
  const kind = r.u32(), path = new Path2D();
  if (kind <= 2) {
    const [x, y, w, h] = r.rect();
    if (w < 0 || h < 0) throw new Error('Invalid Rusterize shape');
    if (kind === 0) path.rect(x, y, w, h);
    else if (kind === 1) path.roundRect(x, y, w, h, r.f32());
    else path.ellipse(x + w / 2, y + h / 2, w / 2, h / 2, 0, 0, Math.PI * 2);
  } else if (kind === 3) {
    const count = r.u32();
    for (let i = 0; i < count; i++) {
      switch (r.u32()) {
        case 0: path.moveTo(...r.point()); break;
        case 1: path.lineTo(...r.point()); break;
        case 2: path.bezierCurveTo(...r.point(), ...r.point(), ...r.point()); break;
        case 3: path.closePath(); break;
        default: throw new Error('Unknown Rusterize path segment');
      }
    }
  } else throw new Error('Unknown Rusterize shape');
  return path;
}

function paint(r, ctx) {
  const kind = r.u32();
  if (kind === 0) return r.color();
  if (kind !== 1) throw new Error('Unknown Rusterize paint');
  const gradient = ctx.createLinearGradient(...r.point(), ...r.point());
  gradient.addColorStop(0, r.color());
  gradient.addColorStop(1, r.color());
  return gradient;
}

export class Renderer {
  constructor(canvas) {
    this.canvas = canvas;
    this.context = canvas.getContext('2d');
    if (!this.context) throw new Error('Canvas 2D is unavailable');
    const measureCanvas = document.createElement('canvas');
    this.text = new TextEngine(measureCanvas.getContext('2d'));
    this.images = new Map();
  }
  render(bytes, scale = 1) {
    const r = new Reader(bytes), ctx = this.context, used = new Set();
    if (utf8.decode(r.take(4)) !== 'RZ02') throw new Error('Unsupported Rusterize display-list version');
    const count = r.u32();
    let depth = 0;
    ctx.save();
    try {
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
      ctx.setTransform(scale, 0, 0, scale, 0, 0);
      ctx.globalAlpha = 1;
      ctx.globalCompositeOperation = 'source-over';
      ctx.lineCap = 'butt'; ctx.lineJoin = 'miter'; ctx.miterLimit = 10;
      ctx.imageSmoothingEnabled = true;
      for (let i = 0; i < count; i++) {
        switch (r.u32()) {
          case 1:
            if (i !== 0) throw new Error('Clear must be the first command');
            ctx.fillStyle = r.color();
            ctx.fillRect(0, 0, this.canvas.width / scale, this.canvas.height / scale);
            break;
          case 2: ctx.save(); depth++; break;
          case 3:
            if (!depth) throw new Error('Unbalanced Rusterize restore');
            ctx.restore(); depth--; break;
          case 4: ctx.transform(...r.rect(), ...r.point()); break;
          case 5: ctx.clip(shape(r)); break;
          case 6: { const p = shape(r); ctx.fillStyle = paint(r, ctx); ctx.fill(p); break; }
          case 7: {
            const p = shape(r); ctx.strokeStyle = paint(r, ctx); ctx.lineWidth = r.f32(); ctx.stroke(p); break;
          }
          case 8: case 10: {
            const multiline = r.view.getUint32(r.offset - 4, true) === 10;
            const [x, y] = r.point(); this.text.draw(ctx, r.text(), x, y, multiline); break;
          }
          case 9: {
            const id = r.u64(), width = r.u32(), height = r.u32(), destination = r.rect(), opacity = r.f32();
            const pixels = r.take(r.u32());
            if (!width || !height || width * height * 4 !== pixels.length || pixels.length > 64 * 1024 * 1024)
              throw new Error('Invalid Rusterize image');
            let image = this.images.get(id);
            if (!image) {
              image = document.createElement('canvas'); image.width = width; image.height = height;
              image.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(pixels), width, height), 0, 0);
              this.images.set(id, image);
            }
            used.add(id);
            ctx.globalAlpha = opacity; ctx.drawImage(image, ...destination); ctx.globalAlpha = 1;
            break;
          }
          default: throw new Error('Unknown Rusterize drawing command');
        }
      }
      if (depth) throw new Error('Unbalanced Rusterize save');
      r.done();
    } finally {
      while (depth-- > 0) ctx.restore();
      ctx.restore();
      for (const id of this.images.keys()) if (!used.has(id)) this.images.delete(id);
    }
  }
}

/** Mount one independent WASM instance. The host exclusively owns the canvas.
 * destroy() removes listeners, observers and animation frames. requestRedraw()
 * wakes an idle app after an embedding page changes its external resources.
 */
export async function start(canvas, wasmUrl, { onError = console.error, onExit = () => {} } = {}) {
  if (!(canvas instanceof HTMLCanvasElement)) throw new Error('Expected an HTML canvas');
  if (!globalThis.Intl?.Segmenter) throw new Error('This browser needs Intl.Segmenter support');
  await document.fonts.ready;
  const renderer = new Renderer(canvas);
  let exports, reply = new Uint8Array(), id, destroyed = false, raf = 0, dirty = true;
  let width = 0, height = 0, scale = 1, titleRevision = -1, frames = 0;
  let focused = false;
  const listeners = [], contacts = new Map();
  const services = new BrowserServices(canvas, renderer.text, wake);
  const memory = (pointer, length) => {
    const buffer = exports.memory.buffer;
    pointer >>>= 0;
    if (length < 0 || pointer + length > buffer.byteLength) throw new Error('Invalid WASM memory range');
    return new Uint8Array(buffer, pointer, length);
  };
  const cstring = pointer => {
    const bytes = memory(pointer, exports.memory.buffer.byteLength - (pointer >>> 0));
    const end = bytes.indexOf(0);
    if (end < 0) throw new Error('Unterminated WASM string');
    return utf8.decode(bytes.subarray(0, end));
  };
  const imports = { rusterize: {
    service(pointer, length) {
      try {
        const payload = services.sync(memory(pointer, length));
        reply = new Uint8Array(4 + payload.length); reply.set(payload, 4);
      } catch (error) {
        const message = encoder.encode(String(error.message ?? error));
        reply = new Uint8Array(4 + message.length); reply[0] = 1; reply.set(message, 4);
      }
      return reply.length;
    },
    service_read(pointer, length) {
      if (length !== reply.length) throw new Error('Browser service response length mismatch');
      memory(pointer, length).set(reply); reply = new Uint8Array();
    },
    request(app, request, pointer, length) { services.request(app, request, memory(pointer, length).slice()); },
    cancel_requests(app) { services.cancel(app); },
  } };
  try {
  const response = await fetch(wasmUrl);
  if (!response.ok) throw new Error(`WASM load failed: HTTP ${response.status}`);
  const result = await WebAssembly.instantiate(await response.arrayBuffer(), imports);
  exports = result.instance.exports;
  const required = ['create', 'destroy', 'status', 'frame', 'event', 'poll_native', 'title', 'window_option', 'data', 'len', 'error'];
  for (const name of required) if (typeof exports[`rusterize_${name}`] !== 'function')
    throw new Error(`Missing export: rusterize_${name}; use export_app!(App)`);
  if (!(exports.memory instanceof WebAssembly.Memory)) throw new Error('Missing WASM memory export');
  } catch (error) { services.destroy(); throw error; }

  const previous = { cursor: canvas.style.cursor, touchAction: canvas.style.touchAction, tabindex: canvas.getAttribute('tabindex') };
  let observer, dprQuery;
  function listen(target, name, callback, options) {
    const safe = event => { try { callback(event); } catch (error) { fail(error); } };
    target.addEventListener(name, safe, options);
    listeners.push(() => target.removeEventListener(name, safe, options));
  }
  function destroy() {
    if (destroyed) return;
    destroyed = true;
    if (raf) cancelAnimationFrame(raf);
    observer?.disconnect();
    dprQuery?.removeEventListener('change', dprChanged);
    for (const remove of listeners) remove();
    for (const pointerId of contacts.keys()) if (canvas.hasPointerCapture(pointerId)) canvas.releasePointerCapture(pointerId);
    contacts.clear();
    try { if (id !== undefined) exports.rusterize_destroy(id); }
    finally {
      services.destroy();
      renderer.images.clear(); renderer.text.cache.clear();
      canvas.style.cursor = previous.cursor; canvas.style.touchAction = previous.touchAction;
      if (previous.tabindex === null) canvas.removeAttribute('tabindex'); else canvas.setAttribute('tabindex', previous.tabindex);
    }
  }
  function fail(error) { try { destroy(); } catch { /* Rust may already have trapped. */ } onError(error); }
  function check(status) {
    if (status & FAILED) throw new Error(cstring(exports.rusterize_error(id)) || 'Rusterize application failed');
    if (status & 4) { syncTitle(); destroy(); onExit(); return false; }
    return true;
  }
  function syncTitle() {
    const revision = exports.rusterize_window_option(id, 8);
    if (revision !== titleRevision) {
      titleRevision = revision;
      document.title = cstring(exports.rusterize_title(id));
    }
  }
  function wake() {
    if (!destroyed && !document.hidden && !raf) raf = requestAnimationFrame(tick);
  }
  function poll() {
    const status = exports.rusterize_poll_native(id);
    if (check(status)) { syncTitle(); if (status & 3 || dirty) wake(); }
  }
  function event(kind, x = 0, y = 0, dx = 0, dy = 0, detail = 0, flags = 0) {
    if (destroyed) return;
    if (check(exports.rusterize_event(id, kind, x, y, dx, dy, detail, flags))) poll();
  }
  function resize() {
    services.update();
    const rect = canvas.getBoundingClientRect();
    const nextScale = window.devicePixelRatio || 1;
    if (width !== rect.width || height !== rect.height || scale !== nextScale) {
      width = rect.width; height = rect.height; scale = nextScale;
      // A hidden canvas is dormant, not a failed zero-sized backing store.
      const w = Math.max(1, Math.round(width * scale)), h = Math.max(1, Math.round(height * scale));
      if (w > 32768 || h > 32768 || w * h > 67108864) throw new Error('Canvas backing store is too large');
      canvas.width = w; canvas.height = h; dirty = true;
      wake();
    }
  }
  // RAF timestamps refer to the start of a rendering interval and can precede
  // performance.now() at mount. Anchor the clock to the first actual frame.
  let started;
  function tick(now) {
    raf = 0;
    if (destroyed || document.hidden) return;
    try {
      resize();
      if (width <= 0 || height <= 0) return;
      const status = exports.rusterize_poll_native(id);
      if (!check(status)) return;
      if (dirty || status & 3) {
        dirty = false;
        if (started === undefined) started = now;
        if (!check(exports.rusterize_frame(id, width, height, scale, (now - started) / 1000))) return;
        // No Rust call between taking this view and completing replay.
        renderer.render(memory(exports.rusterize_data(id), exports.rusterize_len(id)), scale);
        frames++;
      }
      poll();
    } catch (error) { fail(error); }
  }
  function position(e) {
    const r = canvas.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  }
  function cancelContacts() {
    for (const [pointerId, contact] of contacts) {
      contacts.delete(pointerId);
      event(4, ...contact.position, 0, 0, contact.id, 0);
      if (canvas.hasPointerCapture(pointerId)) canvas.releasePointerCapture(pointerId);
    }
  }
  function dprChanged() {
    dprQuery?.removeEventListener('change', dprChanged);
    dprQuery = matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
    dprQuery.addEventListener('change', dprChanged);
    try { resize(); } catch (error) { fail(error); }
  }
  try {
    id = exports.rusterize_create();
    if (!check(exports.rusterize_status(id))) throw new Error('Application exited during startup');
    canvas.tabIndex = canvas.tabIndex < 0 ? 0 : canvas.tabIndex;
    canvas.style.touchAction = 'none';
    for (const [name, kind] of [['pointerdown', 1], ['pointermove', 2], ['pointerup', 3], ['pointercancel', 4]]) {
      listen(canvas, name, e => {
        const pos = position(e), pointer = e.pointerType === 'mouse' ? 0 : e.pointerId;
        if (kind === 1) {
          canvas.focus({ preventScroll: true });
          if (destroyed) return;
          canvas.setPointerCapture(e.pointerId);
          contacts.set(e.pointerId, { id: pointer, position: pos });
        } else if (contacts.has(e.pointerId)) contacts.get(e.pointerId).position = pos;
        // Cancel must not activate a button; lost capture is handled separately.
        if (kind >= 3) contacts.delete(e.pointerId);
        event(kind, ...pos, 0, 0, pointer, e.buttons);
        if (kind >= 3 && canvas.hasPointerCapture(e.pointerId)) canvas.releasePointerCapture(e.pointerId);
        e.preventDefault();
      });
    }
    listen(canvas, 'lostpointercapture', e => {
      const contact = contacts.get(e.pointerId);
      if (contact) { contacts.delete(e.pointerId); event(4, ...contact.position, 0, 0, contact.id); }
    });
    listen(canvas, 'contextmenu', e => e.preventDefault());
    listen(canvas, 'wheel', e => {
      const unit = e.deltaMode === 1 ? 40 : e.deltaMode === 2 ? height : 1;
      event(5, ...position(e), e.deltaX * unit, e.deltaY * unit);
      e.preventDefault();
    }, { passive: false });
    const keys = { Enter: 13, ' ': 32, Escape: 27, Tab: 9, Backspace: 8, Delete: 46, ArrowLeft: 37, ArrowUp: 38, ArrowRight: 39, ArrowDown: 40, Home: 36, End: 35 };
    for (const [name, kind] of [['keydown', 6], ['keyup', 7]]) listen(canvas, name, e => {
      const flags = +e.shiftKey | (+e.ctrlKey << 1) | (+e.altKey << 2) | (+e.metaKey << 3) | (+e.repeat << 4);
      event(kind, 0, 0, 0, 0, keys[e.key] ?? e.keyCode, flags);
      if (kind === 6 && !e.isComposing && !e.ctrlKey && !e.metaKey && !e.altKey && [...e.key].length === 1)
        event(8, 0, 0, 0, 0, e.key.codePointAt(0));
      if (!e.ctrlKey && !e.metaKey && !e.altKey && keys[e.key]) e.preventDefault();
    });
    function focus(value) {
      if (focused === value) return;
      focused = value;
      if (!value) cancelContacts();
      event(9, 0, 0, 0, 0, +value);
    }
    listen(canvas, 'focus', () => focus(document.hasFocus()));
    listen(canvas, 'blur', () => focus(false));
    listen(window, 'blur', () => focus(false));
    listen(window, 'focus', () => focus(document.activeElement === canvas));
    listen(document, 'visibilitychange', () => {
      if (document.hidden) { cancelContacts(); if (raf) cancelAnimationFrame(raf); raf = 0; event(10); }
      else { event(11); dirty = true; wake(); }
    });
    listen(window, 'pagehide', e => { if (!e.persisted) destroy(); });
    listen(window, 'resize', resize);
    listen(window, 'scroll', () => services.update(), true);
    observer = new ResizeObserver(() => { try { resize(); } catch (error) { fail(error); } });
    observer.observe(canvas);
    dprChanged();
    if (document.hidden) event(10);
    syncTitle();
    wake();
  } catch (error) { try { destroy(); } catch { /* Preserve the startup error. */ } throw error; }
  return {
    destroy,
    requestRedraw() { dirty = true; wake(); },
    get frameCount() { return frames; },
    get stopped() { return destroyed; },
  };
}
