// Test adapter only: Windows Edge's native HLS demuxer rejects FLAC. Feed the
// same authenticated fMP4 bytes to its real FLAC/MSE decoder instead. This does
// not mock audio time, readiness, play(), pause(), or the synchronization code.
module.exports=function installMseTestAdapter(){
 const proto=HTMLMediaElement.prototype,src=Object.getOwnPropertyDescriptor(proto,'src'),load=proto.load,remove=proto.removeAttribute;
 const sessions=new WeakMap();
 Object.defineProperty(proto,'src',{...src,set(value){
  sessions.get(this)?.abort();sessions.delete(this);
  if(!String(value).includes('/hls/')){src.set.call(this,value);return;}
  const abort=new AbortController(),media=new MediaSource(),url=URL.createObjectURL(media);sessions.set(this,abort);
  const audio=this,manifest=new URL(value,location.href);src.set.call(audio,url);
  media.addEventListener('sourceopen',async()=>{
   try{
    const buffer=media.addSourceBuffer('audio/mp4; codecs="flac"');
    const get=async path=>{const r=await fetch(new URL(path,manifest),{signal:abort.signal});if(!r.ok)throw Error('test media HTTP '+r.status);return r;};
    const append=data=>new Promise((resolve,reject)=>{buffer.addEventListener('updateend',resolve,{once:true});buffer.addEventListener('error',reject,{once:true});buffer.appendBuffer(data);});
    await append(await (await get('init.mp4')).arrayBuffer());let next=0;
    while(!abort.signal.aborted&&media.readyState==='open'){
     const text=await (await get(manifest)).text(),segments=text.split('\n').filter(s=>/^\d+\.m4s$/.test(s));
     for(const segment of segments){const sequence=Number(segment.split('.')[0]);if(sequence<next)continue;await append(await (await get(segment)).arrayBuffer());next=sequence+1;}
     if(text.includes('#EXT-X-ENDLIST')){media.endOfStream();break;}
     await new Promise(resolve=>setTimeout(resolve,100));
    }
   }catch(e){if(!abort.signal.aborted)window.__audioErrors.push({adapterError:String(e)});}
  },{once:true});
  abort.signal.addEventListener('abort',()=>URL.revokeObjectURL(url),{once:true});
 }});
 proto.load=function(){if(!sessions.has(this))load.call(this);};
 proto.removeAttribute=function(name){if(name==='src'){sessions.get(this)?.abort();sessions.delete(this);}return remove.call(this,name);};
};
