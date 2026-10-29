let ready;
self.onmessage = async ({data}) => {
  try {
    if (!ready) ready=(async()=>{
      const fail = () => { throw new Error('UI services are unavailable in the image worker'); };
      const response = await fetch(data.url);
      if (!response.ok) throw new Error(`Image worker WASM: HTTP ${response.status}`);
      return (await WebAssembly.instantiate(await response.arrayBuffer(), {rusterize:{service:fail,service_read:fail,request:fail,cancel_requests:fail}})).instance;
    })().catch(error=>{ready=null;throw error;});
    const instance=await ready;
    const e=instance.exports, input=new Uint8Array(data.bytes);
    if (!e.rusterize_image_alloc) throw new Error('Build the application with image-codecs');
    const pointer=e.rusterize_image_alloc(input.length);
    if (!pointer) throw new Error('Image source exceeds 32 MiB');
    new Uint8Array(e.memory.buffer,pointer,input.length).set(input);
    const length=e.rusterize_image_decode(pointer,input.length), at=e.rusterize_image_result();
    if(length<4 || length>64*1024*1024+12) throw new Error('Invalid image decoder response');
    const result=new Uint8Array(e.memory.buffer,at,length).slice();
    if(new DataView(result.buffer).getUint32(0,true)!==0) throw new Error(new TextDecoder().decode(result.subarray(4)));
    const bytes=result.slice(4).buffer;
    self.postMessage({id:data.id,bytes},[bytes]);
  } catch(error) { self.postMessage({id:data.id,error:String(error.message ?? error)}); }
};
