// Portable pixel check of the exact compositor math from gpu.swift.
// Only the GLSL profile/precision is adapted; this is not a macOS/GL 4.1 test.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
const { chromium } = createRequire(import.meta.url)('playwright');
const source = await readFile(new URL('./gpu.swift', import.meta.url), 'utf8');
const match = source.match(/composition = try Self\.program\(vertex: """\r?\n([\s\S]*?)\r?\n\s*""", fragment: """\r?\n([\s\S]*?)\r?\n\s*"""\)/);
assert.ok(match, 'Missing MacGPU composition shaders in gpu.swift');
function adapt(text) {
  const lines = text.split(/\r?\n/);
  const indent = Math.min(...lines.filter(line => line.trim()).map(line => line.match(/^ */)[0].length));
  const glsl = lines.map(line => line.slice(indent)).join('\n');
  assert.equal((glsl.match(/#version 330 core/g) || []).length, 1);
  return glsl.replace('#version 330 core', '#version 300 es\nprecision highp float;\nprecision highp int;');
}
const browser = await chromium.launch({ headless: true, ...(process.env.WEB_CHANNEL ? { channel: process.env.WEB_CHANNEL } : {}) });
try {
  const page = await browser.newPage();
  const result = await page.evaluate(({ vertex, fragment }) => {
    const canvas = document.createElement('canvas');canvas.width = canvas.height = 4;
    const gl = canvas.getContext('webgl2', { premultipliedAlpha: false, antialias: false });
    if (!gl) throw new Error('WebGL2 unavailable');
    const program = gl.createProgram();
    for (const [type, text] of [[gl.VERTEX_SHADER, vertex], [gl.FRAGMENT_SHADER, fragment]]) {
      const stage = gl.createShader(type);gl.shaderSource(stage, text);gl.compileShader(stage);
      if (!gl.getShaderParameter(stage, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(stage));
      gl.attachShader(program, stage);gl.deleteShader(stage);
    }
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program));
    gl.useProgram(program);gl.bindVertexArray(gl.createVertexArray());
    const uniform = name => gl.getUniformLocation(program, name);
    gl.uniform1i(uniform('pixels'), 0);gl.uniform1i(uniform('coverage'), 1);
    gl.enable(gl.BLEND);gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    function texture(unit, width, height, pixels) {
      gl.activeTexture(gl.TEXTURE0 + unit);gl.bindTexture(gl.TEXTURE_2D, gl.createTexture());
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, width, height, 0, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array(pixels));
    }
    function clear(r, g, b, a) {gl.clearColor(r, g, b, a);gl.clear(gl.COLOR_BUFFER_BIT);}
    function read() {
      const w = canvas.width, h = canvas.height, pixels = new Uint8Array(w*h*4);
      gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
      return Array.from({length: h}, (_, y) => Array.from({length: w}, (_, x) => Array.from(pixels.slice(((h-1-y)*w+x)*4, ((h-1-y)*w+x)*4+4))));
    }
    function draw(shader, inverseX = [1,0,0], inverseY = [0,-1,4], destination = [0,0,4,4]) {
      gl.viewport(0,0,canvas.width,canvas.height);
      gl.uniform2f(uniform('viewportSize'),canvas.width,canvas.height);
      gl.uniform1i(uniform('shaderLayer'),shader);
      gl.uniform3fv(uniform('inverseX'),inverseX);gl.uniform3fv(uniform('inverseY'),inverseY);
      gl.uniform4fv(uniform('destination'),destination);gl.drawArrays(gl.TRIANGLES,0,3);
      const error = gl.getError();if (error) throw new Error(`GL error ${error}`);
      return read();
    }
    // CoreGraphics uploads RGBA already premultiplied, with the first row at the top.
    texture(1,1,1,[255,255,255,255]);texture(0,1,2,[10,20,30,128,50,60,70,255]);
    clear(0,0,0,0);const canvasLayer = draw(0);
    // Shader FBO row zero is the bottom. Inverse CGContext CTM includes its Y flip.
    const shaderPixels = [0,0,255,64,255,255,255,0,255,0,0,255,0,255,0,128];
    texture(0,2,2,shaderPixels);clear(1,1,1,1);const orientation = draw(1);
    const mask = [];
    for (let y=0;y<4;y++)for (let x=0;x<4;x++)mask.push(255,255,255,x+y<3?255:0);
    texture(1,4,4,mask);clear(1,1,1,1);const clipped = draw(1);
    // logical: x=-localY+4, y=localX. Device Y is 4-logicalY.
    texture(1,1,1,[255,255,255,255]);clear(1,1,1,1);
    const rotated = draw(1,[0,-1,4],[-1,0,4]);
    const translatedMask = [];
    for (let y=0;y<4;y++)for (let x=0;x<4;x++)translatedMask.push(255,255,255,x>=1&&x<3&&y>=1&&y<3?255:0);
    texture(1,4,4,translatedMask);clear(1,1,1,1);
    const translated = draw(1,[1,0,-1],[0,-1,3],[0,0,2,2]);
    texture(0,1,1,[255,0,0,128]);texture(1,1,1,[255,255,255,128]);
    clear(1,1,1,1);const coverage = draw(1);
    texture(0,1,1,[0,0,255,128]);texture(1,1,1,[255,255,255,255]);
    const secondShader = draw(1);
    texture(0,1,1,[0,0,0,255]);const order = draw(0);
    // Retina: inverse of CTM(scale=2, translateY=8) maps physical centres to points.
    canvas.width = canvas.height = 8;
    texture(0,2,2,shaderPixels);texture(1,1,1,[255,255,255,255]);
    clear(1,1,1,1);const retina = draw(1,[0.5,0,0],[0,-0.5,4]);
    return {canvasLayer,orientation,clipped,rotated,translated,coverage,secondShader,order,retina,renderer:gl.getParameter(gl.RENDERER)};
  }, { vertex:adapt(match[1]), fragment:adapt(match[2]) });
  let assertions = 0;
  const close = (actual,expected,label) => {assertions++;assert.ok(actual.every((v,i)=>Math.abs(v-expected[i])<=2),`${label}: ${actual} != ${expected}`);};
  close(result.canvasLayer[0][0],[10,20,30,128],'CoreGraphics RGBA premultiplication/top row');
  close(result.canvasLayer[3][0],[50,60,70,255],'CoreGraphics bottom row');
  close(result.orientation[0][0],[255,0,0,255],'top left');
  close(result.orientation[0][3],[127,255,127,255],'top right straight alpha');
  close(result.orientation[3][0],[191,191,255,255],'bottom left straight alpha');
  close(result.orientation[3][3],[255,255,255,255],'bottom right transparent');
  close(result.clipped[0][0],[255,0,0,255],'inside arbitrary mask');
  close(result.clipped[3][0],[255,255,255,255],'outside arbitrary mask');
  close(result.rotated[0][3],[255,0,0,255],'rotated top right');
  close(result.rotated[0][0],[191,191,255,255],'rotated top left');
  close(result.translated[1][1],[255,0,0,255],'translated top left');
  close(result.translated[2][1],[191,191,255,255],'translated bottom left');
  close(result.translated[0][0],[255,255,255,255],'outside translated element');
  close(result.coverage[0][0],[255,191,191,255],'partial coverage with alpha');
  close(result.secondShader[0][0],[127,95,223,255],'second shader above first shader');
  close(result.order[0][0],[0,0,0,255],'CoreGraphics after shaders');
  close(result.retina[0][0],[255,0,0,255],'Retina top left');
  close(result.retina[0][7],[127,255,127,255],'Retina top right');
  close(result.retina[7][0],[191,191,255,255],'Retina bottom left');
  console.log(`MACOS_COMPOSITOR_GLSL_ES_ADAPTATION_OK assertions=${assertions} renderer=${result.renderer}; not_native_GL4.1`);
} finally { await browser.close(); }
