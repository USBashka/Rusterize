import {Writer} from './wire.js';
const MAX=32*1024*1024, MAX_PCM=256*1024*1024;
const empty=()=>new Uint8Array();
const float64=n=>{const b=new Uint8Array(8);new DataView(b.buffer).setFloat64(0,n,true);return b;};
const read64=r=>{const b=r.take(8),n=new DataView(b.buffer,b.byteOffset,8).getFloat64(0,true);if(!Number.isFinite(n))throw new Error('Invalid audio number');return n;};

export class Media {
  constructor(wake,wasmUrl) { this.wake=wake;this.wasmUrl=wasmUrl;this.next=1;this.clips=new Map();this.players=new Map();this.pending=new Map();this.closed=false; }
  context() {
    if(this.closed) throw new Error('Media services were destroyed');
    if(!this.audio) {
      const Audio=globalThis.AudioContext || globalThis.webkitAudioContext;
      if(!Audio) throw new Error('Web Audio is unavailable');
      this.audio=new Audio({latencyHint:'interactive'});
      this.audio.addEventListener('statechange',()=>{
        if(this.audio.state==='interrupted' || this.audio.state==='suspended') this.interrupt();
        this.wake();
      });
    }
    return this.audio;
  }
  async decodeAudio(bytes) {
    if(bytes.length===0 || bytes.length>MAX) throw new Error('Audio source must contain 1 byte to 32 MiB');
    if(this.clips.size>=16) throw new Error('Too many retained audio clips (16)');
    const context=this.context();
    // Web Audio decodes asynchronously on the browser media thread.
    const buffer=await context.decodeAudioData(bytes.slice().buffer);
    if(this.closed) throw new Error('Media services were destroyed');
    if(buffer.numberOfChannels<1 || buffer.numberOfChannels>2 || buffer.length*buffer.numberOfChannels*4>MAX_PCM || buffer.sampleRate<8000 || buffer.sampleRate>192000) throw new Error('Audio requires mono/stereo, 8–192 kHz and at most 256 MiB of PCM');
    const peaks=new Float32Array(512),channels=Array.from({length:buffer.numberOfChannels},(_,i)=>buffer.getChannelData(i));
    for(let start=0;start<buffer.length;start+=32768) {
      for(let i=start,end=Math.min(start+32768,buffer.length);i<end;i++) {
        const bin=Math.min(511,Math.floor(i*512/buffer.length));
        for(const channel of channels) {const sample=channel[i];if(!Number.isFinite(sample))throw new Error('Non-finite audio PCM');peaks[bin]=Math.max(peaks[bin],Math.abs(sample));}
      }
      // The envelope never monopolises the UI event loop on long songs.
      if(start%262144===0) await new Promise(resolve=>setTimeout(resolve,0));
      if(this.closed) throw new Error('Media services were destroyed');
    }
    if(this.clips.size>=16) throw new Error('Too many retained audio clips (16)');
    const id=this.next++;this.clips.set(id,buffer);
    return new Writer().u32(id).u32(buffer.sampleRate).u32(buffer.numberOfChannels).u32(buffer.length).raw(new Uint8Array(peaks.buffer)).finish();
  }
  decodeImage(bytes) {
    if(bytes.length>MAX) return Promise.reject(new Error('Image source exceeds 32 MiB'));
    if(this.pending.size>=4) return Promise.reject(new Error('Image worker queue is full'));
    if(!this.worker) {
      this.worker=new Worker(new URL('./media-worker.js',import.meta.url),{type:'module'});
      this.worker.onmessage=({data})=>{const entry=this.pending.get(data.id);this.pending.delete(data.id);if(entry)data.error?entry.reject(new Error(data.error)):entry.resolve(new Uint8Array(data.bytes));};
      this.worker.onerror=event=>{for(const entry of this.pending.values())entry.reject(new Error(event.message || 'Image worker failed'));this.pending.clear();this.worker.terminate();this.worker=null;};
    }
    const id=this.next++,copy=bytes.slice().buffer;
    return new Promise((resolve,reject)=>{this.pending.set(id,{resolve,reject});this.worker.postMessage({id,bytes:copy,url:this.wasmUrl},[copy]);});
  }
  position(player) {return player.state===1?Math.min(player.buffer.duration,player.offset+this.audio.currentTime-player.started):player.offset;}
  halt(player,state) {
    player.offset=this.position(player);player.state=state;
    if(player.source){player.source.onended=null;player.source.stop();player.source.disconnect();player.source=null;}
  }
  start(player) {
    const context=this.context();
    if(context.state!=='running') {
      if(!navigator.userActivation?.isActive) throw new Error('Audio requires a click or key press to unlock');
      context.resume().catch(error=>{player.state=5;player.error=String(error);this.wake();});
    }
    if(player.source) this.halt(player,2);
    if(player.offset>=player.buffer.duration) player.offset=0;
    const source=context.createBufferSource();source.buffer=player.buffer;source.connect(player.gain);
    player.source=source;player.started=context.currentTime;player.state=1;
    source.onended=()=>{if(player.source!==source)return;player.offset=player.buffer.duration;player.state=3;source.disconnect();player.source=null;this.wake();};
    source.start(0,player.offset);
  }
  sync(op,r) {
    if(op===80){this.interrupt();return empty();}
    const id=r.u32();
    if(op===70) {
      const buffer=this.clips.get(id);if(!buffer)throw new Error('Unknown audio clip');
      if(this.players.size>=32)throw new Error('Too many audio players (32)');
      const gain=this.context().createGain();gain.connect(this.audio.destination);
      const key=this.next++;this.players.set(key,{buffer,gain,source:null,state:0,offset:0,started:0});return new Writer().u32(key).finish();
    }
    if(op===79){this.clips.delete(id);return empty();}
    if(op===78) {
      const clip=this.clips.get(id);if(!clip)throw new Error('Unknown audio clip');
      const start=r.u32(),count=r.u32();if(count>32768)throw new Error('PCM request exceeds 32768 samples');
      const pcm=new Float32Array(count),channels=clip.numberOfChannels;
      for(let channel=0;channel<channels;channel++) {
        const data=clip.getChannelData(channel);
        for(let i=channel;i<count;i+=channels)pcm[i]=data[start+Math.floor(i/channels)] ?? 0;
      }
      return new Uint8Array(pcm.buffer);
    }
    const player=this.players.get(id);if(!player)throw new Error('Unknown audio player');
    switch(op) {
      case 71:this.start(player);break;
      case 72:this.halt(player,2);break;
      case 73:this.halt(player,0);player.offset=0;break;
      case 74:{const position=read64(r);if(position<0 || position>player.buffer.duration)throw new Error('Seek outside audio duration');const active=player.state===1;this.halt(player,player.state===0?0:2);player.offset=position;if(active){if(position<player.buffer.duration)this.start(player);else player.state=3;}break;}
      case 75:{const gain=r.f32();if(gain<0 || gain>1)throw new Error('Invalid audio volume');player.gain.gain.value=gain;break;}
      case 76:return new Writer().u32(player.state).raw(float64(this.position(player))).raw(float64(player.buffer.duration)).raw(float64((this.audio.baseLatency||0)+(this.audio.outputLatency||0))).finish();
      case 77:this.halt(player,0);player.gain.disconnect();this.players.delete(id);break;
      default:throw new Error('Unknown audio operation');
    }
    return empty();
  }
  interrupt() {for(const player of this.players.values())if(player.state===1)this.halt(player,4);}
  destroy() {
    this.closed=true;this.interrupt();for(const player of this.players.values())player.gain.disconnect();
    this.players.clear();this.clips.clear();this.audio?.close().catch(()=>{});this.worker?.terminate();
    for(const entry of this.pending.values())entry.reject(new Error('Media services were destroyed'));this.pending.clear();
  }
}

function name(value) {if(!/^[A-Za-z0-9_.-]{1,120}$/.test(value) || value==='.' || value==='..' || value.endsWith('.') || /^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)/i.test(value))throw new Error('Invalid storage name');}
export class Storage {
  constructor(){this.database=null;this.closed=false;}
  open(){
    if(this.closed)return Promise.reject(new Error('Storage was destroyed'));
    if(!this.database)this.database=new Promise((resolve,reject)=>{
      const request=indexedDB.open('rusterize-data-v1',1);
      request.onupgradeneeded=()=>request.result.createObjectStore('data');
      request.onerror=()=>reject(request.error);
      request.onblocked=()=>reject(new Error('Persistent storage upgrade is blocked'));
      request.onsuccess=()=>{const db=request.result;db.onversionchange=()=>db.close();if(this.closed){db.close();reject(new Error('Storage was destroyed'));}else resolve(db);};
    });
    return this.database;
  }
  async execute(op,app,key,bytes){
    name(app);name(key);if(bytes && bytes.length>MAX)throw new Error('Storage value exceeds 32 MiB');
    const db=await this.open();if(this.closed)throw new Error('Storage was destroyed');
    return new Promise((resolve,reject)=>{
      const tx=db.transaction('data',op===42?'readonly':'readwrite'),store=tx.objectStore('data');
      let result=empty();const request=op===42?store.get([app,key]):store.put(bytes,[app,key]);
      request.onsuccess=()=>{if(op===42){const bytes=request.result;if(bytes && bytes.length>MAX){tx.abort();return;}result=bytes===undefined?new Uint8Array([0]):new Writer().raw(new Uint8Array([1])).raw(bytes).finish();}};
      tx.oncomplete=()=>resolve(result);
      tx.onerror=tx.onabort=()=>reject(tx.error || new Error('Storage transaction failed or quota exceeded'));
    });
  }
  destroy(){this.closed=true;this.database?.then(db=>db.close()).catch(()=>{});}
}
