import assert from 'node:assert/strict';
import {build} from 'esbuild';
import {readFileSync} from 'node:fs';
const result=await build({entryPoints:['apps/mobile/src/navigation-preferences.tsx'],bundle:true,platform:'node',format:'cjs',write:false,external:['react','expo-file-system/legacy']});
function harness(initialRead) {
  const cells=[];let cursor=0;const effects=[];const writes=[];
  const react={
    createContext:value=>({Provider:'Provider',value}),useContext:context=>context.value,
    createElement:(type,props,...children)=>({type,props:{...props,children}}),
    useState:initial=>{const i=cursor++;if(!(i in cells))cells[i]=initial;return[cells[i],value=>{cells[i]=typeof value==='function'?value(cells[i]):value;}];},
    useRef:initial=>{const i=cursor++;return cells[i]??(cells[i]={current:initial});},
    useEffect:effect=>{const i=cursor++;if(!(i in cells)){cells[i]=true;effects.push(effect);}},
  };
  const fs={documentDirectory:'file:///preferences/',readAsStringAsync:()=>initialRead,
    writeAsStringAsync:async(file,text)=>{assert.equal(file,'file:///preferences/sda-navigation.json');writes.push(JSON.parse(text));}};
  const module={exports:{}};
  new Function('require','module','exports',result.outputFiles[0].text)(name=>name==='react'?react:fs,module,module.exports);
  const render=()=>{cursor=0;const tree=module.exports.NavigationPreferencesProvider({children:'home'});for(const effect of effects.splice(0))effect();return tree.props.value;};
  return {render,writes};
}
const tick=()=>new Promise(resolve=>setImmediate(resolve));
let release;
const delayed=harness(new Promise(resolve=>{release=resolve;}));
assert.equal(delayed.render().landscapeSideTabs,false,'side navigation must default off');
delayed.render().setLandscapeSideTabs(true);
release(JSON.stringify({landscapeSideTabs:false}));await tick();
assert.equal(delayed.render().landscapeSideTabs,true,'late disk load must not override the user');
delayed.render().setLandscapeSideTabs(false);delayed.render().setLandscapeSideTabs(true);await tick();
assert.deepEqual(delayed.writes.map(v=>v.landscapeSideTabs),[true,false,true]);
const restored=harness(Promise.resolve(JSON.stringify(delayed.writes.at(-1))));restored.render();await tick();assert.equal(restored.render().landscapeSideTabs,true);
for(const invalid of ['bad JSON',JSON.stringify({landscapeSideTabs:'true'})]) {const h=harness(Promise.resolve(invalid));h.render();await tick();assert.equal(h.render().landscapeSideTabs,false);}
const ui=readFileSync('apps/mobile/src/IOSPlayer.tsx','utf8');
const expression=ui.match(/const sideNavigationActive = ([^;]+);/)[1];
const enabled=new Function('Platform','navigationPreferences','landscapePlayer','hasAndroidChrome','return '+expression);
for(const os of ['ios','android','web'])for(const nativeChrome of [false,true])for(const pref of [false,true])for(const landscape of [false,true])assert.equal(enabled({OS:os},{landscapeSideTabs:pref},landscape,nativeChrome),(os==='ios'||os==='android'&&nativeChrome)&&pref&&landscape);
console.log('Side navigation preference: default off, save order, reload, late-read protection and portrait/platform gates passed.');

const sides=harness(Promise.resolve(JSON.stringify({landscapeSideTabs:true})));
assert.equal(sides.render().sideTabPosition,'right');await tick();
sides.render().setSideTabPosition('left');sides.render().setLandscapeSideTabs(false);await tick();
assert.deepEqual(sides.writes.at(-1),{landscapeSideTabs:false,tabletPortraitLayout:false,sideTabPosition:'left'});
const sideReload=harness(Promise.resolve(JSON.stringify(sides.writes.at(-1))));sideReload.render();await tick();
assert.equal(sideReload.render().sideTabPosition,'left');
let releasePosition;const sideRace=harness(new Promise(resolve=>{releasePosition=resolve;}));
sideRace.render().setSideTabPosition('right');releasePosition(JSON.stringify({landscapeSideTabs:true,sideTabPosition:'left'}));await tick();
assert.equal(sideRace.render().sideTabPosition,'right');assert.equal(sideRace.render().landscapeSideTabs,true);
assert.deepEqual(sideRace.writes.at(-1),{landscapeSideTabs:true,tabletPortraitLayout:false,sideTabPosition:'right'});
const invalidSide=harness(Promise.resolve(JSON.stringify({sideTabPosition:'top'})));invalidSide.render();await tick();assert.equal(invalidSide.render().sideTabPosition,'right');
console.log('Side position: default right, migration, persistence, independent late-read protection passed.');

const layout=harness(Promise.resolve(JSON.stringify({landscapeSideTabs:true,sideTabPosition:'left'})));
assert.equal(layout.render().tabletPortraitLayout,false,'existing installations retain their original phone layout');await tick();
layout.render().setTabletPortraitLayout(true);await tick();assert.equal(layout.render().tabletPortraitLayout,true);
assert.equal(layout.writes.at(-1).sideTabPosition,'left');assert.equal(layout.writes.at(-1).landscapeSideTabs,true);
const layoutReload=harness(Promise.resolve(JSON.stringify(layout.writes.at(-1))));layoutReload.render();await tick();assert.equal(layoutReload.render().tabletPortraitLayout,true);
layoutReload.render().setTabletPortraitLayout(false);await tick();assert.equal(layoutReload.render().tabletPortraitLayout,false);
let releaseLayout;const layoutRace=harness(new Promise(resolve=>{releaseLayout=resolve;}));
layoutRace.render().setTabletPortraitLayout(true);releaseLayout(JSON.stringify({tabletPortraitLayout:false,landscapeSideTabs:true,sideTabPosition:'left'}));await tick();
assert.equal(layoutRace.render().tabletPortraitLayout,true);assert.equal(layoutRace.writes.at(-1).sideTabPosition,'left');
let releaseOther;const otherRace=harness(new Promise(resolve=>{releaseOther=resolve;}));
otherRace.render().setSideTabPosition('left');releaseOther(JSON.stringify({tabletPortraitLayout:true,sideTabPosition:'right'}));await tick();assert.equal(otherRace.writes.at(-1).tabletPortraitLayout,true);
const invalidLayout=harness(Promise.resolve(JSON.stringify({tabletPortraitLayout:'true'})));invalidLayout.render();await tick();assert.equal(invalidLayout.render().tabletPortraitLayout,false);
for(const file of ['apps/mobile/src/AndroidSettings.tsx','apps/mobile/src/IOSNativeSettings.tsx']) {
 const settings=readFileSync(file,'utf8');assert.match(settings,/平板风格界面/);assert.match(settings,/setTabletPortraitLayout/);
}
console.log('Phone portrait layout preference: default-off migration, enable/disable, persistence, independent load races and both native settings switches passed.');
