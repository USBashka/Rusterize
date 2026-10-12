export const utf8 = new TextDecoder('utf-8', { fatal: true });
export const encoder = new TextEncoder();

export class Reader {
  constructor(bytes) {
    this.bytes = bytes;
    this.view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    this.offset = 0;
  }
  take(length) {
    if (!Number.isInteger(length) || length < 0 || length > this.bytes.length - this.offset)
      throw new Error('Truncated Rusterize buffer');
    const result = this.bytes.subarray(this.offset, this.offset + length);
    this.offset += length;
    return result;
  }
  u32() { const at = this.offset; this.take(4); return this.view.getUint32(at, true); }
  u64() { const at = this.offset; this.take(8); return this.view.getBigUint64(at, true); }
  f32() {
    const at = this.offset;
    this.take(4);
    const value = this.view.getFloat32(at, true);
    if (!Number.isFinite(value)) throw new Error('Non-finite Rusterize coordinate');
    return value;
  }
  string() { return utf8.decode(this.take(this.u32())); }
  point() { return [this.f32(), this.f32()]; }
  rect() { return [...this.point(), ...this.point()]; }
  color() { const c = this.take(4); return `rgba(${c[0]},${c[1]},${c[2]},${c[3] / 255})`; }
  done() { if (this.offset !== this.bytes.length) throw new Error('Trailing Rusterize data'); }
  text() {
    return {
      size: this.f32(), color: this.color(), family: this.u32(), flags: this.u32(),
      font: this.string(), width: this.f32(), wrap: this.u32(), align: this.u32(),
      direction: this.u32(), text: this.string(),
    };
  }
}

export class Writer {
  constructor() { this.parts = []; this.length = 0; }
  raw(bytes) { this.parts.push(bytes); this.length += bytes.length; return this; }
  u32(n) { const b = new Uint8Array(4); new DataView(b.buffer).setUint32(0,n,true); return this.raw(b); }
  u64(n) { const b = new Uint8Array(8); new DataView(b.buffer).setBigUint64(0,n,true); return this.raw(b); }
  string(s) { const bytes = encoder.encode(s); return this.u32(bytes.length).raw(bytes); }
  finish() { const out = new Uint8Array(this.length); let p = 0; for (const bytes of this.parts) { out.set(bytes,p); p += bytes.length; } return out; }
}
