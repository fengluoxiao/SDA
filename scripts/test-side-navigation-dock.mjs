import assert from 'node:assert/strict';
import {build} from 'esbuild';
const built=await build({entryPoints:['apps/mobile/src/SideNavigationDock.tsx'],bundle:true,platform:'node',format:'cjs',write:false,external:['react','react-native','react-native-safe-area-context','./SideNavigationRail']});
let insets={left:0,right:0,top:0,bottom:0};
const react={createElement:(type,props,...children)=>({type,props:{...props,children}})};
const module={exports:{}};
new Function('require','module','exports',built.outputFiles[0].text)(name=>{
  if(name==='react')return react;
  if(name==='react-native')return {Platform:{OS:'ios'},StyleSheet:{absoluteFill:{position:'absolute',left:0,right:0,top:0,bottom:0}},View:'View'};
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
  assert.equal(screenGap-safe,contentGap,'right rail centers in the usable gap after the full safe inset');
  assert(screenGap-48*(1.04-1)/2>=safe,'pressed capsule stays outside the cutout');
  assert(screenGap-(44*1.18-48)/2>=safe,'enlarged selection stays outside the cutout');
  assert.equal(style.paddingTop,placement==='top'?16:0);
  assert.equal(style.paddingBottom,placement==='bottom'?8:0);
  const left=module.exports.SideNavigationDock({side:'left',placement,testID:'dock',children:'buttons'});
  assert.equal(left.props.edges.left,'off','explicit dock offset already contains its safe inset');
  assert.equal(left.props.style[1].paddingLeft+left.props.children[0].props.style.marginHorizontal,Math.max(safe,14)+6,'approved left position must stay identical');
}
for (const [leftInset,rightInset] of [[59,0],[0,59],[62,0],[0,62],[59,59],[0,0]]) {
  insets={left:leftInset,right:rightInset,top:0,bottom:21};
  const dock=module.exports.SideNavigationDock({side:'right',placement:'bottom',testID:'rotated',children:'rail'});
  assert.equal(dock.props.style[1].paddingRight,rightInset+4,'rotation follows the right inset, regardless of the left inset');
}
console.log('Side dock: full cutout inset in both rotations, press expansion safety and unchanged left positioning passed.');

for (const rightInset of [0,44,59,62]) for(const placement of ['top','bottom']) {
 insets={left:rightInset,right:rightInset,top:0,bottom:21};
 const same=module.exports.SideNavigationDock({side:'right',placement,testID:'same',children:'rail',cutoutSide:'right'});
 const opposite=module.exports.SideNavigationDock({side:'right',placement,testID:'opposite',children:'rail',cutoutSide:'left'});
 assert.equal(same.props.style[1].paddingRight,rightInset+4);
 assert.equal(opposite.props.style[1].paddingRight,(rightInset+8)/2);
 const leftSame=module.exports.SideNavigationDock({side:'left',placement,testID:'left',children:'rail',cutoutSide:'left'});
 const leftOpposite=module.exports.SideNavigationDock({side:'left',placement,testID:'left',children:'rail',cutoutSide:'right'});
 assert.equal(leftSame.props.style[1].paddingLeft,Math.max(14,rightInset)+6);
 assert.equal(leftOpposite.props.style[1].paddingLeft,Math.max(20,(rightInset+8)/2));
}

for(const side of ['left','right'])for(const placement of ['top','bottom']) {
 insets={left:24,right:12,top:24,bottom:20};
 const dock=module.exports.SideNavigationDock({side,placement,testID:'tablet-dock',children:'rail',tablet:true});
 const style=dock.props.style[1];
 assert.equal(style.justifyContent,placement==='top'?'flex-start':'flex-end');
 assert.equal(side==='left'?style.paddingLeft:style.paddingRight,(side==='left'?insets.left:insets.right)+20);
}
console.log('Tablet docks: bottom-anchored native navigation, top actions and inset-aware 20pt outer clearance passed.');
