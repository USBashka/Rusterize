import { Writer } from './wire.js';
const MAX=32*1024*1024;
const paths=values=>{ const w=new Writer().u32(values.length); for(const value of values) w.string(value); return w.finish(); };

export class Files {
  constructor() { this.entries=new Map(); this.next=1; this.cleanups=new Set(); this.closed=false; }
  token(entry) {
    if(this.closed) throw new Error('Application was destroyed');
    if(this.entries.size>=4096) throw new Error('Too many retained browser file tokens');
    const token=`rusterize-file:${this.next++}/${encodeURIComponent(entry.name || 'file')}`;
    this.entries.set(token,entry); return token;
  }
  get(token) { const entry=this.entries.get(token); if(!entry) throw new Error('Unknown or released browser file token'); return entry; }
  input(directory) {
    return new Promise((resolve,reject)=>{
      const input=document.createElement('input'); input.type='file'; input.hidden=true;
      if(directory) { input.webkitdirectory=true; input.multiple=true; }
      const cleanup=()=>{ input.remove(); this.cleanups.delete(cancel); };
      const cancel=()=>{ cleanup(); reject(new Error('Application was destroyed')); };
      this.cleanups.add(cancel);
      input.addEventListener('cancel',()=>{cleanup();resolve([]);},{once:true});
      input.addEventListener('change',()=>{const files=[...input.files];cleanup();resolve(files);},{once:true});
      document.body.append(input); input.click();
    });
  }
  async pick(save,directory,name) {
    if(save && directory) throw new Error('Save cannot select a directory');
    try {
      if(directory && window.showDirectoryPicker) {
        const handle=await window.showDirectoryPicker({mode:'read'});
        return paths([this.token({name:handle.name,handle,children:new Map()})]);
      }
      if(save) {
        if(window.showSaveFilePicker) {
          const handle=await window.showSaveFilePicker({suggestedName:name || 'document'});
          return paths([this.token({name:handle.name,handle})]);
        }
        // Selection alone does not download or write. write_file performs the save.
        const chosen=window.prompt('Имя файла',name || 'document');
        return paths(chosen===null?[]:[this.token({name:chosen || 'document',download:true})]);
      }
      if(!directory && window.showOpenFilePicker) {
        const handles=await window.showOpenFilePicker({multiple:false});
        return paths(handles.map(handle=>this.token({name:handle.name,handle})));
      }
      const files=await this.input(directory);
      if(!directory) return paths(files.map(file=>this.token({name:file.name,file})));
      if(!files.length) return paths([]);
      const root={name:files[0].webkitRelativePath.split('/')[0],children:new Map(),virtual:true};
      for(const file of files) {
        const names=file.webkitRelativePath.split('/').slice(1); let parent=root;
        for(const name of names.slice(0,-1)) {
          if(!parent.children.has(name)) parent.children.set(name,{name,children:new Map(),virtual:true});
          parent=parent.children.get(name);
        }
        parent.children.set(file.name,{name:file.name,file});
      }
      return paths([this.token(root)]);
    } catch(error) { if(error.name==='AbortError') return paths([]); throw error; }
  }
  async execute(op,token,data) {
    const entry=this.get(token);
    if(op===16) { this.release(token); return new Uint8Array(); }
    if(op===15) {
      if(!entry.children) throw new Error('Token is not a directory');
      if(entry.handle) {
        let count=0; const current=new Map();
        for await(const [name,handle] of entry.handle.entries()) {
          if(++count>1024) throw new Error('Directory listing exceeds 1024 entries');
          const old=entry.children.get(name);
          if(old && old.handle.kind===handle.kind) {old.handle=handle;current.set(name,old);}
          else current.set(name,{name,handle,...(handle.kind==='directory'?{children:new Map()}: {})});
        }
        for(const [name,child] of entry.children) if(current.get(name)!==child && child.token && this.entries.has(child.token)) this.release(child.token);
        entry.children=current;
      }
      if(entry.children.size>1024) throw new Error('Directory listing exceeds 1024 entries');
      const tokens=[];
      for(const child of entry.children.values()) {
        if(!child.token || !this.entries.has(child.token)) child.token=this.token(child);
        tokens.push(child.token);
      }
      return paths(tokens);
    }
    if(entry.children) throw new Error('Token is a directory');
    if(op===13) {
      const file=entry.handle?await entry.handle.getFile():entry.file;
      if(!file) throw new Error('This save token has not been written');
      if(file.size>MAX) throw new Error('Browser files are limited to 32 MiB per operation');
      return new Uint8Array(await file.arrayBuffer());
    }
    if(op===14) {
      if(data.length>MAX) throw new Error('Browser files are limited to 32 MiB per operation');
      if(entry.handle) {
        const stream=await entry.handle.createWritable();
        try {
          if(this.closed) throw new Error('Application was destroyed');
          await stream.write(data);
          if(this.closed) throw new Error('Application was destroyed');
          await stream.close();
        }
        catch(error) { await stream.abort().catch(()=>{}); throw error; }
      } else {
        if(!entry.download) throw new Error('This selected file is read-only; choose a save token');
        entry.file=new File([data],entry.name);
        const url=URL.createObjectURL(entry.file),a=document.createElement('a');
        a.href=url; a.download=entry.name; a.hidden=true; document.body.append(a); a.click(); a.remove();
        const cleanup=()=>{URL.revokeObjectURL(url);this.cleanups.delete(cleanup);};
        this.cleanups.add(cleanup); setTimeout(cleanup,1000);
      }
      return new Uint8Array();
    }
    throw new Error('Unknown browser file operation');
  }
  release(token) {
    const entry=this.get(token);
    if(entry.children) for(const child of entry.children.values()) if(child.token && this.entries.has(child.token)) this.release(child.token);
    this.entries.delete(token);
  }
  destroy() { this.closed=true; for(const cleanup of [...this.cleanups]) cleanup(); this.cleanups.clear(); this.entries.clear(); }
}
