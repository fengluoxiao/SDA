import assert from "node:assert/strict";
import test from "node:test";
import {readFileSync} from "node:fs";
import {build} from "esbuild";

const code=(await build({entryPoints:["apps/mobile/src/IOSNativeSettings.tsx"],bundle:true,platform:"node",format:"cjs",external:["*"],write:false})).outputFiles[0].text;
const React={createElement:(type,props,...children)=>({type,props:{...props,children}}),Fragment:"Fragment"};
function navigation(nativeStack){
 const previous=globalThis.expo,dev=globalThis.__DEV__,module={exports:{}};
 globalThis.expo={getViewConfig:()=>({})};globalThis.__DEV__=false;
 try{
  new Function("require","module","exports",code)(name=>{
   if(name==="react")return React;
   if(name==="react-native")return {Platform:{OS:"ios",Version:27},PlatformColor:x=>x,StyleSheet:{create:x=>x},Modal:"Modal"};
   if(name==="react-native-screens")return {ScreenStack:"Stack",ScreenStackItem:"Screen"};
   if(name==="./IOSSystemTabs")return {swiftUI:Object.fromEntries(["Host","Form","Section","Toggle","Picker","Text","Button","HStack","VStack"].map(name=>[name,name]))};
   if(name==="./IOSNativePage")return {hasNativePageHeader:nativeStack,IOSNativePage:"Page"};
   if(name==="./ios-platform-version")return {supportsIOSGlass:()=>true};
   return {};
  },module,module.exports);
 }finally{globalThis.expo=previous;globalThis.__DEV__=dev;}
 return module.exports.IOSSettingsNavigation;
}

test("closing settings waits for the native home appearance before resuming motion",()=>{
 const Nav=navigation(true);let settings=false,visible=true;
 const base={children:"Player",title:"Playing",player:{},accent:"#fff",theme:"dark",onSettingsChange:value=>{settings=value;},onPlayerVisibilityChange:value=>{visible=value;}};
 const expression=readFileSync("apps/mobile/src/IOSPlayer.tsx","utf8").match(/const motionSurfaceVisible = ([^;]+);/)[1];
 const evaluate=new Function("immersiveHome","playerNavigationVisible","p","settings","sharedMini","lyricsPageSelected","landscapePlayer",`return ${expression};`);
 const playing=()=>evaluate(true,visible,{ended:false,busy:false,preparingAudio:false},settings,{active:false},false,false);
 for(let visit=0;visit<4;visit++){
  let tree=Nav({...base,settings});tree.props.children[0].props.onAppear();assert.equal(playing(),true);
  settings=true;tree=Nav({...base,settings});tree.props.children[0].props.onWillDisappear();assert.equal(playing(),false);
  // Back updates React before the native pop animation is finished.
  tree.props.children[1].props.onDismissed();assert.equal(settings,false);
  tree=Nav({...base,settings});assert.equal(playing(),false,"pop start must not resume a still-hidden native layer");
  tree.props.children[0].props.onAppear();assert.equal(playing(),true,"completed pop must resume without changing any preference");
 }
});

test("compatibility settings modal also reports its actual visibility",()=>{
 const Nav=navigation(false),changes=[];
 const tree=Nav({children:"Player",settings:true,onSettingsChange:()=>{},onPlayerVisibilityChange:value=>changes.push(value),title:"Playing",player:{},accent:"#fff",theme:"dark"});
 const modal=tree.props.children[1];modal.props.onShow();modal.props.onDismiss();assert.deepEqual(changes,[false,true]);
});
