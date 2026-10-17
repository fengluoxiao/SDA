import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {build} from 'esbuild';
const source=readFileSync('apps/mobile/src/IOSPlayer.tsx','utf8');
const expression=source.match(/scrollIndicatorInsets=\{(\{[^}]+\})\}/)[1];
const indicator=new Function('libraryHeaderOverlay','homeHeaderHeight','sideNavigationRight','SIDE_NAVIGATION_WIDTH','return '+expression);
for(const height of [64,74,90]) {
  assert.deepEqual(indicator(true,height,true,48),{top:height+10,bottom:10,right:56,left:0});
  assert.equal(indicator(true,height,false,48).right,3);
  assert.equal(indicator(false,height,false,48).top,10);
  for(const safeRight of [0,44,59,62]) {
    const railLeft=932-(safeRight+8)/2-48;
    const indicatorX=932-safeRight-indicator(true,height,true,48).right;
    assert(indicatorX<railLeft,'indicator stays on the content side of both side controls');
  }
}
const built=await build({entryPoints:['apps/mobile/src/IOSLibraryHeaderBackdrop.tsx'],bundle:true,platform:'node',format:'cjs',write:false,external:['react','react-native','react-native-safe-area-context','./IOSSystemTabs','@expo/ui/swift-ui/modifiers']});
const module={exports:{}};let insets={left:59,right:59,top:0,bottom:21};
const react={createElement:(type,props,...children)=>({type,props:{...props,children}}),useState:()=>[{width:932,height:64},()=>{}]};
new Function('require','module','exports',built.outputFiles[0].text)(name=>{
 if(name==='react')return react;
 if(name==='react-native')return {View:'View',StyleSheet:{absoluteFill:{position:'absolute',top:0,left:0,right:0,bottom:0}}};
 if(name==='react-native-safe-area-context')return {useSafeAreaInsets:()=>insets};
 if(name==='./IOSSystemTabs')return {swiftUI:{Host:'Host',VStack:'VStack'}};
 if(name==='@expo/ui/swift-ui/modifiers')return new Proxy({},{get:(_,name)=>(...args)=>({name,args})});
 throw new Error(name);
},module,module.exports);
const previous=globalThis.expo;
try {
 globalThis.expo={getViewConfig:()=>true};
 for(const safe of [{left:59,right:59,top:0,bottom:21},{left:0,right:0,top:59,bottom:34}]) {
  insets=safe;const tree=module.exports.IOSLibraryHeaderBackdrop({dark:false});
  assert.equal(tree.props.pointerEvents,'none');
  assert.equal(tree.props.style[1].left,-safe.left-18);assert.equal(tree.props.style[1].top,-safe.top);
  const host=tree.props.children[0];assert.equal(host.props.ignoreSafeArea,'container');
  assert.deepEqual(host.props.children[0].props.modifiers.find(m=>m.name==='background').args[0],{type:'material',material:'bar'});
 }
 globalThis.expo={getViewConfig:()=>false};
 const fallback=module.exports.IOSLibraryHeaderBackdrop({dark:true});assert.equal(fallback.props.children[0],false);
} finally {globalThis.expo=previous;}
console.log('Library header/indicator: portrait and landscape backdrop coverage, native material, missing-view fallback and side-control clearance passed.');
