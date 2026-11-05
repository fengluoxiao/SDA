import assert from 'node:assert/strict';
import {test} from 'node:test';
import {build} from 'esbuild';
const compile = async (entry, external=[]) => (await build({entryPoints:[entry],bundle:true,platform:'node',format:'cjs',write:false,external})).outputFiles[0].text;
const load=(code,require=()=>{})=>{const m={exports:{}};new Function('require','module','exports',code)(require,m,m.exports);return m.exports;};
const model=load(await compile('apps/mobile/src/lyric-karaoke.ts'));
const lyrics=load(await compile('apps/mobile/src/mobile-lyrics.ts'));
const {karaokeWordRanges,karaokeLayout,karaokeFraction,karaokeClockNeedsSync,karaokeHeldNote}=model;

test('TTML and enhanced LRC fill within each character continuously, with no fabricated timing for ordinary LRC',()=>{
 const sources=['[0:3.0]<0:3.0>风<0:3.5>吹<0:4.0>过\n[0:6.0]下一句','<tt xmlns:itunes="http://music.apple.com/lyric"><body><p begin="3.0" end="6.0"><span begin="3.0" end="3.5">风</span><span begin="3.5" end="4.0">吹</span><span begin="4.0" end="6.0">过</span></p></body></tt>','<tt><body><p begin="3s" end="6s"><span begin="0s" end="0.5s">风</span><span begin="0.5s" end="1s">吹</span><span begin="1s" end="3s">过</span></p></body></tt>'];
 for(const source of sources){const line=lyrics.parseLyrics({text:source}).lines[0],ranges=karaokeWordRanges(line);assert.equal(ranges.length,3);
  assert.deepEqual([3000,3125,3250,3375,3500].map(t=>karaokeFraction(ranges[0].startMs,ranges[0].endMs,t)),[0,.25,.5,.75,1]);
  assert.equal(karaokeFraction(ranges[1].startMs,ranges[1].endMs,3750),.5);assert.equal(karaokeFraction(ranges[0].startMs,ranges[0].endMs,3100),.2);
 }
 assert.deepEqual(karaokeWordRanges(lyrics.parseLyrics({text:'[0:3.0]风吹过\n[0:6.0]Next'}).lines[0]),[]);
});
test('timed text maps whitespace and Unicode to the original displayed paragraph without splitting its native shaping',()=>{
 const line={text:'風 😀 hello',words:[{text:' 風 ',startMs:0,endMs:500},{text:'😀 ',startMs:500,endMs:1000},{text:'hello ',startMs:1000,endMs:2000}]};
 const ranges=karaokeWordRanges(line);assert.deepEqual(ranges.map(r=>line.text.slice(r.from,r.to)),['風 ','😀 ','hello']);
 assert.deepEqual(karaokeWordRanges({...line,text:'different text'}),[]);
});
test('actual native glyph widths position the reveal correctly for proportional fonts and right-aligned singers',()=>{
 const ranges=[{from:0,to:1,startMs:0,endMs:1000},{from:1,to:2,startMs:1000,endMs:2000}],lines=[{text:'Wi',x:76,y:0,width:24,height:40}];
 const pending=karaokeLayout('Wi',ranges,lines,{});assert.deepEqual(pending.requests,[{id:'0:1',text:'W'}]);assert.deepEqual(pending.pieces,[]);
 const ready=karaokeLayout('Wi',ranges,lines,{'0:1':20});assert.deepEqual(ready.pieces.map(p=>[p.x,p.width]),[[76,20],[96,4]]);
 assert.equal(ready.pieces[0].width*karaokeFraction(0,1000,250),5);
});
test('a word wrapped over two native lines fills its first line then the second by actual glyph width',()=>{
 const result=karaokeLayout('abcd',[{from:0,to:4,startMs:1000,endMs:3000}],[{text:'ab',x:0,y:0,width:30,height:40},{text:'cd',x:0,y:40,width:10,height:40}],{});
 assert.deepEqual(result.pieces.map(p=>[p.y,p.startMs,p.endMs]),[[0,1000,2500],[40,2500,3000]]);
 assert.equal(karaokeFraction(result.pieces[1].startMs,result.pieces[1].endMs,2750),.5);
});
test('native clock keeps normal audio ticks smooth but resynchronizes pause, forward/backward seeks and line changes',()=>{
 const anchor={positionMs:3000,reportMs:3200,at:0,running:true,endMs:10000};
 assert.equal(karaokeClockNeedsSync(anchor,3500,500,true,10000),false);
 assert.equal(karaokeClockNeedsSync(anchor,3540,500,true,10000),false);
 for(const [position,now,running,end] of [[3100,500,true,10000],[7000,500,true,10000],[3500,500,false,10000],[3500,500,true,12000]])assert.equal(karaokeClockNeedsSync(anchor,position,now,running,end),true);
});
const componentCode=await compile('apps/mobile/src/KaraokeText.tsx',['react','react-native','./lyric-karaoke']);
function fixture({positionMs=250,playing=true,active=true,providedLine}={}){
 const cells=[],effects=[],calls=[];let cursor=0,now=0,appStateListener,options={positionMs,playing,active};
 const same=(a,b)=>a&&b&&a.length===b.length&&a.every((v,i)=>Object.is(v,b[i]));
 const React={createElement:(type,props,...children)=>({type,props:{...props,children}}),useRef(value){const i=cursor++;return cells[i]??={current:value};},useState(initial){const i=cursor++;cells[i]??={value:initial};return[cells[i].value,next=>{cells[i].value=typeof next==='function'?next(cells[i].value):next;}];},useMemo(fn,deps){const i=cursor++;if(!cells[i]||!same(cells[i].deps,deps))cells[i]={value:fn(),deps};return cells[i].value;},useEffect(fn,deps){const i=cursor++;if(!cells[i]||!same(cells[i].deps,deps)){const old=cells[i];cells[i]={deps};effects.push(()=>{old?.cleanup?.();cells[i].cleanup=fn();});}}};React.useLayoutEffect=React.useEffect;
 class Value{constructor(value){this.value=value;}setValue(value){this.value=value;calls.push(['set',value]);}interpolate(config){return{value:this,config};}}
 const native={View:'View',Text:'Text',Animated:{Value,View:'AnimatedView',Text:'AnimatedText',timing(value,config){return{start(){calls.push(['start',config]);},stop(){calls.push(['stop']);}};}},Easing:{linear:'linear'},StyleSheet:{absoluteFill:{position:'absolute',left:0,top:0,right:0,bottom:0},flatten:value=>Array.isArray(value)?Object.assign({},...value.flat(Infinity).filter(Boolean)):value},AppState:{currentState:'active',addEventListener(_name,listener){appStateListener=listener;return{remove(){calls.push(['removeAppState']);}};}}};
 const api=load(componentCode,name=>name==='react'?React:name==='react-native'?native:model);
 const line=providedLine??{id:'line',text:'Wi',startMs:0,endMs:2000,words:[{text:'W',startMs:0,endMs:1000},{text:'i',startMs:1000,endMs:2000}]};
 const props={line,style:{fontSize:28,lineHeight:40,fontWeight:'700',textAlign:'right'},testID:'karaoke-text'};
 const render=(update={})=>{options={...options,...update};cursor=0;const result=api.KaraokeText({...props,...options});const root=typeof result.type==='function'?result.type(result.props):result;while(effects.length)effects.shift()();return root;};
 const find=(node,predicate)=>{if(!node||typeof node!=='object')return; if(predicate(node))return node;for(const child of node.props?.children?.flat(Infinity)??[]){const value=find(child,predicate);if(value)return value;}};
 const originalPerformance=globalThis.performance;
 return{render,calls,line,find,React,api,appState(state){appStateListener?.(state);},setNow(value){now=value;},withClock(fn){Object.defineProperty(globalThis,'performance',{configurable:true,value:{now:()=>now}});try{return fn();}finally{Object.defineProperty(globalThis,'performance',{configurable:true,value:originalPerformance});}},dispose(){for(const cell of cells)cell?.cleanup?.();}};
}
test('native glyph reveal keeps paragraph layout and click/accessibility surfaces stable while filling inside a word',()=>{
 const f=fixture();f.withClock(()=>{
  let root=f.render();root.props.onLayout({nativeEvent:{layout:{width:100}}});f.find(root,n=>n.props.testID==='karaoke-text').props.onTextLayout({nativeEvent:{lines:[{text:'Wi',x:76,y:0,width:24,height:40}]}});
  root=f.render();const measure=f.find(root,n=>n.props.numberOfLines===1);assert.equal(measure.props.children[0],'W');measure.props.onTextLayout({nativeEvent:{lines:[{width:20}]}});
  root=f.render();const wordNode=f.find(root,n=>n.props.piece?.id==='0:0'),word=wordNode.type(wordNode.props);
  assert.equal(word.props.style.left,76);assert.equal(word.props.style.width,20);
  const moving=word.props.children[0],fixed=moving.props.children[0],reveal=moving.props.style.transform[0].translateX,compensate=fixed.props.style.transform[0].translateX;
  assert.deepEqual(reveal.config.inputRange,[0,1000]);assert.deepEqual(reveal.config.outputRange,[-20,0]);assert.deepEqual(compensate.config.outputRange,[20,0]);
  for(const progress of [0,.25,.5,.75,1])assert.equal((-20+20*progress)+(20-20*progress),0,'bright glyphs remain stationary while the clipping edge moves');
  assert.equal(f.find(root,n=>n.props.testID==='karaoke-text').props.children[0],'Wi');
  const overlay=f.find(root,n=>n.props.importantForAccessibility==='no-hide-descendants');assert.equal(overlay.props.pointerEvents,'none');
  const animation=f.calls.find(c=>c[0]==='start')[1];assert.equal(animation.useNativeDriver,true);assert.equal(animation.easing,'linear');assert.equal(animation.duration,1750);
 });f.dispose();assert(f.calls.some(c=>c[0]==='stop'));assert(f.calls.some(c=>c[0]==='removeAppState'));
});
test('pause freezes partial fill immediately, resume continues, and seeking rewinds the same native clock',()=>{
 const f=fixture();f.withClock(()=>{
  f.render();f.setNow(100);f.render({positionMs:350});assert.equal(f.calls.filter(c=>c[0]==='start').length,1,'normal status ticks must not restart the native animation');
  f.render({positionMs:400,playing:false});assert.deepEqual(f.calls.at(-1),['set',400]);
  f.render({positionMs:400,playing:true});assert.equal(f.calls.at(-1)[1].duration,1600);
  f.render({positionMs:100});assert(f.calls.some(c=>c[0]==='set'&&c[1]===100));
 });f.dispose();
});
test('hidden lyrics do not start a native animation',()=>{const f=fixture({active:false});const root=f.render();assert.equal(root.type,'Text');assert.equal(root.props.children[0],'Wi');assert.equal(f.calls.length,0);f.dispose();});

test('backgrounding stops the native fill and foreground return uses the latest audio position',()=>{
 const f=fixture();f.withClock(()=>{
  f.render();f.appState('background');f.render();assert.deepEqual(f.calls.at(-1),['set',250]);
  f.appState('active');f.render({positionMs:1250});assert.equal(f.calls.at(-1)[1].duration,750);
 });f.dispose();
});
test('rotation discards stale glyph measurements and keeps the original paragraph alignment',()=>{
 const f=fixture();f.withClock(()=>{
  let root=f.render();root.props.onLayout({nativeEvent:{layout:{width:100}}});f.find(root,n=>n.props.testID==='karaoke-text').props.onTextLayout({nativeEvent:{lines:[{text:'Wi',x:76,y:0,width:24,height:40}]}});
  root=f.render();const stale=f.find(root,n=>n.props.numberOfLines===1);
  root.props.onLayout({nativeEvent:{layout:{width:200}}});root=f.render();stale.props.onTextLayout({nativeEvent:{lines:[{width:999}]}});
  root=f.render();assert.equal(f.find(root,n=>n.props.piece),undefined,'old-width prefix measurements must not enter the new layout');
  f.find(root,n=>n.props.testID==='karaoke-text').props.onTextLayout({nativeEvent:{lines:[{text:'Wi',x:176,y:0,width:24,height:40}]}});
  root=f.render();f.find(root,n=>n.props.numberOfLines===1).props.onTextLayout({nativeEvent:{lines:[{width:20}]}});
  root=f.render();assert.equal(f.find(root,n=>n.props.piece?.id==='0:0').props.piece.x,176);
 });f.dispose();
});

test('held-note glow swells and releases on authored timing without flashing during fast lyrics',()=>{
 const note=karaokeHeldNote('光',10000,12500);assert(note);
 assert.equal(note.inputRange[0],10000);assert.equal(note.inputRange.at(-1),12500);
 assert.equal(note.outputRange[0],0);assert.equal(note.outputRange.at(-1),0);
 assert(note.outputRange[2]>note.outputRange[1]);assert(note.outputRange.every(v=>v>=0&&v<=.75));
 assert(note.inputRange.every((t,i)=>i===0||t>note.inputRange[i-1]));
 assert.equal(karaokeHeldNote('风',0,500),undefined);
 assert.equal(karaokeHeldNote('风吹过长长的街',0,3000),undefined);
 assert.equal(karaokeHeldNote('♪',0,3000),undefined);
 assert(karaokeHeldNote('love',0,2500),'a held English word is a sung unit too');
});
test('wrapped pieces of one held word share a single envelope rather than restarting the bloom per line',()=>{
 const result=karaokeLayout('光明',[{from:0,to:2,startMs:1000,endMs:5000}],[{text:'光',x:0,y:0,width:28,height:40},{text:'明',x:0,y:40,width:28,height:40}],{});
 assert(result.pieces[0].hold);assert.equal(result.pieces[0].hold,result.pieces[1].hold);
 assert.equal(result.pieces[0].hold.inputRange[0],1000);assert.equal(result.pieces[1].hold.inputRange.at(-1),5000);
});
test('held glow shares the word-fill native clock and clipping, with unchanged glyph layout',()=>{
 const line={id:'held',text:'光',startMs:10000,endMs:12500,words:[{text:'光',startMs:10000,endMs:12500}]};
 const f=fixture({providedLine:line,positionMs:11000});f.withClock(()=>{
  let root=f.render();root.props.onLayout({nativeEvent:{layout:{width:100}}});f.find(root,n=>n.props.testID==='karaoke-text').props.onTextLayout({nativeEvent:{lines:[{text:'光',x:72,y:0,width:28,height:40}]}});
  root=f.render();const node=f.find(root,n=>n.props.piece),word=node.type(node.props),glow=f.find(word,n=>n.props.testID==='lyric-held-note-glow');assert(glow);
  const paint=Object.assign({},...glow.props.style),reveal=word.props.children[0].props.style.transform[0].translateX;
  assert.equal(paint.opacity.value,reveal.value,'fill and bloom must pause and seek together');
  assert.deepEqual(paint.opacity.config.inputRange,node.props.piece.hold.inputRange);
  assert.equal(paint.textShadowRadius,7);assert.deepEqual(paint.textShadowOffset,{width:0,height:0});
  assert.equal(paint.left,-72);assert.equal(paint.width,100);assert.equal(paint.fontSize,28);
  assert.equal(word.props.style.width,28);assert.equal(word.props.style.height,40);
  assert.equal(f.calls.filter(c=>c[0]==='start').length,1,'glow adds no timer or separate animation');
  f.render({playing:false});assert.deepEqual(f.calls.at(-1),['set',11000]);
  f.render({positionMs:10250});assert.deepEqual(f.calls.at(-1),['set',10250]);
 });f.dispose();
});
