// Run after building shader/webview examples with self-check and web_services.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile, mkdir } from 'node:fs/promises';
import { resolve, dirname, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
const { chromium, firefox, webkit }=createRequire(import.meta.url)('playwright');
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..'),out=resolve(root,'target/web-check');
await mkdir(out,{recursive:true});
const server=createServer(async(req,res)=>{
  try {
    const path=new URL(req.url,'http://localhost').pathname;
    let file;
    if(path==='/services.wasm') file=resolve(root,'target/wasm32-unknown-unknown/release/examples/web_services.wasm');
    else if(path==='/fixture') {res.setHeader('Content-Type','text/html');res.end('<!doctype html><meta charset="utf-8"><style>body{margin:0}canvas{width:100vw;height:100vh;display:block}</style><canvas id="app" tabindex="0"></canvas>');return;}
    else {
      const p=path.endsWith('/')?path+'index.html':path;
      if(!/^\/rusterize-(shaders|webview)\/(index\.html|[a-z]+\.js|app\.wasm)$/.test(p)) {res.writeHead(404);res.end();return;}
      file=resolve(root,'dist/web',p.slice(1));
    }
    res.setHeader('Content-Type',{'.wasm':'application/wasm','.js':'text/javascript','.html':'text/html; charset=utf-8'}[extname(file)]);
    res.end(await readFile(file));
  } catch(e) {res.writeHead(500);res.end(e.message);}
});
await new Promise(done=>server.listen(0,'127.0.0.1',done));
const url=`http://127.0.0.1:${server.address().port}`,engine=process.env.WEB_BROWSER||'chromium';
const browser=await ({chromium,firefox,webkit}[engine]).launch({headless:true,...(process.env.WEB_CHANNEL?{channel:process.env.WEB_CHANNEL}:{})});
try {
  const context=await browser.newContext({viewport:{width:760,height:540}}),page=await context.newPage(),errors=[];
  page.on('pageerror',e=>errors.push(String(e)));
  await page.addInitScript(()=>{
    const instantiate=WebAssembly.instantiate;
    WebAssembly.instantiate=async(...args)=>{const result=await instantiate(...args);window.testExports=(result.instance??result).exports;return result;};
    window.drawnText=[];
    const fillText=CanvasRenderingContext2D.prototype.fillText;
    CanvasRenderingContext2D.prototype.fillText=function(text,...args){window.drawnText.push(text);return fillText.call(this,text,...args);};
  });
  await page.goto(`${url}/rusterize-shaders/`);
  await page.waitForFunction(()=>window.drawnText.includes('WGSL') || window.rusterize?.stopped || window.drawnText.length>5);
  assert.ok(await page.evaluate(()=>window.drawnText.includes('WGSL')),JSON.stringify(await page.evaluate(()=>window.drawnText)));
  await page.waitForFunction(()=>window.rusterize.frameCount>3);
  const pixels=await page.evaluate(()=>Array.from(document.querySelector('canvas').getContext('2d').getImageData(150,150,2,2).data));
  assert.ok(pixels.some((v,i)=>i%4!==3 && v>50),'shader must draw real colors');
  await page.screenshot({path:resolve(out,`shaders-${engine}.png`)});
  await page.locator('canvas').click();
  await page.waitForTimeout(120);
  const frames=await page.evaluate(()=>window.rusterize.frameCount);
  await page.waitForTimeout(150);
  assert.equal(await page.evaluate(()=>window.rusterize.frameCount),frames,'paused shader must be idle');
  await page.evaluate(()=>window.rusterize.destroy());

  await page.goto(`${url}/rusterize-webview/`);
  await page.waitForFunction(()=>window.rusterize?.stopped,{},{timeout:15000});
  assert.equal(await page.title(),'WEBVIEW_SMOKE_OK',JSON.stringify(await page.evaluate(()=>window.drawnText)));
  assert.equal(await page.locator('iframe').count(),0,'destroy removes child views');

  await page.goto(`${url}/fixture`);
  await page.evaluate(async()=>{
    const content='Ёж идёт по лесу 🦔';
    Object.defineProperty(navigator,'clipboard',{configurable:true,value:{readText:async()=>content,writeText:async text=>{window.written=text;}}});
    window.showOpenFilePicker=undefined;window.showSaveFilePicker=undefined;window.showDirectoryPicker=undefined;
    const {start}=await import('/rusterize-shaders/rusterize.js');
    window.app=await start(document.querySelector('canvas'),'/services.wasm',{onError:e=>{window.failure=String(e);}});
  });
  await page.waitForFunction(()=>window.app.frameCount>0);
  const metric=async(index,value=1)=>page.waitForFunction(([index,value])=>window.failure || window.testExports.web_service_metric(index)===value,[index,value]).then(async()=>assert.equal(await page.evaluate(()=>window.failure),undefined));
  await page.locator('canvas').focus();
  await page.keyboard.press('1');await metric(1);
  await page.keyboard.press('2');await metric(2);
  assert.equal(await page.evaluate(()=>window.written),'Ёж идёт по лесу 🦔');
  await page.keyboard.press('3');
  await page.getByRole('dialog').waitFor();
  assert.equal(await page.getByRole('dialog').innerText(),'Проверка\n\nЁж идёт по лесу 🦔\n\nОК');
  await page.getByRole('button',{name:'ОК',exact:true}).click();await metric(3);
  await page.locator('canvas').focus();
  const chooser=page.waitForEvent('filechooser');await page.keyboard.press('4');
  await (await chooser).setFiles({name:'ёж.txt',mimeType:'text/plain',buffer:Buffer.from('Ёж идёт по лесу 🦔')});await metric(4);
  await page.keyboard.press('0');await metric(12);
  page.once('dialog',dialog=>dialog.accept('ёж.txt'));
  const download=page.waitForEvent('download');await page.keyboard.press('5');
  const saved=await download;assert.equal(saved.suggestedFilename(),'ёж.txt');
  assert.equal(await readFile(await saved.path(),'utf8'),'Ёж идёт по лесу 🦔');await metric(5);
  await page.keyboard.press('9');await metric(9);
  // Native file handles and cancellation are tested through the Rust ABI too.
  await page.evaluate(()=>{
    const data=new File(['Ёж идёт по лесу 🦔'],'ёж.txt');
    window.showOpenFilePicker=async()=>[{name:data.name,getFile:async()=>data}];
    window.showDirectoryPicker=async()=>({name:'folder',async *entries(){yield ['ёж.txt',{name:'ёж.txt',kind:'file',getFile:async()=>data}];}});
    window.showSaveFilePicker=async()=>({name:'ёж.txt',createWritable:async()=>({write:async bytes=>{window.savedBytes=Array.from(bytes);},close:async()=>{},abort:async()=>{}})});
  });
  await page.keyboard.press('4');await metric(4,2);
  await page.keyboard.press('5');await metric(5,2);
  assert.equal(Buffer.from(await page.evaluate(()=>window.savedBytes)).toString(),'Ёж идёт по лесу 🦔');
  await page.keyboard.press('6');await metric(6);
  await page.evaluate(()=>{window.showDirectoryPicker=async()=>({name:'empty',async *entries(){}});});
  await page.keyboard.press('6');await metric(14);
  await page.evaluate(()=>{window.showOpenFilePicker=async()=>{throw new DOMException('cancel','AbortError');};});
  await page.keyboard.press('4');await metric(13);
  if(await page.evaluate(()=>document.fullscreenEnabled)) {
    await page.keyboard.press('7');await metric(7);assert.ok(await page.evaluate(()=>!!document.fullscreenElement));
    await page.keyboard.press('8');await metric(8);assert.equal(await page.evaluate(()=>document.fullscreenElement),null);
  }
  // An unresolved permission request must not revive a destroyed instance.
  await page.evaluate(()=>{navigator.clipboard.readText=()=>new Promise(resolve=>{window.finishClipboard=resolve;});});
  await page.keyboard.press('1');await page.evaluate(()=>window.app.destroy());
  await page.evaluate(()=>window.finishClipboard('late'));await page.waitForTimeout(80);
  assert.equal(await page.evaluate(()=>window.testExports.web_service_metric(1)),1);

  // Browser-specific isolation, history and navigation cancellation contracts.
  await page.evaluate(async()=>{
    const {BrowserServices}=await import('/rusterize-shaders/services.js');
    const {Reader,Writer}=await import('/rusterize-shaders/wire.js');
    window.services=new BrowserServices(document.querySelector('canvas'),null,()=>{});
    window.view=(action,append=()=>{})=>{
      const w=new Writer().u32(20).u32(1).u32(action);append(w);
      return window.services.execute(new Reader(w.finish()));
    };
    window.result=async(kind,token)=>{
      const deadline=performance.now()+3000;
      while(performance.now()<deadline) {
        const r=new Reader(window.services.webviews.poll()),found=r.u32();
        if(found) {const id=r.u32(),t=r.u32(),value=r.string();if(found===kind && (token===undefined||token===t)) return {id,token:t,value};}
        await new Promise(resolve=>setTimeout(resolve,10));
      }
      throw new Error(`Missing WebView event ${kind}/${token}`);
    };
    await window.view(1,w=>w.raw(new Uint8Array(new Float32Array([10,20,300,200]).buffer)).u32(1).u32(1));
    await window.result(1);
    await window.view(6,w=>w.string('<button onclick="this.textContent=\'Ёж\'">Нажать</button>'));
    await window.result(2);
  });
  assert.equal(await page.locator('iframe').getAttribute('sandbox'),'allow-scripts');
  await page.frameLocator('iframe').getByRole('button').click();
  assert.equal(await page.frameLocator('iframe').getByRole('button').innerText(),'Ёж');
  assert.equal(await page.evaluate(async()=>{
    await window.view(7,w=>w.u32(42).string('try {parent.document.body.innerHTML; "unsafe"} catch(e) {e.name}'));
    return (await window.result(4,42)).value;
  }),'"SecurityError"');
  assert.equal(await page.evaluate(async()=>{
    await window.view(7,w=>w.u32(43).string('Promise.resolve(42)'));
    return (await window.result(4,43)).value;
  }),'42');
  assert.match(await page.evaluate(async()=>{
    await window.view(7,w=>w.u32(44).string('new Promise(()=>{})'));
    await window.view(6,w=>w.string('<p>Второй документ</p>'));
    return (await window.result(5,44)).value;
  }),/navigated/);
  await page.frameLocator('iframe').getByText('Второй документ').waitFor();
  await page.evaluate(()=>window.view(10));
  await page.frameLocator('iframe').getByRole('button',{name:'Нажать'}).waitFor();
  await page.evaluate(()=>window.view(11));
  await page.frameLocator('iframe').getByText('Второй документ').waitFor();
  await page.screenshot({path:resolve(out,`webview-${engine}.png`)});
  const remoteError=await page.evaluate(async url=>{
    await window.view(5,w=>w.string(url+'/fixture'));
    try {await window.view(7,w=>w.u32(45).string('document.URL'));return '';}
    catch(e) {return e.message;}
  },url);
  assert.match(remoteError,/origin-isolated/);
  await page.evaluate(async url=>{
    const {Reader,Writer}=await import('/rusterize-shaders/wire.js');
    const button=document.createElement('button');button.textContent='Открыть';document.body.append(button);
    button.style.cssText='position:fixed;bottom:5px;right:5px;z-index:2';
    button.onclick=()=>{window.openResult=window.services.execute(new Reader(new Writer().u32(12).string(url+'/fixture').finish()));};
  },url);
  const popupPromise=page.waitForEvent('popup');await page.getByRole('button',{name:'Открыть',exact:true}).click();
  const popup=await popupPromise;await popup.waitForURL(`${url}/fixture`);
  assert.equal(await popup.evaluate(()=>window.opener),null);await popup.close();await page.bringToFront();
  await page.evaluate(async()=>{await window.openResult;window.services.destroy();});
  assert.equal(await page.locator('iframe').count(),0);
  assert.deepEqual(errors,[]);
  console.log(`Web features OK: ${engine} ${browser.version()} — WGSL pixels, WebView lifecycle, async Rust services, files, cancellation`);
} finally {await browser.close();await new Promise(done=>server.close(done));}
