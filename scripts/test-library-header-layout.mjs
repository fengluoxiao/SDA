import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {build} from 'esbuild';
const source=readFileSync('apps/mobile/src/IOSPlayer.tsx','utf8');
const expression=source.match(/scrollIndicatorInsets=\{(\{[^}]+\})\}/)[1];
const indicator=new Function('libraryHeaderOverlay','homeHeaderHeight','sideNavigationRight','SIDE_NAVIGATION_WIDTH','sideNavigationActive','p','screenInsets','return '+expression);
for(const height of [64,74,90]) for(const safeRight of [0,44,59,62]) {
  const inset=indicator(true,height,true,48,true,{selectedUri:'mock'},{right:safeRight});
  assert.equal(inset.top,height+10);assert.equal(inset.bottom,62);
  const railLeft=932-(safeRight+8)/2-48;
  const contentRight=932-safeRight-56;
  const indicatorX=932-safeRight-inset.right;
  assert(indicatorX>contentRight,'indicator must not overlap row arrows');
  assert(indicatorX<railLeft,'indicator must not overlap side controls');
  assert.equal(indicator(true,height,false,48,false,{selectedUri:''},{right:safeRight}).right,3);
  const leftMode=indicator(true,height,false,48,true,{selectedUri:'mock'},{right:safeRight});
  assert.equal(leftMode.right,3);
  const leftModeIndicatorX=932-safeRight-leftMode.right;
  assert(leftModeIndicatorX<932-safeRight,'left mode indicator remains inside native screen clipping bounds');
  assert.equal(leftModeIndicatorX-(932-safeRight-18),15,'left mode reserves a real 15 pt card-to-scrollbar gap');
  assert.equal(inset.right,56-Math.min(12,(safeRight+8)/4),'approved right-side formula must remain unchanged');
}
assert(source.indexOf('{libraryHeaderOverlay && homeHeader}')>source.indexOf('</IOSSystemTabs>'),'mount blur after scrolling content');
const built=await build({entryPoints:['apps/mobile/src/IOSLibraryHeaderBackdrop.tsx'],bundle:true,platform:'node',format:'cjs',write:false,external:['react','react-native','react-native-safe-area-context','./IOSHeaderBlur']});
const module={exports:{}};let insets={left:59,right:59,top:0,bottom:21};
const react={createElement:(type,props,...children)=>({type,props:{...props,children}})};
new Function('require','module','exports',built.outputFiles[0].text)(name=>{
 if(name==='react')return react;
 if(name==='react-native')return {StyleSheet:{absoluteFill:{position:'absolute',top:0,left:0,right:0,bottom:0}}};
 if(name==='react-native-safe-area-context')return {useSafeAreaInsets:()=>insets};
 if(name==='./IOSHeaderBlur')return {IOSHeaderBlur:'NativeHeaderBlur'};
 throw new Error(name);
},module,module.exports);
for(const safe of [{left:59,right:59,top:0,bottom:21},{left:0,right:0,top:59,bottom:34}]) {
 insets=safe;const tree=module.exports.IOSLibraryHeaderBackdrop({dark:false});
 assert.equal(tree.type,'NativeHeaderBlur');assert.equal(tree.props.style[1].left,-safe.left-18);assert.equal(tree.props.style[1].top,-safe.top);
 const settings=module.exports.IOSLibraryHeaderBackdrop({dark:false,horizontalPadding:0});
 assert.equal(settings.props.style[1].left,-safe.left);
 assert.equal(settings.props.style[1].right,-safe.right);
 assert.equal(settings.props.style[1].top,tree.props.style[1].top);
}
const blurBuild=await build({entryPoints:['apps/mobile/src/IOSHeaderBlur.tsx'],bundle:true,platform:'node',format:'cjs',write:false,external:['react','react-native','expo-blur','@expo/ui/community/masked-view','*.png']});
const previous=globalThis.expo;
try {
 for(const available of [true,false,'throws']) {
  let imports=0;const m={exports:{}};
  globalThis.expo={getViewConfig:()=>{if(available==='throws')throw Error('missing');return available;}};
  new Function('require','module','exports','__DEV__',blurBuild.outputFiles[0].text)(name=>{
   if(name==='react')return react;
   if(name==='react-native')return {Platform:{OS:'ios'},View:'View',Image:'Image',Animated:{View:'AnimatedView'},StyleSheet:{absoluteFill:{position:'absolute'}}};
   if(name==='expo-blur'){imports++;return {BlurView:'ExpoBlur'};}
   if(name==='@expo/ui/community/masked-view')return {MaskedView:'MaskedView'};
   if(name.endsWith('.png'))return 'edge-mask';
   throw Error(name);
  },m,m.exports,false);
  const tree=m.exports.IOSHeaderBlur({dark:false});
  assert.equal(imports,available===true?1:0);
  assert.equal(tree.props.pointerEvents,'none');
  if(available===true){assert.equal(tree.type,'AnimatedView');const mask=tree.props.children[0];assert.equal(mask.type,'MaskedView');assert.equal(tree.props.style[2].bottom,-16);assert.equal(mask.props.maskElement.props.children[1].props.style.height,16);const blur=mask.props.children[0];assert.equal(blur.type,'ExpoBlur');assert.equal(blur.props.intensity,100);assert.equal(blur.props.tint,'systemChromeMaterialLight');assert.equal(tree.props.style[1].opacity,1);const idle=m.exports.IOSHeaderBlur({dark:false,opacity:0});assert.equal(idle.props.style[1].opacity,0);}
  else {assert.equal(tree.type,'View');assert.equal(tree.props.style[1].backgroundColor,'#f2f2f7','fallback must not expose overlapping text');}
 }
} finally {globalThis.expo=previous;}
console.log('Library: scrollbar clears row arrows, side controls and mini-player; blur mounts after content and safely falls back on missing native support.');
