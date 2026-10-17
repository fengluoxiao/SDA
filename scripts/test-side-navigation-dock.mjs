import assert from 'node:assert/strict';
import {build} from 'esbuild';
const built=await build({entryPoints:['apps/mobile/src/SideNavigationDock.tsx'],bundle:true,platform:'node',format:'cjs',write:false,external:['react','react-native','react-native-safe-area-context','./SideNavigationRail']});
let insets={left:0,right:0,top:0,bottom:0};
const react={createElement:(type,props,...children)=>({type,props:{...props,children}})};
const module={exports:{}};
new Function('require','module','exports',built.outputFiles[0].text)(name=>{
  if(name==='react')return react;
  if(name==='react-native')return {StyleSheet:{absoluteFill:{position:'absolute',left:0,right:0,top:0,bottom:0}},View:'View'};
  if(name==='react-native-safe-area-context')return {SafeAreaView:'SafeAreaView',useSafeAreaInsets:()=>insets};
  if(name==='./SideNavigationRail')return {SIDE_NAVIGATION_WIDTH:48};
  throw new Error(name);
},module,module.exports);
for(const safe of [0,20,44,59,62]) for(const placement of ['top','bottom']) {
  insets={left:safe,right:safe,top:0,bottom:21};
  const right=module.exports.SideNavigationDock({side:'right',placement,testID:'dock',children:'buttons'});
  const style=right.props.style[1];const lane=right.props.children[0].props.style;
  const screenGap=style.paddingRight+lane.marginHorizontal;
  const contentGap=safe+48+8-screenGap-lane.width;
  assert.equal(screenGap,contentGap,'right rail must have equal space to content and screen');
  assert.equal(style.paddingTop,placement==='top'?16:0);
  assert.equal(style.paddingBottom,placement==='bottom'?8:0);
  const left=module.exports.SideNavigationDock({side:'left',placement,testID:'dock',children:'buttons'});
  assert.equal(left.props.edges.left,'maximum');
  assert.equal(Math.max(safe,left.props.style[1].paddingLeft)+left.props.children[0].props.style.marginHorizontal,Math.max(safe,14)+6,'approved left position must stay identical');
}
console.log('Side dock: equal right-side gaps at five device insets; top/bottom alignment and approved left spacing preserved.');
