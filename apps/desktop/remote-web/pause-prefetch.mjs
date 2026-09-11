// Warm a small, authenticated HTTP media cache without touching the audio clock.
export function pauseSegments(playlist,position,limit=8){
  let time=0,duration=0;const segments=[];
  const sequence=Number(playlist.match(/^#EXT-X-MEDIA-SEQUENCE:(\d+)/m)?.[1]??0);
  // SDA uses one-second segments; only the final segment may be shorter.
  time=sequence;
  for(const line of playlist.split(/\r?\n/)){
    if(line.startsWith('#EXTINF:'))duration=Number(line.slice(8).split(',')[0]);
    else if(/^\d+\.m4s$/.test(line)&&duration>0){
      if(time+duration>position&&time<position+limit)segments.push(line);
      time+=duration;duration=0;
    }
  }
  return segments.slice(0,limit+1);
}
export class PausePrefetch {
  constructor(fetcher=fetch){this.fetcher=fetcher;this.done=new Map();this.controller=null;this.stream='';this.nextAt=0;}
  cancel(){this.controller?.abort();this.controller=null;this.done.clear();this.stream='';this.nextAt=0;}
  async update(owner,paused){
    if(!paused||owner.closed||owner.mediaPending||!owner.audio){if(this.controller)this.cancel();return;}
    const stream=owner.lastMediaInfo?.stream;
    if(!/^\/hls\/[a-f0-9]{32}\/e\d+\/index\.m3u8$/.test(stream??''))return;
    if(stream!==this.stream){this.cancel();this.stream=stream;}
    if(this.controller||Date.now()<this.nextAt)return;
    const controller=this.controller=new AbortController();const options={credentials:'same-origin',signal:controller.signal};
    const timeout=setTimeout(()=>controller.abort(),10000);
    try{
      const response=await this.fetcher(stream,{...options,cache:'no-store'});
      if(!response.ok)return;
      const names=pauseSegments(await response.text(),owner.audio.currentTime);
      const wanted=new Set(['init.mp4',...names]);for(const name of this.done.keys())if(!wanted.has(name))this.done.delete(name);
      for(const name of ['init.mp4',...names]){
        if(Date.now()-(this.done.get(name)??0)<25000)continue;
        const result=await this.fetcher(stream.replace('index.m3u8',name),{...options,cache:'default'});
        if(!result.ok)break;
        await result.arrayBuffer();if(controller.signal.aborted)return;
        this.done.set(name,Date.now());
      }
    }catch{/* Native HLS remains responsible for playback and its own retries. */}
    finally{clearTimeout(timeout);if(this.controller===controller){this.controller=null;this.nextAt=Date.now()+5000;}}
  }
}
