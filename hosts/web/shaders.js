import { Writer } from './wire.js';

class GPU {
  constructor(wake = () => {}) {
    this.canvas = document.createElement('canvas');
    this.gl = this.canvas.getContext('webgl2', { alpha:true, premultipliedAlpha:false, antialias:false, depth:false, stencil:false });
    if (!this.gl) throw new Error('WebGL 2 is unavailable');
    this.max = Math.min(4096, this.gl.getParameter(this.gl.MAX_TEXTURE_SIZE));
    this.name = `${this.gl.getParameter(this.gl.RENDERER)} / ${this.gl.getParameter(this.gl.VERSION)}`;
    this.programs = new Map();
    this.textures = new Map();
    this.lost = e => { e.preventDefault(); this.resources = null; this.programs.clear(); this.textures.clear(); };
    this.restored = () => wake();
    this.canvas.addEventListener('webglcontextlost', this.lost);
    this.canvas.addEventListener('webglcontextrestored', this.restored);
  }
  check() {
    if (this.gl.isContextLost()) throw new Error('WebGL context lost; retry after restoration');
    const error = this.gl.getError();
    if (error !== this.gl.NO_ERROR) throw new Error(`WebGL error 0x${error.toString(16)}`);
  }
  init() {
    this.check();
    if (this.resources) return;
    const g = this.gl;
    const resources = { vao:g.createVertexArray(), uniform:g.createBuffer(), fbo:g.createFramebuffer(), output:g.createTexture(), input:g.createTexture() };
    if (Object.values(resources).some(v => !v)) throw new Error('WebGL resource allocation failed');
    this.resources = resources;
    this.size = [0,0]; this.inputId = null;
    for (const t of [resources.input,resources.output]) {
      g.bindTexture(g.TEXTURE_2D,t);
      g.texParameteri(g.TEXTURE_2D,g.TEXTURE_MIN_FILTER,g.LINEAR);
      g.texParameteri(g.TEXTURE_2D,g.TEXTURE_MAG_FILTER,g.LINEAR);
      g.texParameteri(g.TEXTURE_2D,g.TEXTURE_WRAP_S,g.CLAMP_TO_EDGE);
      g.texParameteri(g.TEXTURE_2D,g.TEXTURE_WRAP_T,g.CLAMP_TO_EDGE);
    }
    this.check();
  }
  program(id, vertex, fragment, block, sampler) {
    if (this.programs.has(id)) {
      const p = this.programs.get(id); this.programs.delete(id); this.programs.set(id,p); return p;
    }
    const g = this.gl, program = g.createProgram(), stages = [];
    if (!program) throw new Error('WebGL program allocation failed');
    try {
      for (const [kind,source] of [[g.VERTEX_SHADER,vertex],[g.FRAGMENT_SHADER,fragment]]) {
        const stage = g.createShader(kind);
        if (!stage) throw new Error('WebGL shader allocation failed');
        stages.push(stage); g.shaderSource(stage,source); g.compileShader(stage);
        if (!g.getShaderParameter(stage,g.COMPILE_STATUS)) throw new Error(`WebGL compilation: ${g.getShaderInfoLog(stage)}`);
        g.attachShader(program,stage);
      }
      g.linkProgram(program);
      if (!g.getProgramParameter(program,g.LINK_STATUS)) throw new Error(`WebGL link: ${g.getProgramInfoLog(program)}`);
      g.useProgram(program);
      if (block) {
        const index = g.getUniformBlockIndex(program,block);
        if (index !== g.INVALID_INDEX) g.uniformBlockBinding(program,index,0);
      }
      if (sampler) g.uniform1i(g.getUniformLocation(program,sampler),0);
      this.check();
    } catch (error) { g.deleteProgram(program); throw error; }
    finally { for (const stage of stages) g.deleteShader(stage); }
    if (this.programs.size === 32) {
      const key = this.programs.keys().next().value;
      g.deleteProgram(this.programs.get(key)); this.programs.delete(key);
    }
    this.programs.set(id,program); return program;
  }
  render(r) {
    const id = r.u32(), sources = [r.string(),r.string(),r.string(),r.string()];
    const values = Float32Array.from({length:20},()=>r.f32());
    const imageId = r.u64(), iw = r.u32(), ih = r.u32(), pixels = r.take(r.u32());
    r.done();
    return this.draw(id, sources, values, imageId, {width:iw, height:ih, pixels}, false);
  }
  draw(id, sources, values, imageId, image, direct) {
    this.init();
    const [width,height] = values, {width:iw, height:ih, pixels} = image;
    if (![width,height,iw,ih].every(n => Number.isInteger(n) && n > 0 && n <= this.max) ||
        width * height > 4194304 || iw * ih * 4 !== pixels.length || values[3] <= 0)
      throw new Error('Invalid WebGL dimensions or parameters');
    const g = this.gl, q = this.resources;
    const program = this.program(id,...sources);
    g.useProgram(program); g.bindVertexArray(q.vao);
    g.bindBuffer(g.UNIFORM_BUFFER,q.uniform); g.bufferData(g.UNIFORM_BUFFER,values,g.DYNAMIC_DRAW);
    g.bindBufferBase(g.UNIFORM_BUFFER,0,q.uniform);
    if (direct) {
      if (this.canvas.width !== width) this.canvas.width = width;
      if (this.canvas.height !== height) this.canvas.height = height;
      g.bindFramebuffer(g.FRAMEBUFFER,null);
    } else {
    if (width !== this.size[0] || height !== this.size[1]) {
      g.bindTexture(g.TEXTURE_2D,q.output);
      g.texImage2D(g.TEXTURE_2D,0,g.RGBA8,width,height,0,g.RGBA,g.UNSIGNED_BYTE,null);
      this.size = [width,height];
    }
    g.bindFramebuffer(g.FRAMEBUFFER,q.fbo);
    g.framebufferTexture2D(g.FRAMEBUFFER,g.COLOR_ATTACHMENT0,g.TEXTURE_2D,q.output,0);
    if (g.checkFramebufferStatus(g.FRAMEBUFFER) !== g.FRAMEBUFFER_COMPLETE) throw new Error('Incomplete WebGL framebuffer');
    }
    g.activeTexture(g.TEXTURE0);
    let texture = direct ? this.textures.get(imageId) : q.input;
    const upload = direct ? !texture : this.inputId !== imageId;
    if (!texture) {
      texture = g.createTexture();
      if (!texture) throw new Error('WebGL texture allocation failed');
      this.textures.set(imageId,texture);
    }
    g.bindTexture(g.TEXTURE_2D,texture);
    if (upload) {
      g.texParameteri(g.TEXTURE_2D,g.TEXTURE_MIN_FILTER,g.LINEAR);
      g.texParameteri(g.TEXTURE_2D,g.TEXTURE_MAG_FILTER,g.LINEAR);
      g.texParameteri(g.TEXTURE_2D,g.TEXTURE_WRAP_S,g.CLAMP_TO_EDGE);
      g.texParameteri(g.TEXTURE_2D,g.TEXTURE_WRAP_T,g.CLAMP_TO_EDGE);
      g.pixelStorei(g.UNPACK_ALIGNMENT,1);
      g.pixelStorei(g.UNPACK_PREMULTIPLY_ALPHA_WEBGL,false);
      g.texImage2D(g.TEXTURE_2D,0,g.RGBA8,iw,ih,0,g.RGBA,g.UNSIGNED_BYTE,pixels);
      if (!direct) this.inputId = imageId;
    }
    g.viewport(0,0,width,height);
    for (const flag of [g.BLEND,g.DEPTH_TEST,g.SCISSOR_TEST,g.DITHER]) g.disable(flag);
    g.clearColor(0,0,0,0); g.clear(g.COLOR_BUFFER_BIT); g.drawArrays(g.TRIANGLES,0,3);
    if (direct) { this.check(); return this.canvas; }
    const output = new Uint8Array(width*height*4);
    g.pixelStorei(g.PACK_ALIGNMENT,1); g.readPixels(0,0,width,height,g.RGBA,g.UNSIGNED_BYTE,output);
    this.check();
    const stride = width*4, row = new Uint8Array(stride);
    for (let y=0;y<Math.floor(height/2);y++) {
      const a=y*stride,b=(height-1-y)*stride;
      row.set(output.subarray(a,a+stride)); output.copyWithin(a,b,b+stride); output.set(row,b);
    }
    return output;
  }
  releaseImage(id) {
    const texture = this.textures.get(id);
    if (texture) { this.gl.deleteTexture(texture); this.textures.delete(id); }
  }
  releaseSource(id) {
    const program = this.programs.get(id);
    if (program) { this.gl.deleteProgram(program); this.programs.delete(id); }
  }
  destroy() {
    const g=this.gl,q=this.resources;
    for (const p of this.programs.values()) g.deleteProgram(p);
    for (const texture of this.textures.values()) g.deleteTexture(texture);
    if (q) { g.deleteVertexArray(q.vao); g.deleteBuffer(q.uniform); g.deleteFramebuffer(q.fbo); g.deleteTexture(q.input); g.deleteTexture(q.output); }
    this.programs.clear(); this.textures.clear(); this.resources=null;
    this.canvas.removeEventListener('webglcontextlost',this.lost);
    this.canvas.removeEventListener('webglcontextrestored',this.restored);
    g.getExtension('WEBGL_lose_context')?.loseContext();
  }
}

export class Shaders {
  constructor(wake = () => {}) { this.contexts=new Map(); this.next=1; this.sources=new Map(); this.wake=wake; }
  uploadSource(id, sources) { this.compositor?.releaseSource(id); this.sources.set(id,sources); }
  releaseSource(id) { this.sources.delete(id); this.compositor?.releaseSource(id); }
  releaseImage(id) { this.compositor?.releaseImage(id); }
  draw(ctx, id, values, imageId, image, destination) {
    const source = this.sources.get(id);
    if (!source) throw new Error('Unknown Rusterize shader resource');
    this.compositor ??= new GPU(() => this.wake());
    // Retain source/bitmap resources while WebGL restores its device objects.
    if (this.compositor.gl.isContextLost()) return;
    const input = imageId === 0n ? {width:1,height:1,pixels:new Uint8Array([255,255,255,255])} : image;
    if (!input) throw new Error('Unknown Rusterize shader image');
    const canvas = this.compositor.draw(id,source,values,imageId,input,true);
    ctx.drawImage(canvas,...destination);
  }
  execute(op,r) {
    if (op===30) {
      if (this.contexts.size>=8) throw new Error('At most eight WebGL renderers per application');
      const gpu=new GPU(),id=this.next++; this.contexts.set(id,gpu);
      return new Writer().u32(id).u32(gpu.max).string(gpu.name).finish();
    }
    const id=r.u32(),gpu=this.contexts.get(id);
    if (!gpu) throw new Error('Unknown WebGL renderer');
    if (op===31) return gpu.render(r);
    if (op===32) { gpu.destroy(); this.contexts.delete(id); return new Uint8Array(); }
    throw new Error('Unknown GPU operation');
  }
  destroy() { for (const gpu of this.contexts.values()) gpu.destroy(); this.contexts.clear(); this.compositor?.destroy(); this.compositor=null; this.sources.clear(); }
}
