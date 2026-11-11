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
const cardboardDefaults={enabled:false,eyeDistanceMm:64,lensDistanceMm:64,screenWidthMm:140,fov:85,k1:.22,k2:.24};
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
assert.deepEqual(sides.writes.at(-1),{cardboard:cardboardDefaults,landscapeSideTabs:false,tabletPortraitLayout:false,immersiveSpace:false,showChannelSpeakers:false,space120Fps:false,sideTabPosition:'left'});
const sideReload=harness(Promise.resolve(JSON.stringify(sides.writes.at(-1))));sideReload.render();await tick();
assert.equal(sideReload.render().sideTabPosition,'left');
let releasePosition;const sideRace=harness(new Promise(resolve=>{releasePosition=resolve;}));
sideRace.render().setSideTabPosition('right');releasePosition(JSON.stringify({landscapeSideTabs:true,sideTabPosition:'left'}));await tick();
assert.equal(sideRace.render().sideTabPosition,'right');assert.equal(sideRace.render().landscapeSideTabs,true);
assert.deepEqual(sideRace.writes.at(-1),{cardboard:cardboardDefaults,landscapeSideTabs:true,tabletPortraitLayout:false,immersiveSpace:false,showChannelSpeakers:false,space120Fps:false,sideTabPosition:'right'});
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

const stageDefaults=harness(Promise.resolve(JSON.stringify({landscapeSideTabs:true,tabletPortraitLayout:true,sideTabPosition:'left'})));
assert.equal(stageDefaults.render().immersiveSpace,false);await tick();assert.equal(stageDefaults.render().immersiveSpace,false);
stageDefaults.render().setImmersiveSpace(true);await tick();
assert.deepEqual(stageDefaults.writes.at(-1),{cardboard:cardboardDefaults,landscapeSideTabs:true,tabletPortraitLayout:true,immersiveSpace:true,showChannelSpeakers:false,space120Fps:false,sideTabPosition:'left'});
const stageReload=harness(Promise.resolve(JSON.stringify(stageDefaults.writes.at(-1))));stageReload.render();await tick();assert.equal(stageReload.render().immersiveSpace,true);
let releaseStage;const stageRace=harness(new Promise(resolve=>{releaseStage=resolve;}));
stageRace.render().setImmersiveSpace(true);releaseStage(JSON.stringify({immersiveSpace:false,showChannelSpeakers:false,space120Fps:false,sideTabPosition:'left',tabletPortraitLayout:true}));await tick();
assert.equal(stageRace.render().immersiveSpace,true);assert.equal(stageRace.render().sideTabPosition,'left');assert.equal(stageRace.render().tabletPortraitLayout,true);
assert.equal(stageRace.writes.at(-1).immersiveSpace,true);assert.equal(stageRace.writes.at(-1).sideTabPosition,'left');
for(const invalid of ['true',1,null,{}]){const h=harness(Promise.resolve(JSON.stringify({immersiveSpace:invalid})));h.render();await tick();assert.equal(h.render().immersiveSpace,false);}
stageRace.render().setImmersiveSpace(false);await tick();assert.equal(stageRace.writes.at(-1).immersiveSpace,false);
console.log('Immersive Space preference: migration defaults off, persistence, independent late-read protection and restoring classic appearance passed.');

const refreshDefaults=harness(Promise.resolve('{}'));refreshDefaults.render();await tick();assert.equal(refreshDefaults.render().space120Fps,false);
refreshDefaults.render().setSpace120Fps(true);await tick();
const refreshReload=harness(Promise.resolve(JSON.stringify(refreshDefaults.writes.at(-1))));refreshReload.render();await tick();assert.equal(refreshReload.render().space120Fps,true);
let releaseRefresh;const refreshRace=harness(new Promise(resolve=>{releaseRefresh=resolve;}));
refreshRace.render().setSpace120Fps(true);releaseRefresh(JSON.stringify({space120Fps:false,sideTabPosition:'left'}));await tick();assert.equal(refreshRace.render().space120Fps,true);assert.equal(refreshRace.render().sideTabPosition,'left');
for(const invalid of ['true',120,null,{}]){const h=harness(Promise.resolve(JSON.stringify({space120Fps:invalid})));h.render();await tick();assert.equal(h.render().space120Fps,false);}
console.log('Scene 120fps preference: default60, save/reload, invalid data and late-read protection passed.');

const speakerDefaults=harness(Promise.resolve('{}'));speakerDefaults.render();await tick();assert.equal(speakerDefaults.render().showChannelSpeakers,false);
speakerDefaults.render().setShowChannelSpeakers(true);await tick();
const speakerReload=harness(Promise.resolve(JSON.stringify(speakerDefaults.writes.at(-1))));speakerReload.render();await tick();assert.equal(speakerReload.render().showChannelSpeakers,true);
speakerReload.render().setShowChannelSpeakers(false);await tick();assert.equal(speakerReload.writes.at(-1).showChannelSpeakers,false);
let releaseSpeakers;const speakerRace=harness(new Promise(resolve=>{releaseSpeakers=resolve;}));speakerRace.render().setShowChannelSpeakers(true);releaseSpeakers(JSON.stringify({showChannelSpeakers:false,immersiveSpace:true}));await tick();assert.equal(speakerRace.render().showChannelSpeakers,true);assert.equal(speakerRace.render().immersiveSpace,true);
for(const invalid of ['true',1,null,{}]){const h=harness(Promise.resolve(JSON.stringify({showChannelSpeakers:invalid})));h.render();await tick();assert.equal(h.render().showChannelSpeakers,false);}
for(const file of ['apps/mobile/src/IOSNativeSettings.tsx','apps/mobile/src/AndroidSettings.tsx','apps/mobile/src/IOSPlayer.tsx']){const text=readFileSync(file,'utf8');assert(text.includes('炫彩风格'));assert(text.includes('显示声道音箱'));assert(!text.includes('Folia 风格'));}
console.log('Channel speaker visibility: default hidden, persisted enable/disable, migration and late-read protection; shared colorful style/settings names passed.');

const vrDefaults=harness(Promise.resolve('{}'));vrDefaults.render();await tick();assert.deepEqual(vrDefaults.render().cardboard,cardboardDefaults);
vrDefaults.render().setCardboard({...cardboardDefaults,enabled:true,eyeDistanceMm:68,k1:.3});await tick();
const vrReload=harness(Promise.resolve(JSON.stringify(vrDefaults.writes.at(-1))));vrReload.render();await tick();assert.equal(vrReload.render().cardboard.enabled,true);assert.equal(vrReload.render().cardboard.eyeDistanceMm,68);
let releaseVr;const vrRace=harness(new Promise(resolve=>releaseVr=resolve));vrRace.render().setCardboard({...cardboardDefaults,enabled:true});releaseVr(JSON.stringify({cardboard:{enabled:false,eyeDistanceMm:72},immersiveSpace:true}));await tick();assert.equal(vrRace.render().cardboard.enabled,true);assert.equal(vrRace.render().cardboard.eyeDistanceMm,64);assert.equal(vrRace.render().immersiveSpace,true);
const vrInvalid=harness(Promise.resolve(JSON.stringify({cardboard:{enabled:'true',eyeDistanceMm:1000,k1:-3,k2:'bad'}})));vrInvalid.render();await tick();assert.equal(vrInvalid.render().cardboard.enabled,false);assert.equal(vrInvalid.render().cardboard.eyeDistanceMm,76);assert.equal(vrInvalid.render().cardboard.k1,0);assert.equal(vrInvalid.render().cardboard.k2,.24);
for(const file of ['IOSNativeSettings.tsx','AndroidSettings.tsx','IOSPlayer.tsx'])assert.match(readFileSync('apps/mobile/src/'+file,'utf8'),/纸盒 VR/);
console.log('Cardboard VR: default off, validated calibration, persistence, independent late-read protection and all mobile settings entries passed.');
