import assert from 'node:assert/strict';
import {build} from 'esbuild';
const compiled=await build({entryPoints:['apps/mobile/src/native-tab-selection.ts'],bundle:true,platform:'node',format:'esm',write:false});
const {resolveNativeTabSelection}=await import('data:text/javascript;base64,'+Buffer.from(compiled.outputFiles[0].text).toString('base64'));
for(const selected of [0,1,2]) {
 const other=String((selected+1)%3);
 for(const actionOrigin of ['implicit','programmatic-js','programmatic-native',undefined]) {
  assert.deepEqual(resolveNativeTabSelection({selectedScreenKey:other,provenance:5,actionOrigin},4,selected,true),{provenance:5,index:null},'native synchronization must not become application navigation');
 }
 assert.deepEqual(resolveNativeTabSelection({selectedScreenKey:other,provenance:6,actionOrigin:'user'},5,selected,true),{provenance:6,index:Number(other)});
 assert.deepEqual(resolveNativeTabSelection({selectedScreenKey:other,provenance:6,actionOrigin:'user'},5,selected,false),{provenance:6,index:null},'inactive UI must not navigate from queued taps');
 assert.equal(resolveNativeTabSelection({selectedScreenKey:other,provenance:4,actionOrigin:'user'},5,selected,true),null);
 for(const selectedScreenKey of ['', '-1','3','1.5','1e0'])assert.equal(resolveNativeTabSelection({selectedScreenKey,provenance:6,actionOrigin:'user'},5,selected,true).index,null);
}
// Rotating mounts the secondary bar with a default/implicit selection. The app
// must retain the current scene while acknowledging each controller's sequence.
let selected=2,provenance=0;
for(const event of [
 {selectedScreenKey:'0',provenance:1,actionOrigin:'implicit'},
 {selectedScreenKey:'2',provenance:2,actionOrigin:'programmatic-js'},
 {selectedScreenKey:'0',provenance:1,actionOrigin:'user'},
]){const result=resolveNativeTabSelection(event,provenance,selected,true);if(result){provenance=result.provenance;if(result.index!==null)selected=result.index;}}
assert.equal(selected,2);assert.equal(provenance,2);
console.log('Native tab selection: user-only commits, inactive/stale rejection and mount/rotation acknowledgement without page changes passed.');

// A native page transition must not resize the common host by removing a
// sibling header or changing the library's margins when another tab is selected.
const {readFileSync}=await import('node:fs');
const player=readFileSync('apps/mobile/src/IOSPlayer.tsx','utf8');
assert.match(player,/const nativeLibraryHeaderActive = hasNativePageHeader && hasUIKitIOSTabs;/);
assert.match(player,/edges=\{hasUIKitIOSTabs \? \[\]/);
assert.match(player,/!hasUIKitIOSTabs && !hasAndroidChrome && pageHeader\(page\)/);
const tabsBundle=await build({entryPoints:['apps/mobile/src/IOSUIKitTabs.tsx'],bundle:true,platform:'node',format:'cjs',write:false,define:{__DEV__:'false'},external:['react','react-native','react-native-screens','react-native-safe-area-context','./IOSNativePage']});
const mod={exports:{}};
const element=(type,props,...children)=>({type,props:{...props,children}});
new Function('require','module','exports',tabsBundle.outputFiles[0].text)(name=>{
 if(name==='react')return {createElement:element,useRef:value=>({current:value}),useState:value=>[value,()=>{}],isValidElement:value=>!!value?.type,cloneElement:(node,props)=>({...node,props:{...node.props,...props}})};
 if(name==='react-native')return {AppState:{currentState:'active'},Platform:{OS:'ios',Version:26},UIManager:{hasViewManagerConfig:()=>true},View:'View'};
 if(name==='react-native-safe-area-context')return {SafeAreaProvider:'SafeAreaProvider',SafeAreaView:'SafeAreaView',useSafeAreaInsets:()=>({top:0,left:59,right:59,bottom:21})};
 if(name==='react-native-screens')return {Tabs:{Host:'TabsHost',Screen:'TabsScreen'}};
 if(name==='./IOSNativePage')return {hasNativePageHeader:true,IOSNativePage:'NativeHeaderPage'};
 throw Error(name);
},mod,mod.exports);
for(const sideBar of [undefined,true]) for(const sideBarPosition of ['left','right']) {
 let baseline;
 for(const selected of [0,1,2,1,0]) {
  const tree=mod.exports.IOSUIKitTabs({selected,onChange:()=>{},pages:[element('Playback'),element('ScrollView'),element('Scene')],pageHeaders:['play-header',null,'space-header'],accent:'#7799CC',theme:'light',backgroundColor:'transparent',pageBackgroundColors:['transparent','#f2f2f7','#f2f2f7'],sideBar,sideBarPosition});
  const host=tree.props.children[0],screens=host.props.children[0];
  const geometry=JSON.stringify({frame:tree.props.style,screens});
  if(baseline)assert.equal(geometry,baseline,'switching tabs preserves each screen/header/safe-area tree and host frame');
  baseline=geometry;
  const playbackSafe=screens[0].props.children[0].props.children[0];
  assert.deepEqual(playbackSafe.props.edges,['top','left','right']);
  assert.equal(playbackSafe.props.children[0].props.children[0],'play-header');
  assert.equal(screens[1].props.children[0].type,'NativeHeaderPage','library keeps its native blurred navigation header');
 }
}
console.log('Native tab transitions: fixed host bounds, persistent per-page headers/insets and library geometry across repeated switches.');
