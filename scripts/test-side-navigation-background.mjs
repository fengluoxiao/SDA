import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {build} from 'esbuild';
const source=readFileSync('apps/mobile/src/IOSPlayer.tsx','utf8');
assert.match(source,/sideNavigationActive && sideNavigationRight && !settings && <IOSSideNavigationBackground/);
assert(source.indexOf('<IOSSideNavigationBackground')<source.indexOf('<SideNavigationDock'),'background stays below both control docks');
assert.doesNotMatch(readFileSync('apps/mobile/src/SideNavigationRail.tsx','utf8'),/IOSSideRailBackdrop|expo-blur/,'capsule change was withdrawn');
const built=await build({entryPoints:['apps/mobile/src/IOSSideNavigationBackground.tsx'],bundle:true,platform:'node',format:'cjs',write:false,external:['react','react-native','react-native-safe-area-context','./SideNavigationRail','expo-blur']});
const previous=globalThis.expo;
try {
 for(const available of [true,false,'throws']) for(const right of [0,44,59,62]) {
  let imports=0;const m={exports:{}};
  globalThis.expo={getViewConfig:()=>{if(available==='throws')throw Error('missing');return available;}};
  new Function('require','module','exports','__DEV__',built.outputFiles[0].text)(name=>{
   if(name==='react')return {createElement:(type,props,...children)=>({type,props:{...props,children}})};
   if(name==='react-native')return {Platform:{OS:'ios'},View:'View',StyleSheet:{absoluteFill:{position:'absolute'}}};
   if(name==='react-native-safe-area-context')return {useSafeAreaInsets:()=>({right})};
   if(name==='./SideNavigationRail')return {SIDE_NAVIGATION_WIDTH:48};
   if(name==='expo-blur'){imports++;return {BlurView:'UIKitBlur'};}
   throw Error(name);
  },m,m.exports,false);
  const tree=m.exports.IOSSideNavigationBackground({dark:false});
  assert.equal(tree.props.pointerEvents,'none');assert.equal(tree.props.style.top,0);assert.equal(tree.props.style.bottom,0);assert.equal(tree.props.style.right,0);
  assert.equal(tree.props.style.borderRadius,undefined,'full side region, not capsule');
  const gap=Math.min(12,(right+8)/4);
  const edge=932-tree.props.style.width;
  assert.equal(edge,932-right-56+2*gap,'background meets native content viewport');
  const indicator=932-right-56+gap;assert(edge>indicator,'background never blurs the scrollbar');
  if(available===true){assert.equal(imports,1);assert.equal(tree.props.children[0].type,'UIKitBlur');}
  else {assert.equal(imports,0);assert.equal(tree.props.children[0].type,'View');}
 }
} finally {globalThis.expo=previous;}
console.log('Side background: full-height right region, control layering, device-inset alignment, scrollbar clearance and safe fallback passed.');
