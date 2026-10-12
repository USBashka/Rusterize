import { Writer, encoder } from './wire.js';

function childBridge(secret) {
  addEventListener('message', async event => {
    if (event.source !== parent || event.data?.secret !== secret || event.data?.kind !== 'evaluate') return;
    const { request,script }=event.data;
    let value,ok=true;
    try { const result=await (0,eval)(script); value=JSON.stringify(result) ?? 'null'; }
    catch(error) { ok=false; value=String(error.message ?? error); }
    parent.postMessage({secret,request,ok,value},'*');
  });
}

export class WebViews {
  constructor(canvas,wake) {
    this.canvas=canvas; this.wake=wake; this.views=new Map(); this.events=[]; this.next=1;
    this.layer=document.createElement('div');
    this.layer.className='rusterize-webviews';
    Object.assign(this.layer.style,{position:'fixed',overflow:'hidden',pointerEvents:'none',zIndex:'1'});
    this.message=e=>this.receive(e);
    window.addEventListener('message',this.message);
  }
  update() {
    const r=this.canvas.getBoundingClientRect();
    Object.assign(this.layer.style,{left:`${r.left}px`,top:`${r.top}px`,width:`${r.width}px`,height:`${r.height}px`});
  }
  push(view,kind,value='',token=0) {
    if (this.views.get(view.id)!==view) return;
    if (encoder.encode(value).length>1024*1024) { kind=5; value='WebView result exceeds 1 MiB'; }
    if (this.events.length>=1024) throw new Error('Too many pending WebView events');
    this.events.push({view,kind,value,token}); this.wake();
  }
  poll() {
    while(this.events.length) {
      const e=this.events.shift();
      if (this.views.get(e.view.id)!==e.view) continue;
      return new Writer().u32(e.kind).u32(e.view.id).u32(e.token).string(e.value).finish();
    }
    return new Writer().u32(0).finish();
  }
  bounds(view,r) {
    const rect=r.rect();
    if (!rect.every(n=>Math.abs(n)<=32768) || rect[2]<1 || rect[3]<1) throw new Error('Invalid WebView bounds');
    view.bounds=rect; this.position(view);
  }
  position(view) {
    if (!view.frame) return;
    const [x,y,w,h]=view.bounds;
    Object.assign(view.frame.style,{left:`${x}px`,top:`${y}px`,width:`${w}px`,height:`${h}px`,display:view.visible?'block':'none'});
  }
  clearPending(view,reason) {
    for (const entry of view.pending.values()) {
      clearTimeout(entry.timer);
      if(reason) this.push(view,5,reason,entry.token);
    }
    view.pending.clear();
  }
  load(view,entry,history=true) {
    this.clearPending(view,'WebView document navigated before the script completed');
    view.frame?.remove();
    const frame=document.createElement('iframe'); view.frame=frame; view.local=entry.html!==undefined;
    view.secret=Array.from(crypto.getRandomValues(new Uint32Array(4)),n=>n.toString(16).padStart(8,'0')).join('');
    frame.title='WebView'; frame.referrerPolicy='no-referrer';
    // Never combine same-origin access with scripts. Local evaluation uses an
    // unprivileged message bridge inside an opaque-origin sandbox.
    frame.setAttribute('sandbox',view.javascript?'allow-scripts':'');
    Object.assign(frame.style,{position:'absolute',border:'0',pointerEvents:'auto',background:'white'});
    this.position(view);
    frame.addEventListener('load',()=>{
      if (view.frame===frame) this.push(view,2,entry.url);
    });
    frame.addEventListener('error',()=>{
      if(view.frame===frame) this.push(view,3,'WebView navigation failed');
    });
    if(view.local) {
      const bridge=view.javascript?`<script>(${childBridge.toString()})(${JSON.stringify(view.secret)})<\/script>`:'';
      frame.srcdoc=bridge+entry.html;
    } else frame.src=entry.url;
    if(history) {
      view.history.splice(view.index+1); view.history.push(entry);
      if(view.history.length>64) view.history.shift();
      view.index=view.history.length-1;
    }
    this.layer.append(frame); this.update();
  }
  receive(event) {
    for (const view of this.views.values()) {
      if(event.source!==view.frame?.contentWindow || event.data?.secret!==view.secret) continue;
      const entry=view.pending.get(event.data.request);
      if(!entry || typeof event.data.value!=='string' || typeof event.data.ok!=='boolean') return;
      clearTimeout(entry.timer); view.pending.delete(event.data.request);
      this.push(view,event.data.ok?4:5,event.data.value,entry.token); return;
    }
  }
  execute(r) {
    const id=r.u32(),action=r.u32();
    if(!id) throw new Error('WebView ID must be nonzero');
    let view=this.views.get(id);
    if(action===1) {
      if(view) throw new Error('WebView ID already exists');
      if(this.views.size>=32) throw new Error('At most 32 WebViews per application');
      view={id,pending:new Map(),history:[],index:-1};
      this.bounds(view,r); view.visible=!!r.u32(); view.javascript=!!r.u32();
      this.views.set(id,view);
      if(!this.layer.isConnected) document.body.append(this.layer);
      this.update();
      queueMicrotask(()=>this.push(view,1));
    } else if(action===2) {
      if(view) { this.clearPending(view); view.frame?.remove(); this.views.delete(id); this.events=this.events.filter(e=>e.view!==view); }
      if(!this.views.size) this.layer.remove();
    } else {
      if(!view) throw new Error('Unknown WebView ID');
      switch(action) {
        case 3: this.bounds(view,r); break;
        case 4: view.visible=!!r.u32(); this.position(view); break;
        case 5: {
          const url=r.string();
          if(url!=='about:blank' && !/^https?:\/\//.test(url)) throw new Error('Unsupported WebView URL');
          this.load(view,url==='about:blank'?{url,html:''}:{url}); break;
        }
        case 6: this.load(view,{url:'about:srcdoc',html:r.string()}); break;
        case 7: {
          const token=r.u32(),script=r.string();
          if(!view.javascript) throw new Error('WebView JavaScript is disabled');
          if(!view.local || !view.frame) throw new Error('Evaluate is available only in LoadHtml/about:blank documents; remote frames are origin-isolated');
          if(view.pending.size>=64) throw new Error('Too many pending WebView scripts');
          const request=this.next++;
          const timer=setTimeout(()=>{
            if(view.pending.delete(request)) this.push(view,5,'WebView script timed out or document navigated',token);
          },10000);
          view.pending.set(request,{token,timer});
          view.frame.contentWindow.postMessage({kind:'evaluate',secret:view.secret,request,script},'*');
          break;
        }
        case 8: view.frame?.focus(); break;
        case 9: if(view.index>=0) this.load(view,view.history[view.index],false); break;
        case 10: case 11: {
          const next=view.index+(action===10?-1:1);
          if(next>=0 && next<view.history.length) { view.index=next; this.load(view,view.history[next],false); }
          break;
        }
        default: throw new Error('Unknown WebView operation');
      }
    }
    r.done(); return new Uint8Array();
  }
  destroy() {
    for(const view of this.views.values()) { this.clearPending(view); view.frame?.remove(); }
    this.views.clear(); this.events=[]; this.layer.remove(); window.removeEventListener('message',this.message);
  }
}
