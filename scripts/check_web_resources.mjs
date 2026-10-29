// Exercise the actual host: resource deltas, GPU composition and context restoration.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile, mkdir } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
const {chromium,firefox,webkit}=createRequire(import.meta.url)('playwright');
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..'),out=resolve(root,'target/web-check');
await mkdir(out,{recursive:true});
const server=createServer(async(req,res)=>{
  const path=new URL(req.url,'http://localhost').pathname;
  if(path==='/') {res.setHeader('Content-Type','text/html');res.end('<!doctype html><canvas width="128" height="128"></canvas>');return;}
  if(!/^\/[a-z]+\.js$/.test(path)) {res.writeHead(404);res.end();return;}
  try {res.setHeader('Content-Type','text/javascript');res.end(await readFile(resolve(root,'hosts/web',path.slice(1))));}
  catch {res.writeHead(404);res.end();}
});
await new Promise(done=>server.listen(0,'127.0.0.1',done));
const engine=process.env.WEB_BROWSER||'chromium';
let browser;
try {
  browser=await ({chromium,firefox,webkit}[engine]).launch({headless:process.env.WEB_HEADLESS!=='0',...(process.env.WEB_CHANNEL?{channel:process.env.WEB_CHANNEL}:{})});
  const page=await browser.newPage(),errors=[];
  page.on('pageerror',e=>errors.push(String(e)));
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  const result=await page.evaluate(async()=>{
    const {Renderer}=await import('/rusterize.js'),{Writer,encoder}=await import('/wire.js');
    Writer.prototype.f32=function(n){const b=new Uint8Array(4);new DataView(b.buffer).setFloat32(0,n,true);return this.raw(b);};
    Writer.prototype.floats=function(ns){for(const n of ns)this.f32(n);return this;};
    const canvas=document.querySelector('canvas'),renderer=new Renderer(canvas),ctx=canvas.getContext('2d');
    const check=(condition,message)=>{if(!condition)throw new Error(message);};
    const near=(actual,expected)=>actual.every((v,i)=>Math.abs(v-expected[i])<=2);
    const pixel=(x,y)=>[...ctx.getImageData(x,y,1,1).data];
    const packet=(resources,commands)=>new Writer().raw(encoder.encode('RZ03')).u32(resources.length).raw(join(resources)).u32(commands.length).raw(join(commands)).finish();
    const join=parts=>{const w=new Writer();for(const p of parts)w.raw(p.finish());return w.finish();};
    const imageId=9007199254740993n,sourceId=9007199254740995n;
    const upload=(id,pixels)=>new Writer().u32(1).u64(id).u32(2).u32(2).u32(pixels.length).raw(new Uint8Array(pixels));
    const rgba=[255,0,0,255,0,255,0,128,0,0,255,64,255,255,255,0];
    const drawImage=(id,x=0)=>new Writer().u32(9).u64(id).floats([x,0,2,2,1]);
    renderer.render(packet([upload(imageId,rgba)],[drawImage(imageId)]));
    check(near(pixel(0,0),[255,0,0,255]),'initial bitmap upload');
    const cached=renderer.images.get(imageId);
    const delta=packet([],[drawImage(imageId,3),drawImage(imageId,6)]);
    renderer.render(delta);renderer.render(delta);
    check(renderer.images.get(imageId)===cached && near(pixel(3,1),[0,0,255,64]),'resource reuse/replay');
    renderer.render(packet([upload(imageId,Array(4).fill([0,255,0,255]).flat())],[drawImage(imageId)]));
    check(near(pixel(0,0),[0,255,0,255]),'re-upload replaces existing image');
    renderer.render(packet([new Writer().u32(2).u64(imageId)],[]));
    check(renderer.images.size===0,'explicit release');
    let missing=false;try{renderer.render(packet([],[drawImage(imageId)]));}catch{missing=true;}
    check(missing,'missing resource must fail');
    // A driver must receive no readPixels call while replaying Shader commands.
    const read=WebGL2RenderingContext.prototype.readPixels;
    WebGL2RenderingContext.prototype.readPixels=function(){throw new Error('Unexpected GPU readback');};
    let uploads=0;
    const texImage=WebGL2RenderingContext.prototype.texImage2D;
    WebGL2RenderingContext.prototype.texImage2D=function(...args){uploads++;return texImage.apply(this,args);};
    const vertex=`#version 300 es
      precision highp float; out vec2 uv;
      void main(){vec2 p=vec2(float((gl_VertexID<<1)&2),float(gl_VertexID&2));uv=vec2(p.x,1.0-p.y);gl_Position=vec4(p*2.0-1.0,0.0,1.0);}`;
    const fragment=`#version 300 es
      precision highp float; in vec2 uv; out vec4 color;
      layout(std140) uniform Params {vec4 values;vec4 data[4];};uniform sampler2D inputImage;
      void main(){color=data[0].w>0.0?texture(inputImage,uv):vec4(uv,data[0].x,data[0].y);}`;
    const shaderUpload=new Writer().u32(3).u64(sourceId).string(vertex).string(fragment).string('Params').string('inputImage');
    const drawShader=(image=0n,x=0,blue=0.25,alpha=0.5,texture=0)=>new Writer().u32(11).u64(sourceId)
      .floats([8,8,0,1,blue,alpha,0,texture,...Array(12).fill(0)]).u64(image).floats([x,0,8,8]);
    const clear=new Writer().u32(1).raw(new Uint8Array([255,255,255,255]));
    const commands=[clear,new Writer().u32(2),new Writer().u32(4).floats([1,0,0,1,4,4]),
      new Writer().u32(5).u32(0).floats([0,0,4,4]),drawShader(),new Writer().u32(3),
      drawShader(0n,20,1,1),new Writer().u32(6).u32(0).floats([5,5,1,1]).u32(0).raw(new Uint8Array([0,0,0,255]))];
    for(const scale of [1,1.5,2]) {
      renderer.render(packet(scale===1?[shaderUpload,upload(imageId,rgba)]:[],commands),scale);
      const pos=Math.floor(6.5*scale),sample=pixel(pos,pos);
      const uv=((pos+0.5)/scale-4)/8,expected=Math.round(127.5+127.5*uv);
      check(near(sample,[expected,expected,159,255]),`shader alpha/UV/transform scale=${scale}: ${sample}`);
      check(near(pixel(Math.floor(10*scale),Math.floor(10*scale)),[255,255,255,255]),'shader clip');
      check(near(pixel(Math.floor(5.5*scale),Math.floor(5.5*scale)),[0,0,0,255]),'draw order above shader');
      check(pixel(Math.floor(23*scale),Math.floor(3*scale))[2]===255,'multiple shader draws retain distinct uniforms');
    }
    const count=uploads;
    renderer.render(packet([],[drawShader(),drawShader(0n,20,1,1)]));
    check(uploads===count,'unchanged shader texture must not upload each frame');
    renderer.render(packet([],[drawShader(imageId,0,0,1,1)]));
    check(near(pixel(0,0),[255,0,0,255]) && pixel(0,7)[2]>250,'shader input orientation');
    const afterInput=uploads;
    renderer.render(packet([],[drawShader(imageId,0,0,1,1)]));
    check(uploads===afterInput,'bitmap input must not upload each frame');
    renderer.render(packet([upload(imageId,Array(4).fill([0,255,0,255]).flat())],[drawShader(imageId,0,0,1,1)]));
    check(near(pixel(0,0),[0,255,0,255]),'re-upload invalidates GPU input');
    const replacement=new Writer().u32(3).u64(sourceId).string(vertex)
      .string('#version 300 es\nprecision highp float;out vec4 color;void main(){color=vec4(0.0,0.0,1.0,1.0);}')
      .string('').string('');
    renderer.render(packet([replacement],[drawShader()]));
    check(near(pixel(0,0),[0,0,255,255]),'re-upload replaces compiled shader');
    renderer.render(packet([shaderUpload,upload(imageId,rgba)],[drawShader(imageId,0,0,1,1)]));
    const gpu=renderer.shaders.compositor,extension=gpu.gl.getExtension('WEBGL_lose_context');
    let restored=false;
    renderer.shaders.wake=()=>{restored=true;};
    if(extension) {
      const lost=new Promise(done=>gpu.canvas.addEventListener('webglcontextlost',done,{once:true}));
      extension.loseContext();await lost;
      renderer.render(packet([],[drawShader(imageId,0,0,1,1)]));
      const ready=new Promise((done,reject)=>{const timer=setTimeout(()=>reject(new Error('context restore timeout')),5000);gpu.canvas.addEventListener('webglcontextrestored',()=>{clearTimeout(timer);done();},{once:true});});
      // Complete dispatch of the loss event before requesting restoration.
      setTimeout(()=>extension.restoreContext(),100);await ready;
      renderer.render(packet([],[drawShader(imageId,0,0,1,1)]));
      check(restored && near(pixel(0,0),[255,0,0,255]),'context recovery reuploads retained input');
    }
    renderer.render(packet([new Writer().u32(2).u64(imageId),new Writer().u32(4).u64(sourceId)],[]));
    check(!gpu.textures.has(imageId) && gpu.programs.size===0 && renderer.shaders.sources.size===0,'release GPU objects');
    // Leave a representative composed frame for visual inspection.
    renderer.render(packet([shaderUpload],commands),2);
    renderer.shaders.destroy();
    WebGL2RenderingContext.prototype.readPixels=read;
    WebGL2RenderingContext.prototype.texImage2D=texImage;
    return {deltaBytes:delta.length,restored};
  });
  assert.deepEqual(errors,[]);
  await page.screenshot({path:resolve(out,`resources-${engine}.png`)});
  console.log(`WEB_RESOURCES_OK ${engine}: ${JSON.stringify(result)}; upload/release, references, alpha, clip, transforms, ordering, no readPixels, GPU lifecycle`);
} finally {await browser?.close();await new Promise(done=>server.close(done));}
