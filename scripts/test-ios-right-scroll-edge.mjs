import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {build} from 'esbuild';
const built=await build({entryPoints:['apps/mobile/src/IOSRightScrollEdge.tsx'],bundle:true,platform:'node',format:'cjs',write:false,external:['react','react-native','expo','react-native-safe-area-context','./SideNavigationRail']});
const previous=globalThis.expo;
try {
 for(const supported of [true,false,'throws']) {
  const m={exports:{}};let imports=0;
  globalThis.expo={modules:{SdaScrollEdge:{isSupported:()=>{if(supported==='throws')throw Error('missing');return supported;}}}};
  new Function('require','module','exports',built.outputFiles[0].text)(name=>{
   if(name==='react')return {Fragment:'Fragment',createElement:(type,props,...children)=>({type,props:{...props,children}})};
   if(name==='react-native')return {Platform:{OS:'ios',Version:26}};
   if(name==='expo')return {requireNativeView:name=>{imports++;assert.equal(name,'SdaScrollEdge');return 'NativeEdge';}};
   if(name==='react-native-safe-area-context')return {useSafeAreaInsets:()=>({right:59})};
   if(name==='./SideNavigationRail')return {SIDE_NAVIGATION_WIDTH:48};
   throw Error(name);
  },m,m.exports);
  const tree=m.exports.IOSRightScrollEdge({scrollViewTag:42,children:'actual-controls'});
  assert.equal(imports,supported===true?1:0);
  assert.equal(tree.props.children[0],'actual-controls');
  if(supported===true){assert.equal(tree.type,'NativeEdge');assert.equal(tree.props.scrollViewTag,42);assert.equal(tree.props.style.width,91);}
  else assert.equal(tree.type,'Fragment');
 }
} finally {globalThis.expo=previous;}
const swift=readFileSync('apps/mobile/modules/sda-core/ios/SdaScrollEdgeModule.swift','utf8');
assert.match(swift,/UIScrollEdgeElementContainerInteraction/);assert.match(swift,/interaction.edge = \.right/);
assert.match(swift,/target.rightEdgeEffect.style = \.hard/);assert.match(swift,/target.rightEdgeEffect.style = savedStyle/);
assert.doesNotMatch(swift,/UIBlurEffect|UIVisualEffectView|Timer|FileManager/);
const config=JSON.parse(readFileSync('apps/mobile/modules/sda-core/expo-module.config.json','utf8'));
assert(config.apple.modules.includes('SdaScrollEdgeModule'));assert(!config.android.modules.includes('SdaScrollEdgeModule'));
console.log('Native right edge adapter and source contracts passed; UIKit compilation/device validation still required.');
