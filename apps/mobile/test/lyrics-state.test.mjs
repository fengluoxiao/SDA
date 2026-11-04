import assert from 'node:assert/strict';
import {test} from 'node:test';
import {build} from 'esbuild';
const compiled=await build({entryPoints:['apps/mobile/src/use-track-lyrics.ts'],bundle:true,platform:'node',format:'cjs',external:['react','expo-document-picker','./lyric-files'],write:false});
const tick=()=>new Promise(resolve=>setImmediate(resolve));
function fixture(readFile){
 const cells=[],effects=[];let cursor=0,options={uri:'file:///song-a.m4a',metadataLyrics:undefined,active:true},reads=0;
 const changed=(a,b)=>!a||a.some((v,i)=>v!==b[i]);
 const React={
  useState(value){const i=cursor++;cells[i]??={value};return[cells[i].value,v=>{cells[i].value=typeof v==='function'?v(cells[i].value):v;}];},
  useRef(value){const i=cursor++;return cells[i]??={current:value};},
  useEffect(fn,deps){const i=cursor++;if(!cells[i]||changed(cells[i].deps,deps)){const old=cells[i];cells[i]={deps};effects.push(()=>{old?.cleanup?.();cells[i].cleanup=fn();});}},
  useCallback(fn,deps){const i=cursor++;if(!cells[i]||changed(cells[i].deps,deps))cells[i]={value:fn,deps};return cells[i].value;},
 };
 const mod={exports:{}};new Function('require','module','exports',compiled.outputFiles[0].text)(name=>name==='react'?React:name==='expo-document-picker'?{}:name.endsWith('lyric-files')?{readFileLyrics:async uri=>{reads++;return readFile(uri);}}:(()=>{throw Error(name);})(),mod,mod.exports);
 return{render(update={}){options={...options,...update};cursor=0;const api=mod.exports.useTrackLyrics(options);while(effects.length)effects.shift()();return api;},get reads(){return reads;}};
}
test('tab switches retain the current lyric document and do not re-read or reset its rows',async()=>{
 const f=fixture(async uri=>[{text:'[0:0]'+uri+'\n[0:5]second',source:'embedded'}]);f.render();await tick();let api=f.render();const original=api.document;assert.equal(original.lines.length,2);
 for(let i=0;i<3;i++){api=f.render({active:false});assert.equal(api.document,original);api=f.render({active:true});await tick();assert.equal(f.render().document,original);}
 assert.equal(f.reads,1);
 f.render({uri:'file:///song-b.m4a'});await tick();api=f.render();assert.notEqual(api.document,original);assert.equal(f.reads,2);assert(api.document.lines[0].text.includes('song-b'));
});
test('an unfinished lyric read cannot publish after leaving its page; returning resumes a valid load',async()=>{
 const pending=[],f=fixture(()=>new Promise(resolve=>pending.push(resolve)));f.render();assert.equal(pending.length,1);
 f.render({active:false});pending[0]([{text:'[0:0]obsolete',source:'embedded'}]);await tick();assert.equal(f.render().document,null);
 f.render({active:true});assert.equal(pending.length,2);pending[1]([{text:'[0:0]current',source:'embedded'}]);await tick();const api=f.render();assert.equal(api.document.lines[0].text,'current');assert.equal(api.loading,false);
});
