import assert from 'node:assert/strict';
import {test} from 'node:test';
import {build} from 'esbuild';
const bundle=await build({entryPoints:['apps/mobile/src/use-lyrics-follow.ts'],bundle:true,platform:'node',format:'cjs',external:['react'],write:false});
function fixture(){
 const cells=[],effects=[],timers=new Map(),calls=[];let cursor=0,dirty=false,now=0,id=0;
 const document={format:'lrc',timed:true,source:{text:''},lines:[{id:'a',text:'long line',startMs:0,endMs:60000,words:[]},{id:'b',text:'second',startMs:60000,endMs:120000,words:[]},{id:'c',text:'third',startMs:120000,endMs:180000,words:[]}]};
 let options={active:true,document,positionMs:1000};
 const same=(a,b)=>a&&a.length===b.length&&a.every((v,i)=>Object.is(v,b[i]));
 const hooks={useRef(v){const i=cursor++;return cells[i]??={current:v};},useState(v){const i=cursor++;cells[i]??={value:v};return[cells[i].value,n=>{const next=typeof n==='function'?n(cells[i].value):n;if(!Object.is(next,cells[i].value)){cells[i].value=next;dirty=true;}}];},useCallback(fn,deps){const i=cursor++;if(!cells[i]||!same(cells[i].deps,deps))cells[i]={value:fn,deps};return cells[i].value;},useEffect(fn,deps){const i=cursor++;if(!cells[i]||!same(cells[i].deps,deps)){const old=cells[i];cells[i]={deps};effects.push(()=>{old?.cleanup?.();cells[i].cleanup=fn();});}}};hooks.useLayoutEffect=hooks.useEffect;
 const mod={exports:{}};new Function('require','module','exports','setTimeout','clearTimeout',bundle.outputFiles[0].text)(name=>{assert.equal(name,'react');return hooks;},mod,mod.exports,(fn,ms)=>{const token=++id;timers.set(token,{at:now+ms,fn});return token;},token=>timers.delete(token));
 const render=(update={})=>{options={...options,...update};for(let n=0;n<10;n++){dirty=false;cursor=0;const api=mod.exports.useLyricsFollow(options);while(effects.length)effects.shift()();if(!dirty)return api;}throw Error('hook did not settle');};
 const advance=ms=>{const end=now+ms;for(;;){const next=[...timers.entries()].filter(([,t])=>t.at<=end).sort((a,b)=>a[1].at-b[1].at)[0];if(!next)break;timers.delete(next[0]);now=next[1].at;next[1].fn();}now=end;};
 let api=render();api.scrollRef.current={scrollTo:value=>calls.push(value)};api.onViewportLayout(200);api=render();api.onLineLayout(0,20);api.onLineLayout(1,220);api.onLineLayout(2,420);calls.length=0;
 return{render,advance,calls,timers,api,dispose(){for(const cell of cells)cell?.cleanup?.();}};
}
test('paused, same long lyric: manual scrolling returns at 4s without a playback tick or a line change',()=>{
 const f=fixture(),h=f.api.scrollHandlers;h.onScrollBeginDrag();h.onScrollEndDrag();f.advance(3999);assert.equal(f.calls.length,0);f.advance(1);assert.equal(f.calls.length,1);assert.equal(f.calls[0].y,0);assert.equal(f.calls[0].animated,true);f.dispose();
});
test('a playing song returns to its latest timestamp, rather than where the drag started',()=>{
 const f=fixture(),h=f.api.scrollHandlers;h.onScrollBeginDrag();h.onScrollEndDrag();f.render({positionMs:65000});f.advance(4000);assert.equal(f.calls.at(-1).y,152);f.dispose();
});
test('long dragging and momentum do not get interrupted; 4s starts after scrolling settles',()=>{
 const f=fixture(),h=f.api.scrollHandlers;h.onScrollBeginDrag();f.advance(6000);assert.equal(f.calls.length,0);h.onScrollEndDrag();f.advance(100);h.onMomentumScrollBegin();f.advance(6000);assert.equal(f.calls.length,0);h.onMomentumScrollEnd();f.advance(3999);assert.equal(f.calls.length,0);f.advance(1);assert.equal(f.calls.length,1);f.dispose();
});
test('a second gesture resets the return deadline',()=>{
 const f=fixture(),h=f.api.scrollHandlers;h.onScrollBeginDrag();h.onScrollEndDrag();f.advance(2500);h.onScrollBeginDrag();h.onScrollEndDrag();f.advance(1500);assert.equal(f.calls.length,0);f.advance(2500);assert.equal(f.calls.length,1);f.dispose();
});
test('tab return immediately follows the current time, even when the previous line index is unchanged',()=>{
 for(const positionMs of [1000,125000]){
  const f=fixture();f.api.scrollHandlers.onScrollBeginDrag();f.api.scrollHandlers.onScrollEndDrag();f.render({active:false});assert.equal(f.timers.size,0);
  f.render({positionMs});assert.equal(f.calls.length,0);f.render({active:true});assert.equal(f.calls.length,1);assert.equal(f.calls[0].y,positionMs===1000?0:352);assert.equal(f.calls[0].animated,false);f.dispose();
 }
});
test('programmatic momentum is ignored and unmount cancels pending timers',()=>{
 const f=fixture();f.api.scrollHandlers.onMomentumScrollBegin();f.api.scrollHandlers.onMomentumScrollEnd();assert.equal(f.timers.size,0);f.api.scrollHandlers.onScrollBeginDrag();f.api.scrollHandlers.onScrollEndDrag();f.dispose();f.advance(5000);assert.equal(f.calls.length,0);assert.equal(f.timers.size,0);
});
test('a paused instrumental gap still returns to the matching timeline section after 4s',()=>{
 const f=fixture(),document={format:'ttml',source:{text:''},timed:true,lines:[{id:'a',text:'一',startMs:0,endMs:2000,words:[]},{id:'b',text:'二',startMs:10000,endMs:12000,words:[]}]};
 const api=f.render({document,positionMs:5000});f.calls.length=0;api.scrollHandlers.onScrollBeginDrag();api.scrollHandlers.onScrollEndDrag();f.advance(4000);assert.equal(f.calls.length,1);assert.equal(f.calls[0].y,0);f.dispose();
});
