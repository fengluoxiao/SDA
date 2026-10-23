import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {build} from 'esbuild';
const realRequire=createRequire(import.meta.url);const React=realRequire('react');
const bundle=async file=>(await build({entryPoints:[file],bundle:true,platform:'node',format:'cjs',write:false,external:['*']})).outputFiles[0].text;
function load(code,require){const m={exports:{}};new Function('require','module','exports',code)(require,m,m.exports);return m.exports;}
const model=load(await bundle('apps/mobile/src/spatial-head-indicator.ts'),realRequire);
for(const yaw of [-370,-30,0,30,370,NaN,Infinity]) {
 const off=model.spatialHeadIndicator(false,true,yaw,'accent','neutral');assert.equal(off.color,'neutral');assert.equal(off.poseDegrees,0);assert.equal(off.tracking,false);
 const fixed=model.spatialHeadIndicator(true,false,yaw,'accent','neutral');assert.equal(fixed.color,'accent');assert.equal(fixed.poseDegrees,0);
 const tracking=model.spatialHeadIndicator(true,true,yaw,'accent','neutral');assert.equal(tracking.color,'accent');assert(Number.isFinite(tracking.poseDegrees)&&Math.abs(tracking.poseDegrees)<=45);
}
assert.notEqual(model.spatialHeadIndicator(true,true,20,'a','n').poseDegrees,model.spatialHeadIndicator(true,true,-20,'a','n').poseDegrees);
assert.equal(model.spatialHeadIndicator(true,true,350,'a','n').poseDegrees,-10);
const code=await bundle('apps/mobile/src/IOSNativeSettings.tsx');
for(const menu of [true,false]) {
 globalThis.expo={getViewConfig:(_module,name)=>name==='MenuView'?menu:true};
 const ui=Object.fromEntries(['Host','Menu','Picker','Text','Image','Button','Form','Section','HStack','VStack'].map(x=>[x,x]));
 const modifiers=new Proxy({}, {get:(_obj,name)=>(...args)=>({name,args})});
 const mod=load(code,name=>name==='react'?React:name==='react-native'?{Platform:{OS:'ios',Version:26},StyleSheet:{create:x=>x},Alert:{alert(){}},PlatformColor:x=>x}:name==='./IOSSystemTabs'?{swiftUI:ui}:name==='./IOSNativePage'?{hasNativePageHeader:false}:name==='./spatial-head-indicator'?model:name==='@expo/ui/swift-ui/modifiers'?modifiers:{});
 for(const [spatial,tracking,yaw] of [[false,true,30],[true,false,30],[true,true,30],[true,true,-30]]) {
  const changes=[];const element=mod.IOSHeadTrackingButton({enabled:tracking,spatialEnabled:spatial,headYaw:yaw,accent:'accent',inactiveColor:'neutral',disabled:false,onSelect:v=>changes.push(v),onDisable:()=>changes.push('off')});
  const control=element.props.children;const image=menu?control.props.label:control.props.children;
  const gearHost=mod.IOSSettingsButton({onPress(){}});const gear=gearHost.props.children.props.children;
  assert.equal(gear.props.systemName,'gearshape');assert.equal(gear.props.size,image.props.size);assert.equal(gear.props.size,22);
  assert.equal(gearHost.props.style.width,element.props.style.width);assert.equal(gearHost.props.style.height,element.props.style.height);
  assert.equal(image.props.color,undefined,'use the compatible foregroundColor modifier, not Image color -> foregroundStyle');
  assert.equal(image.props.modifiers.find(m=>m.name==='foregroundColor').args[0],spatial?'accent':'neutral');
  assert.equal(image.props.modifiers[0].name,'rotation3DEffect');assert.equal(image.props.modifiers[0].args[0].angle,spatial&&tracking?yaw:0);
  if(menu){const picker=control.props.children;assert.equal(picker.props.selection,!spatial?'off':tracking?'tracking':'fixed');picker.props.onSelectionChange('fixed');picker.props.onSelectionChange('tracking');picker.props.onSelectionChange('off');assert.deepEqual(changes,[false,true,'off']);}
 }
}
// Native Image is optional; both controls retain the same 22pt fallback font.
{
 globalThis.expo={getViewConfig:(_module,name)=>name!=='ImageView'};
 const ui=Object.fromEntries(['Host','Menu','Picker','Text','Image','Button','Form','Section','HStack','VStack'].map(x=>[x,x]));
 const modifiers=new Proxy({}, {get:(_obj,name)=>(...args)=>({name,args})});
 const mod=load(code,name=>name==='react'?React:name==='react-native'?{Platform:{OS:'ios',Version:26},StyleSheet:{create:x=>x},Alert:{alert(){}},PlatformColor:x=>x}:name==='./IOSSystemTabs'?{swiftUI:ui}:name==='./IOSNativePage'?{hasNativePageHeader:false}:name==='./spatial-head-indicator'?model:name==='@expo/ui/swift-ui/modifiers'?modifiers:{});
 const gear=mod.IOSSettingsButton({onPress(){}}).props.children;
 const head=mod.IOSHeadTrackingButton({enabled:false,spatialEnabled:true,accent:'a',disabled:false,onSelect(){}}).props.children;
 for(const control of [gear,head])assert.equal(control.props.modifiers.find(m=>m.name==='font').args[0].size,22);
 assert.equal(gear.props.systemImage,'gearshape');assert.equal(head.props.systemImage,'person.crop.circle');
}
globalThis.expo={modules:{SdaAndroidChrome:true}};
// Native view availability is evaluated while importing the module.
const androidCode=await bundle('apps/mobile/src/AndroidGlassSettingsButton.tsx');const a=load(androidCode,name=>name==='react'?React:name==='react-native'?{Platform:{OS:'android'},useColorScheme:()=> 'dark'}:name==='expo'?{requireNativeView:()=> 'NativeGlassButton'}:name==='./spatial-head-indicator'?model:{});
for(const [enabled,tracking] of [[false,false],[true,false],[true,true]]){const el=a.AndroidGlassSettingsButton({spatial:true,spatialEnabled:enabled,trackingEnabled:tracking,headYaw:20,accent:'#123456',onPress(){}});assert.equal(el.props.spatialEnabled,enabled);assert.equal(el.props.spatialTracking,enabled&&tracking);assert.equal(el.props.headYawDegrees,enabled&&tracking?20:0);assert.equal(el.props.accentColorValue,'#123456');}
const kotlin=readFileSync('apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/SdaAndroidChromeModule.kt','utf8');assert.match(kotlin,/tint = if \(spatialButton && spatialEnabled\) accent else ink/);assert.match(kotlin,/rotationY = headAngle/);
console.log('Head indicator: Off neutral, Fixed theme tint without motion, tracking follows real yaw, native menus/fallbacks and Android props passed.');
