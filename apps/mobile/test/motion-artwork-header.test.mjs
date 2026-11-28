import assert from "node:assert/strict";
import test from "node:test";
import { build } from "esbuild";

const compiled = async path => (await build({entryPoints:[path],bundle:true,platform:"node",format:"cjs",external:["*"],write:false})).outputFiles[0].text;
const load = (code, require) => { const module={exports:{}};new Function("require","module","exports",code)(require,module,module.exports);return module.exports; };
const {motionArtworkHeaderHidden:hidden}=load(await compiled("apps/mobile/src/motion-artwork-header.ts"),()=>({}));
const mode={motionEnabled:true,hideHeader:true,motionUri:"file:///song.m4a",page:0,lyricsSelected:false,tabletLandscape:false};

test("compact controls require both switches and an actual motion cover on playback",()=>{
 assert.equal(hidden(mode),true);
 for(const change of [{motionEnabled:false},{hideHeader:false},{motionUri:""},{page:1},{page:2},{lyricsSelected:true}]){
  assert.equal(hidden({...mode,...change}),false,JSON.stringify(change));
 }
 assert.equal(hidden({...mode,lyricsSelected:true,tabletLandscape:true}),true,"tablet landscape keeps its metadata controls beside lyrics");
});

const preferenceCode=await compiled("apps/mobile/src/navigation-preferences.tsx");
function preferences(){
 let slots=[],cursor=0,effects=[],restore;
 const pending=new Promise(resolve=>{restore=resolve;});const writes=[];
 const React={
  createContext:value=>({Provider:"Provider",value}),
  createElement:(type,props,...children)=>({type,props:{...props,children}}),
  useState:initial=>{const i=cursor++;if(!slots[i])slots[i]={value:initial};return [slots[i].value,v=>{slots[i].value=typeof v==="function"?v(slots[i].value):v;}];},
  useRef:value=>{const i=cursor++;return slots[i]??(slots[i]={current:value});},
  useEffect:(effect,deps)=>{const i=cursor++;if(!slots[i]||deps.some((v,n)=>v!==slots[i][n])){slots[i]=deps;effects.push(effect);}},
 };
 const {NavigationPreferencesProvider:Provider}=load(preferenceCode,name=>{
  if(name==="react")return React;
  if(name==="expo-file-system/legacy")return {documentDirectory:"file:///documents/",readAsStringAsync:()=>pending,writeAsStringAsync:async(_path,text)=>{writes.push(JSON.parse(text));}};
  return {CARDBOARD_DEFAULTS:{enabled:false},normalizeCardboardConfig:value=>value};
 });
 const render=()=>{cursor=0;const tree=Provider({children:"player"});const ready=effects;effects=[];ready.forEach(fn=>fn());return tree.props.value;};
 return {render,restore:value=>restore(JSON.stringify(value)),writes};
}
const settle=()=>new Promise(resolve=>setImmediate(resolve));

test("the new option defaults off, restores safely, and keeps its choice when motion is disabled",async()=>{
 const prefs=preferences();assert.equal(prefs.render().hideMotionArtworkHeader,false);
 prefs.restore({motionArtwork:true,hideMotionArtworkHeader:true});await settle();
 let state=prefs.render();assert.equal(state.hideMotionArtworkHeader,true);
 state.setMotionArtwork(false);state=prefs.render();await settle();
 assert.equal(state.motionArtwork,false);assert.equal(state.hideMotionArtworkHeader,true);
 assert.equal(hidden({...mode,motionEnabled:state.motionArtwork,hideHeader:state.hideMotionArtworkHeader}),false);
 state.setMotionArtwork(true);state=prefs.render();await settle();
 assert.equal(prefs.writes.at(-1).hideMotionArtworkHeader,true);
 assert.equal(hidden({...mode,motionEnabled:state.motionArtwork,hideHeader:state.hideMotionArtworkHeader}),true);
});

test("a late saved preference cannot overwrite a user's new header choice",async()=>{
 const prefs=preferences();prefs.render().setHideMotionArtworkHeader(false);
 prefs.restore({motionArtwork:true,hideMotionArtworkHeader:true});await settle();
 const state=prefs.render();assert.equal(state.motionArtwork,true);assert.equal(state.hideMotionArtworkHeader,false);
 assert.equal(prefs.writes.at(-1).hideMotionArtworkHeader,false);
 assert.equal(prefs.writes.at(-1).motionArtwork,true);
});

test("old preference files retain the original header",async()=>{
 const prefs=preferences();prefs.render();prefs.restore({motionArtwork:true});await settle();
 assert.equal(prefs.render().hideMotionArtworkHeader,false);
});

test("hidden landscape actions retain geometry and mini-player refs but reject touches and accessibility",async()=>{
 const React={createElement:(type,props,...children)=>({type,props:{...props,children}})};
 const {SideNavigationDock:Dock}=load(await compiled("apps/mobile/src/SideNavigationDock.tsx"),name=>{
  if(name==="react")return React;
  if(name==="react-native")return {Platform:{OS:"ios"},View:"View",StyleSheet:{absoluteFill:{position:"absolute",top:0,bottom:0,left:0,right:0}}};
  if(name==="react-native-safe-area-context")return {SafeAreaView:"SafeAreaView",useSafeAreaInsets:()=>({left:0,right:0,top:20,bottom:0})};
  if(name==="./SideNavigationRail")return {SIDE_NAVIGATION_WIDTH:64};
  return {leftSideDockInset:()=>0,rightSideDockInset:()=>0};
 });
 const actions={miniPlayerRef:{},head:"head",settings:"settings"};
 const props={side:"right",placement:"top",testID:"actions",children:actions};
 const visible=Dock(props),invisible=Dock({...props,hidden:true});
 const style=node=>{const value=Object.assign({},...node.props.style);delete value.opacity;return value;};
 assert.deepEqual(style(visible),style(invisible));
 assert.equal(invisible.props.style[1].opacity,0);
 assert.equal(invisible.props.pointerEvents,"none");
 assert.equal(invisible.props.accessibilityElementsHidden,true);
 assert.equal(invisible.props.importantForAccessibility,"no-hide-descendants");
 assert.equal(invisible.props.children[0].props.children[0],actions);
 assert.equal(visible.props.pointerEvents,"box-none");
});
