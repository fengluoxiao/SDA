const assert = require('node:assert/strict');
const {test} = require('node:test');
const fs = require('node:fs');
const ts = require('typescript');
const code = ts.transpileModule(fs.readFileSync(require('node:path').join(__dirname, '../src/use-lyrics-swipe.ts'), 'utf8'), {compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
function fixture(overrides={}) {
 const cells=[],effects=[],animations=[],completions=[];let cursor=0,dirty=false,options={width:390,enabled:true,visible:true,...overrides};
 const changed=(a,b)=>!a||a.some((v,i)=>v!==b[i]);
 const hooks={
  useRef(value){const i=cursor++;return cells[i]??={current:value};},
  useState(value){const i=cursor++;cells[i]??={value};return[cells[i].value,v=>{if(!Object.is(cells[i].value,v)){cells[i].value=v;dirty=true;}}];},
  useMemo(fn,deps){const i=cursor++;if(!cells[i]||changed(cells[i].deps,deps))cells[i]={value:fn(),deps};return cells[i].value;},
  useCallback(fn,deps){return hooks.useMemo(()=>fn,deps);},
  useEffect(fn,deps){const i=cursor++;if(!cells[i]||changed(cells[i].deps,deps)){const old=cells[i];cells[i]={deps};effects.push(()=>{old?.cleanup?.();cells[i].cleanup=fn();});}},
  useLayoutEffect(fn,deps){hooks.useEffect(fn,deps);}
 };
 const native={Animated:{Value:class{constructor(v){this.value=v;this.stops=0;}stopAnimation(){this.stops++;}setValue(v){this.value=v;}interpolate(config){const source=this;return{get value(){const p=Math.max(0,Math.min(1,source.value));return config.outputRange[0]+p*(config.outputRange[1]-config.outputRange[0]);},get stops(){return source.stops;}};}},spring(value,settings){return{start(callback){value.value=settings.toValue;animations.push(settings.toValue);completions.push(callback);}};}},PanResponder:{create:handlers=>({panHandlers:handlers})}};
 const mod={exports:{}};new Function('require','module','exports',code)(name=>name==='react'?hooks:native,mod,mod.exports);
 const render=(update={})=>{options={...options,...update};for(let round=0;round<10;round++){dirty=false;cursor=0;const api=mod.exports.useLyricsSwipe(options);while(effects.length)effects.shift()();if(!dirty)return api;}throw Error("layout effects did not settle");};
 const g=(dx,dy=0,vx=0,touches=1)=>({dx,dy,vx,numberActiveTouches:touches});
 const swipe=(api,gesture)=>{const h=api.panHandlers;assert.equal(h.onMoveShouldSetPanResponderCapture({},gesture),true);h.onPanResponderGrant();h.onPanResponderMove({},gesture);h.onPanResponderRelease({},gesture);};
 return{render,g,swipe,animations,completions,beforeLayout(update={}){options={...options,...update};cursor=0;return mod.exports.useLyricsSwipe(options);}};
}
test('left swipe of 35 points opens lyrics and right swipe of 35 points returns on wide phones',()=>{
 const f=fixture({width:430});let api=f.render();assert.equal(api.translation.value,0);
 f.swipe(api,f.g(-35));api=f.render();assert.equal(api.page,1);assert.equal(api.translation.value,-430);
 f.swipe(api,f.g(35));api=f.render();assert.equal(api.page,0);assert.equal(api.translation.value,0);assert.deepEqual(f.animations,[1,0]);
});
test('a brief left flick opens lyrics; taps and tiny slow drags stay on playback',()=>{
 const f=fixture();let api=f.render();assert.equal(api.panHandlers.onStartShouldSetPanResponder(),false);assert.equal(api.panHandlers.onMoveShouldSetPanResponderCapture({},f.g(-8)),false);
 f.swipe(api,f.g(-20,0,-.05));api=f.render();assert.equal(api.page,0);
 f.swipe(api,f.g(-20,0,-.5));api=f.render();assert.equal(api.page,1);
});
test('vertical lyrics scrolling, diagonals, multiple fingers and wrong direction are excluded',()=>{
 const f=fixture(),api=f.render(),claim=g=>api.panHandlers.onMoveShouldSetPanResponderCapture({},g);
 for(const gesture of [f.g(-15,60),f.g(-30,30),f.g(-50,0,0,2),f.g(50)])assert.equal(claim(gesture),false);
});
test('protected progress and volume gestures cannot be taken by page navigation',()=>{
 const f=fixture(),api=f.render();api.setBlocked(true);assert.equal(api.panHandlers.onMoveShouldSetPanResponderCapture({},f.g(-100)),false);assert.equal(api.panHandlers.onShouldBlockNativeResponder(),false);
 api.setBlocked(false);assert.equal(api.panHandlers.onMoveShouldSetPanResponderCapture({},f.g(-35)),true);
});
test('cancelled drag returns to its page; switching tabs retains the lyric subpage',()=>{
 const f=fixture();let api=f.render();api.panHandlers.onPanResponderGrant();api.panHandlers.onPanResponderMove({},f.g(-150));assert.equal(api.translation.value,-150);api.panHandlers.onPanResponderTerminate();assert.equal(api.translation.value,0);
 f.swipe(api,f.g(-35));api=f.render();assert.equal(api.page,1);f.render({visible:false});api=f.render({visible:true});assert.equal(api.page,1);assert.equal(api.translation.value,-390);
});
test('viewport changes preserve the selected page and back returns to playback',()=>{
 const f=fixture();let api=f.render();f.swipe(api,f.g(-35));api=f.render({width:600});assert.equal(api.page,1);assert.equal(api.translation.value,-600);api.back();api=f.render();assert.equal(api.translation.value,0);assert.equal(api.page,0);
});

test('both playback subpages survive repeated tab switches and orientation changes',()=>{
 for(const page of [0,1]){
  const f=fixture({initialPage:page});let api=f.render();assert.equal(api.page,page);
  for(const width of [390,430,390]){
   api=f.render({visible:false,width});assert.equal(api.page,page);assert.equal(api.panHandlers.onMoveShouldSetPanResponderCapture({},f.g(page===0?-35:35)),false);
   api=f.render({visible:true,enabled:false});assert.equal(api.page,page);
   api=f.render({enabled:true});assert.equal(api.page,page);assert.equal(api.translation.value,page===0?0:-width);
  }
 }
});
test('a recreated native page restores the subpage retained by its parent',()=>{
 const first=fixture();let api=first.render();first.swipe(api,first.g(-35));const saved=first.render().page;
 const recreated=fixture({initialPage:saved}),restored=recreated.render();assert.equal(restored.page,1);assert.equal(restored.translation.value,-390);
});

test('portrait and landscape share selection without inactive views resetting the active one',()=>{
 let selection=0;const changed=[];
 const portrait=fixture({selectedPage:0,onPageChange:page=>{selection=page;changed.push(page);}});
 const landscape=fixture({enabled:false,selectedPage:0,onPageChange:page=>{selection=page;changed.push(page);}});
 let p=portrait.render(),l=landscape.render();portrait.swipe(p,portrait.g(-35));assert.equal(selection,1);
 p=portrait.render({enabled:false,selectedPage:selection});l=landscape.render({enabled:true,selectedPage:selection});assert.equal(p.page,1);assert.equal(l.page,1);assert.deepEqual(changed,[1]);
 landscape.swipe(l,landscape.g(35));assert.equal(selection,0);
 l=landscape.render({selectedPage:selection});p=portrait.render({enabled:true,selectedPage:selection});assert.equal(l.page,0);assert.equal(p.page,0);assert.deepEqual(changed,[1,0]);
});
test('the controlled selection echo does not truncate the active page spring',()=>{
 let selection=0;const f=fixture({selectedPage:0,onPageChange:page=>{selection=page;}});let api=f.render();f.swipe(api,f.g(-35));const stops=api.translation.stops;
 api=f.render({selectedPage:selection});assert.equal(api.page,1);assert.equal(api.translation.stops,stops,'parent confirmation must not snap an in-progress slide');
});
test('late gesture release after a tab/orientation switch cannot change shared selection',()=>{
 const changes=[],f=fixture({selectedPage:0,onPageChange:page=>changes.push(page)});let api=f.render();api.panHandlers.onPanResponderGrant();
 api=f.render({enabled:false});api.panHandlers.onPanResponderRelease({},f.g(-100));api.panHandlers.onPanResponderTerminate();assert.deepEqual(changes,[]);assert.equal(f.render().page,0);
});
test('rotation cannot carry an old pixel translation into the new column width',()=>{
 const f=fixture({selectedPage:1,width:430});let api=f.render();assert.equal(api.translation.value,-430);
 api=f.beforeLayout({width:246});assert.equal(api.position.value,1);assert.equal(api.translation.value,-246,'new page width and translation agree before layout effects run');
 api=f.beforeLayout({width:430});assert.equal(api.translation.value,-430);
});

test('only the current completed swipe refreshes playback motion coordinates',()=>{
 const settled=[],f=fixture({onPageSettled:page=>settled.push(page)});let api=f.render();f.swipe(api,f.g(-35));assert.deepEqual(settled,[]);
 f.completions.at(-1)({finished:true});assert.deepEqual(settled,[1]);
 api=f.render();f.swipe(api,f.g(35));const old=f.completions.at(-1);f.swipe(api,f.g(-35));old({finished:true});assert.deepEqual(settled,[1]);
 f.completions.at(-1)({finished:false});assert.deepEqual(settled,[1]);
});
