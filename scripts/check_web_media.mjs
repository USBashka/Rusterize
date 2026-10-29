import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {readFile,mkdir} from 'node:fs/promises';
import {resolve,extname} from 'node:path';
import {createRequire} from 'node:module';
const {chromium,firefox,webkit}=createRequire(import.meta.url)('playwright');
const root=resolve(import.meta.dirname,'..'),out=resolve(root,'target/media-check');await mkdir(out,{recursive:true});
const server=createServer(async(req,res)=>{
  try{
    const path=new URL(req.url,'http://localhost').pathname;
    if(path==='/demo.mp3'){res.end(await readFile(resolve(root,'examples/media/assets/demo.mp3')));return;}
    if(path==='/cover.png'){res.end(await readFile(resolve(root,'examples/media/assets/cover.png')));return;}
    if(!/^\/(?:[a-z.-]+\.js|app\.wasm|index\.html)?$/.test(path)){res.writeHead(404).end();return;}
    const file=resolve(root,'dist/web/rusterize-media',path.slice(1)||'index.html');res.setHeader('Content-Type',{'.js':'text/javascript','.wasm':'application/wasm','.html':'text/html; charset=utf-8'}[extname(file)]);res.end(await readFile(file));
  }catch(e){res.writeHead(500).end(e.message);}
});
await new Promise(done=>server.listen(0,'127.0.0.1',done));
const origin=`http://127.0.0.1:${server.address().port}`;
const engine=process.env.WEB_BROWSER||'chromium',label=process.env.WEB_CHANNEL||engine;
const browser=await({chromium,firefox,webkit}[engine]).launch({headless:process.env.WEB_HEADLESS!=='0',...(process.env.WEB_CHANNEL?{channel:process.env.WEB_CHANNEL}:{})});
const errors=[];
try{
  const page=await browser.newPage({viewport:{width:840,height:680}});
  page.on('pageerror',e=>errors.push(String(e)));
  await page.addInitScript(()=>{window.showOpenFilePicker=undefined;window.showSaveFilePicker=undefined;});
  await page.goto(origin);
  await page.waitForFunction(()=>window.rusterize?.frameCount>=1 && !window.rusterize?.stopped);
  await page.waitForTimeout(900);
  // Real WASM application: trusted click unlocks audio, changes waveform and spectrum.
  await page.mouse.click(120,416);await page.waitForTimeout(350);
  await page.screenshot({path:resolve(out,`${label}-playing.png`)});
  await page.mouse.click(120,416);
  await page.mouse.click(370,474);await page.waitForTimeout(150);
  const saved=await page.evaluate(async()=>{const {Storage}=await import('./media.js');const s=new Storage();try{return Array.from(await s.execute(42,'dev.rusterize.media','volume'));}finally{s.destroy();}});
  assert.equal(saved[0],1);assert.ok(Number(new TextDecoder().decode(new Uint8Array(saved.slice(1))))>0.3);
  await page.reload();await page.waitForFunction(()=>window.rusterize?.frameCount>=1);await page.waitForTimeout(500);
  await page.screenshot({path:resolve(out,`${label}-restored.png`)});
  const [chooser]=await Promise.all([page.waitForEvent('filechooser'),page.mouse.click(100,100)]);
  await chooser.setFiles(resolve(root,'examples/media/assets/demo.mp3'));await page.waitForTimeout(400);
  const [imageChooser]=await Promise.all([page.waitForEvent('filechooser'),page.mouse.click(550,100)]);
  await imageChooser.setFiles(resolve(root,'examples/media/assets/cover.png'));await page.waitForTimeout(400);
  page.once('dialog',dialog=>dialog.accept('rusterize-volume.txt'));
  const [download]=await Promise.all([page.waitForEvent('download'),page.mouse.click(150,536)]);
  assert.equal((await readFile(await download.path(),'utf8')),new TextDecoder().decode(new Uint8Array(saved.slice(1))));
  // Test backend contracts separately with real decoded data and real AudioContext.
  const contract=await page.evaluate(async()=>{
    const {Media,Storage}=await import('./media.js');const {Reader}=await import('./wire.js');
    window.mediaTest=new Media(()=>{},new URL('./app.wasm',location).href);
    const m=window.mediaTest,bytes=new Uint8Array(await(await fetch('/demo.mp3')).arrayBuffer());
    const metadata=new Reader(await m.decodeAudio(bytes));const clip=metadata.u32(),rate=metadata.u32(),channels=metadata.u32(),frames=metadata.u32();
    const {Writer}=await import('./wire.js');window.mediaCommand=(op,id,extra=[])=>m.sync(op,new Reader(new Writer().u32(id).raw(new Uint8Array(extra)).finish()));
    const player=new Reader(window.mediaCommand(70,clip)).u32();window.mediaPlayer=player;
    const image=await m.decodeImage(new Uint8Array(await(await fetch('/cover.png')).arrayBuffer()));
    let corrupt=false;try{await m.decodeImage(new Uint8Array([0,1,2,3]));}catch{corrupt=true;}
    const storage=new Storage();await storage.execute(43,'contract','key',new Uint8Array([1,2,3]));
    let traversal=false;try{await storage.execute(43,'contract','../key',new Uint8Array([9]));}catch{traversal=true;}
    const before=Array.from(await storage.execute(42,'contract','key'));
    let limit=false;try{await storage.execute(43,'contract','key',new Uint8Array(32*1024*1024+1));}catch{limit=true;}
    const original=IDBObjectStore.prototype.put;let aborted=false;
    try{
      IDBObjectStore.prototype.put=function(...args){const request=original.apply(this,args);this.transaction.abort();return request;};
      await storage.execute(43,'contract','key',new Uint8Array([9]));
    }catch{aborted=true;}finally{IDBObjectStore.prototype.put=original;}
    const after=Array.from(await storage.execute(42,'contract','key'));storage.destroy();
    const {BrowserServices}=await import('./services.js');
    const clock=op=>new DataView(BrowserServices.prototype.sync.call({},new Writer().u32(op).finish()).buffer).getFloat64(0,true);
    const clocks=Math.abs(clock(48)-Date.now()/1000)<0.1 && Math.abs(clock(49)-performance.now()/1000)<0.1;
    const button=document.createElement('button');button.id='audio-contract';button.textContent='Play test';button.onclick=()=>window.mediaCommand(71,player);document.body.append(button);
    return{duration:frames/rate,channels,imageLength:image.length,corrupt,traversal,limit,aborted,clocks,before,after};
  });
  assert.ok(contract.duration>11.8&&contract.duration<12.3);assert.equal(contract.channels,1);
  assert.equal(contract.imageLength,8+256*256*4);assert.ok(contract.corrupt&&contract.traversal&&contract.limit&&contract.aborted&&contract.clocks);assert.deepEqual(contract.before,contract.after);
  await page.locator('#audio-contract').click();await page.waitForTimeout(180);
  const position=await page.evaluate(()=>{const b=window.mediaCommand(76,window.mediaPlayer);return new DataView(b.buffer).getFloat64(4,true);});assert.ok(position>0.05);
  await page.evaluate(()=>window.mediaCommand(72,window.mediaPlayer));
  const paused=await page.evaluate(()=>new DataView(window.mediaCommand(76,window.mediaPlayer).buffer).getFloat64(4,true));
  await page.waitForTimeout(120);
  assert.equal(await page.evaluate(()=>new DataView(window.mediaCommand(76,window.mediaPlayer).buffer).getFloat64(4,true)),paused);
  const seek=await page.evaluate(()=>{const a=new Uint8Array(8);new DataView(a.buffer).setFloat64(0,11.8,true);window.mediaCommand(74,window.mediaPlayer,a);return new DataView(window.mediaCommand(76,window.mediaPlayer).buffer).getFloat64(4,true);});assert.ok(Math.abs(seek-11.8)<0.001);
  await page.locator('#audio-contract').click();await page.waitForTimeout(700);
  assert.equal(await page.evaluate(()=>new DataView(window.mediaCommand(76,window.mediaPlayer).buffer).getUint32(0,true)),3);
  await page.evaluate(()=>{window.mediaCommand(71,window.mediaPlayer);window.mediaTest.interrupt();});
  assert.equal(await page.evaluate(()=>new DataView(window.mediaCommand(76,window.mediaPlayer).buffer).getUint32(0,true)),4);
  await page.evaluate(()=>{window.mediaTest.destroy();window.rusterize.destroy();});
  assert.deepEqual(errors,[]);
  console.log('WEB_MEDIA_OK',label,JSON.stringify(contract));
}finally{await browser.close();await new Promise(done=>server.close(done));}
