// Portable pixel check for the exact GLES compositor source in gpu.h.
// This does not replace a GTK/Linux host run.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
const { chromium } = createRequire(import.meta.url)('playwright');
const source = await readFile(new URL('./gpu.h', import.meta.url), 'utf8');
function shader(name) {
  const body = source.match(new RegExp(`static const char \\*${name}=([\\s\\S]*?);\\n`))?.[1];
  assert.ok(body, `Missing ${name} in gpu.h`);
  return [...body.matchAll(/"(?:[^"\\]|\\.)*"/g)].map(s => JSON.parse(s[0])).join('');
}
const browser = await chromium.launch({ headless: true, ...(process.env.WEB_CHANNEL ? { channel: process.env.WEB_CHANNEL } : {}) });
try {
  const page = await browser.newPage();
  const result = await page.evaluate(({ vertex, fragment }) => {
    const canvas = document.createElement('canvas');canvas.width = canvas.height = 4;
    const gl = canvas.getContext('webgl2', { premultipliedAlpha: false, antialias: false });
    if (!gl) throw new Error('WebGL2 unavailable');
    const program = gl.createProgram();
    for (const [type, source] of [[gl.VERTEX_SHADER, vertex], [gl.FRAGMENT_SHADER, fragment]]) {
      const stage = gl.createShader(type);gl.shaderSource(stage, source);gl.compileShader(stage);
      if (!gl.getShaderParameter(stage, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(stage));
      gl.attachShader(program, stage);gl.deleteShader(stage);
    }
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program));
    gl.useProgram(program);gl.bindVertexArray(gl.createVertexArray());
    const uniform = name => gl.getUniformLocation(program, name);
    gl.uniform1i(uniform('image'), 0);gl.uniform1i(uniform('mask'), 1);
    gl.uniform2f(uniform('viewport'), 4, 4);gl.viewport(0, 0, 4, 4);
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
      const pixels = new Uint8Array(64);gl.readPixels(0, 0, 4, 4, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
      return Array.from({length: 4}, (_, y) => Array.from({length: 4}, (_, x) => Array.from(pixels.slice(((3-y)*4+x)*4, ((3-y)*4+x)*4+4))));
    }
    function draw(mode, matrix = [1,0,0,0,1,0,0,0,1], bounds = [0,0,4,4]) {
      gl.uniform1i(uniform('mode'), mode);gl.uniformMatrix3fv(uniform('inverseTransform'), false, matrix);gl.uniform4fv(uniform('bounds'), bounds);gl.drawArrays(gl.TRIANGLES, 0, 3);
      const error = gl.getError();if (error) throw new Error(`GL error ${error}`);
      return read();
    }
    texture(1, 1, 1, [255,255,255,255]);texture(0, 1, 1, [30,20,10,128]);clear(0,0,0,0);
    const cairo = draw(0);
    // GL shader FBO bottom row is the image's bottom; top-down Cairo mask uses
    // its first uploaded row at texture y=0. The two textures differ by Y flip.
    texture(0, 2, 2, [0,0,255,64,255,255,255,0,255,0,0,255,0,255,0,128]);
    clear(1,1,1,1);const orientation = draw(1);
    const mask = [];
    for (let y = 0; y < 4; y++)for (let x = 0; x < 4; x++)mask.push(255,255,255,x+y<3?255:0);
    texture(1, 4, 4, mask);clear(1,1,1,1);const clipped = draw(1);
    // Rotation x=-localY+4, y=localX: inverse localX=y, localY=4-x.
    texture(1, 1, 1, [255,255,255,255]);clear(1,1,1,1);
    const rotated = draw(1, [0,-1,0,1,0,0,0,4,1]);
    const translatedMask = [];
    for (let y = 0; y < 4; y++)for (let x = 0; x < 4; x++)translatedMask.push(255,255,255,x>=1 && x<3 && y>=1 && y<3?255:0);
    texture(1, 4, 4, translatedMask);clear(1,1,1,1);
    const translated = draw(1, [1,0,0,0,1,0,-1,-1,1], [0,0,2,2]);
    // A half-covered arbitrary clip edge must scale premultiplied RGB and alpha.
    texture(0, 1, 1, [255,0,0,128]);texture(1, 1, 1, [255,255,255,128]);
    clear(1,1,1,1);const coverage = draw(1);
    // Cairo content after a shader is above it in the same target framebuffer.
    texture(0, 1, 1, [0,0,0,255]);const order = draw(0);
    return {cairo, orientation, clipped, rotated, translated, coverage, order, renderer: gl.getParameter(gl.RENDERER)};
  }, { vertex: shader('vertex'), fragment: shader('fragment') });
  let assertions = 0;
  const close = (actual, expected, label) => {assertions++;assert.ok(actual.every((v, i) => Math.abs(v-expected[i]) <= 2), `${label}: ${actual} != ${expected}`);};
  close(result.cairo[0][0], [10,20,30,128], 'Cairo BGRA premultiplication');
  close(result.orientation[0][0], [255,0,0,255], 'top left');
  close(result.orientation[0][3], [127,255,127,255], 'top right straight alpha');
  close(result.orientation[3][0], [191,191,255,255], 'bottom left straight alpha');
  close(result.orientation[3][3], [255,255,255,255], 'bottom right transparent');
  close(result.clipped[0][0], [255,0,0,255], 'inside arbitrary mask');
  close(result.clipped[3][0], [255,255,255,255], 'outside arbitrary mask');
  close(result.rotated[0][3], [255,0,0,255], 'rotated top right');
  close(result.rotated[0][0], [191,191,255,255], 'rotated top left');
  close(result.translated[1][1], [255,0,0,255], 'translated top left');
  close(result.translated[2][1], [191,191,255,255], 'translated bottom left');
  close(result.translated[0][0], [255,255,255,255], 'outside translated element');
  close(result.coverage[0][0], [255,191,191,255], 'partial coverage with alpha');
  close(result.order[0][0], [0,0,0,255], 'Cairo after shader');
  console.log(`LINUX_COMPOSITOR_GLSL_OK assertions=${assertions} renderer=${result.renderer}`);
} finally {
  await browser.close();
}
