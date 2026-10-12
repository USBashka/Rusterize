import { Reader, Writer, encoder } from './wire.js';
import { WebViews } from './webviews.js';
import { Shaders } from './shaders.js';
import { Files } from './files.js';

export class BrowserServices {
  constructor(canvas,text,wake) {
    this.canvas=canvas; this.text=text; this.wake=wake; this.responses=[];
    this.cancelled=new Set(); this.closed=false; this.dialogs=new Set();
    this.webviews=new WebViews(canvas,wake); this.shaders=new Shaders(); this.files=new Files();
  }
  capabilities() {
    return 1|4|128|256|512|1024|(navigator.clipboard?.readText && navigator.clipboard?.writeText?2:0)|
      (document.fullscreenEnabled && document.documentElement.requestFullscreen?8:0);
  }
  sync(bytes) {
    const r=new Reader(bytes),op=r.u32(); let payload;
    if(op===1) payload=this.text.response(r.text());
    else if(op===2) payload=new Writer().u32(this.capabilities()).finish();
    else if(op===21) payload=this.webviews.poll();
    else if(op===22) {
      const app=r.u64(),index=this.responses.findIndex(item=>item.app===app);
      payload=index<0?new Writer().u32(0).finish():this.responses.splice(index,1)[0].bytes;
    } else if(op>=30 && op<=32) payload=this.shaders.execute(op,r);
    else throw new Error('Unsupported synchronous browser operation');
    r.done(); return payload;
  }
  cancel(app) { this.cancelled.add(app); this.responses=this.responses.filter(item=>item.app!==app); }
  request(app,id,bytes) {
    // execute starts before returning to WASM, preserving transient activation.
    let result;
    try { result=this.execute(new Reader(bytes)); } catch(error) { result=Promise.reject(error); }
    Promise.resolve(result).then(payload=>this.complete(app,id,0,payload),error=>{
      this.complete(app,id,1,encoder.encode(String(error.message ?? error)));
    });
  }
  complete(app,id,status,payload) {
    if(this.closed || this.cancelled.has(app)) return;
    if(payload.length>32*1024*1024+16384) { status=1; payload=encoder.encode('Browser response is too large'); }
    this.responses.push({app,bytes:new Writer().u32(1).u64(id).u32(status).raw(payload).finish()});
    this.wake();
  }
  async execute(r) {
    const op=r.u32(); let result=new Uint8Array();
    switch(op) {
      case 3: r.done(); if(!navigator.clipboard?.readText) throw new Error('Clipboard API requires HTTPS or localhost');
        return encoder.encode(await navigator.clipboard.readText());
      case 4: { const text=r.string(); r.done(); if(!navigator.clipboard?.writeText) throw new Error('Clipboard API requires HTTPS or localhost');
        await navigator.clipboard.writeText(text); return result; }
      case 5: {
        const cursor=['default','text','pointer','crosshair','move','ew-resize','ns-resize','none'][r.u32()];
        if(!cursor) throw new Error('Invalid cursor'); this.canvas.style.cursor=cursor; break;
      }
      case 6: {
        const state=r.u32(); r.done();
        if(state===3) {
          await document.documentElement.requestFullscreen();
          if(this.closed) {if(document.fullscreenElement) await document.exitFullscreen();}
          else this.fullscreen=true;
        }
        else if(state===0) { if(this.fullscreen && document.fullscreenElement) await document.exitFullscreen(); this.fullscreen=false; }
        else throw new Error('Browsers do not expose minimize/maximize; use Fullscreen or Normal');
        return result;
      }
      case 7: case 8: case 9:
        throw new Error('The browser does not grant native window sizing, positioning or always-on-top control');
      case 10: {
        const save=!!r.u32(),directory=!!r.u32(); r.string(); const name=r.string(); r.done();
        return this.files.pick(save,directory,name);
      }
      case 11: { const title=r.string(),message=r.string(); r.done(); await this.message(title,message); return result; }
      case 12: {
        const uri=r.string(); r.done();
        const url=new URL(uri);
        if(!['http:','https:','mailto:','tel:'].includes(url.protocol)) throw new Error('Browser URI must use HTTP(S), mailto or tel');
        if(!navigator.userActivation?.isActive) throw new Error('Opening a URI requires a user action');
        // Test the popup result while suppressing its opener before navigation.
        const opened=window.open('about:blank','_blank');
        if(!opened) throw new Error('The browser blocked the new tab');
        opened.opener=null; opened.location.replace(url.href); return result;
      }
      case 13: case 14: case 15: case 16: {
        const token=r.string(),data=op===14?r.take(r.u32()).slice():undefined; r.done();
        return this.files.execute(op,token,data);
      }
      case 20: return this.webviews.execute(r);
      default: throw new Error('Unknown asynchronous browser operation');
    }
    r.done(); return result;
  }
  message(title,message) {
    return new Promise(resolve=>{
      const dialog=document.createElement('dialog'),heading=document.createElement('h2'),text=document.createElement('p'),button=document.createElement('button');
      heading.textContent=title; text.textContent=message; text.style.whiteSpace='pre-wrap'; button.textContent='ОК';
      Object.assign(dialog.style,{maxWidth:'min(480px,80vw)',font:'16px/1.5 system-ui',border:'0',borderRadius:'12px',padding:'24px'});
      const close=()=>{this.dialogs.delete(close);dialog.remove();resolve();};
      this.dialogs.add(close); button.addEventListener('click',()=>dialog.close()); dialog.addEventListener('close',close,{once:true});
      dialog.append(heading,text,button); document.body.append(dialog); dialog.showModal(); button.focus();
    });
  }
  update() { this.webviews.update(); }
  destroy() {
    this.closed=true; this.responses=[]; for(const close of [...this.dialogs]) close();
    this.webviews.destroy(); this.shaders.destroy(); this.files.destroy();
    if(this.fullscreen && document.fullscreenElement) document.exitFullscreen().catch(()=>{});
  }
}
