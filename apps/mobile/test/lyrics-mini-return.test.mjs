import assert from 'node:assert/strict';
import {test} from 'node:test';
import {build} from 'esbuild';
const compiled=await build({entryPoints:['apps/mobile/src/SharedMiniPlayerTransition.tsx'],bundle:true,platform:'node',format:'cjs',external:['react','react-native','./IOSPlaybackSymbol','./ios-ui-model'],write:false});
class Value{constructor(value){this.value=value;}interpolate(config){return{source:this,config};}}
const React={memo:fn=>fn,useEffect(){},useMemo:fn=>fn(),useRef:current=>({current}),createElement:(type,props,...children)=>({type,props:{...props,children}})};
const native={Animated:{Value,View:'AnimatedView'},View:'View',Text:'Text',Image:'Image',StyleSheet:{absoluteFill:{position:'absolute',top:0,bottom:0,left:0,right:0},create:styles=>styles}};
const mod={exports:{}};new Function('require','module','exports',compiled.outputFiles[0].text)(name=>name==='react'?React:name==='react-native'?native:name.endsWith('ios-ui-model')?{formatPlaybackTime:()=> '0:00'}:{IOSPlaybackSymbol:'PlaybackSymbol'},mod,mod.exports);
const frames={artwork:{x:0,y:0,width:390,height:390},progress:{x:450,y:220,width:280,height:5},time:{x:450,y:240,width:24,height:14},mini:{x:780,y:42,width:48,height:54}};
const props={frames,active:true,motionId:1,positionMs:0,durationMs:10000,home:true,targetColor:'#fff',backgroundColor:'#303034'};
test('returning to lyrics fades progress and time at the mini-player without moving them toward the cover',()=>{
 const travel=new Value(1);
 for(const phase of [1,.75,.25,0]){
  travel.value=phase;const tree=mod.exports.SharedMiniPlayerTransition({...props,value:travel,stationaryProgress:true});
  const layer=tree.props.children[0],base=layer.props.children[0].props.children[0],fill=layer.props.children[1],time=layer.props.children[2];
  assert.equal(layer.props.style.at(-1).opacity,travel);assert.notEqual(base.props.value,travel);assert.equal(base.props.value.value,1);assert.equal(fill.props.value.value,1);
  const transform=time.props.style[1].transform;assert.equal(transform[0].translateX.source.value,1);assert.equal(transform[0].translateX.config.outputRange.at(-1),780+24-12);
  assert.equal(transform[1].translateY.config.outputRange.at(-1),42+48.5-7);
 }
});
test('returning to normal playback keeps the existing moving progress animation',()=>{
 const travel=new Value(.5),tree=mod.exports.SharedMiniPlayerTransition({...props,value:travel,stationaryProgress:false}),layer=tree.props.children[0];
 assert.equal(layer.props.style.at(-1).opacity,1);assert.equal(layer.props.children[0].props.children[0].props.value,travel);assert.equal(layer.props.children[1].props.value,travel);
});

test('returning progress uses the real slider alpha over artwork instead of an opaque gray fallback',()=>{
 for(const backgroundColor of ['rgb(63,78,63)','#3f4e3f','rgba(63,78,63,1)']){
  const travel=new Value(.5),tree=mod.exports.SharedMiniPlayerTransition({...props,value:travel,backgroundColor,stationaryProgress:false});
  const layer=tree.props.children[0],paint=layer.props.children[0],track=paint.props.children[0],fill=layer.props.children[1];
  assert.equal(paint.props.needsOffscreenAlphaCompositing,true,'segment joins must be composited as one translucent track');assert.equal(paint.props.shouldRasterizeIOS,true,'the iOS Fabric path must rasterize the opaque children before applying group alpha');assert.equal(paint.props.collapsable,false);
  assert.equal(paint.props.style.at(-1).opacity,.24);assert.equal(track.props.color,'#ffffff');assert.equal(fill.props.color,'#c9c9ce');
 }
});
