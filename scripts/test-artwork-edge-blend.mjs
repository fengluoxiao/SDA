import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {build} from 'esbuild';
const require=createRequire(import.meta.url), React=require('react');
const compile=async file=>(await build({entryPoints:[file],bundle:true,platform:'node',format:'cjs',write:false,external:['*']})).outputFiles[0].text;
function load(code,require){const m={exports:{}};new Function('require','module','exports',code)(require,m,m.exports);return m.exports;}
const model=load(await compile('apps/mobile/src/phone-landscape-blur-model.ts'),require);
const wrapper=load(await compile('apps/mobile/src/ArtworkEdgeBlend.tsx'),name=>name==='react'?React:name==='react-native'?{View:'View'}:{PhoneLandscapeBlur:'GaussianEdge'});
const image=React.createElement('Image',{resizeMode:'contain',source:{uri:'cover'},style:{width:'100%',height:'100%',borderRadius:18}});
const frame={left:100,top:80,width:400,height:400};
const props={children:image,uri:'cover',surfaceWidth:1200,surfaceHeight:800,backgroundColor:'#243443',frame};
assert.equal(wrapper.ArtworkEdgeBlend({...props,landscape:false}),image,'portrait must return the original sharp cover without a mask or edge overlay');
assert.equal(wrapper.ArtworkEdgeBlend({...props,landscape:true,frame:null}),image,'unmeasured landscape must retain the original image');
const rendered=wrapper.ArtworkEdgeBlend({...props,landscape:true});
assert.equal(rendered.type,'View');assert.equal(rendered.props.children[0],image);
const overlay=rendered.props.children[1];assert.equal(overlay.type,'GaussianEdge');assert.equal(overlay.props.edge,true);
assert.equal(overlay.props.parentLeft,100);assert.equal(overlay.props.parentTop,80);assert.equal(overlay.props.backgroundColor,'#243443');
const blurCode=await compile('apps/mobile/src/PhoneLandscapeBlur.tsx');
for(const platform of ['ios','android','web'])for(const available of [false,true]) {
 globalThis.expo={modules:{ExpoUI:available},getViewConfig:()=>available};
 const blur=load(blurCode,name=>name==='react'?React:name==='react-native'?{Platform:{OS:platform},StyleSheet:{absoluteFill:{position:'absolute',top:0,left:0,right:0,bottom:0}},View:'View',Image:'Image'}:name==='./phone-landscape-blur-model'?model:name==='@expo/ui/community/masked-view'?{MaskedView:'MaskedView'}:name.endsWith('.png')?'existing-horizontal-fade':{});
 for(const localParent of [false,true]) {
  const el=blur.PhoneLandscapeBlur({...overlay.props,...(!localParent?{parentLeft:0,parentTop:0}:{})});
  if(platform==='web'||!available){assert.equal(el,null);continue;}
  assert.equal(el.type,'MaskedView');assert.equal(el.props.style.width,120);assert.equal(el.props.style.height,400);
  assert.equal(el.props.maskElement.props.source,'existing-horizontal-fade');
  const background=el.props.children;assert.equal(background.props.style.backgroundColor,'#243443');
  assert.equal(background.props.style.width,1200);assert.equal(background.props.style.height,800);
  assert.equal(background.props.children[0].props.blurRadius,48);
  assert.equal((localParent?100:0)+el.props.style.left+background.props.style.left,0,'blur image must remain aligned to the screen after nesting in a tablet cover');
  assert.equal((localParent?80:0)+el.props.style.top+background.props.style.top,0);
 }
}
const player=readFileSync('apps/mobile/src/IOSPlayer.tsx','utf8');
assert.match(player,/<ArtworkEdgeBlend landscape=\{landscapePlayer\}/);assert.match(player,/measureLayout\(surface,/);
assert.match(player,/testID="tablet-ambient-artwork"/);assert.match(player,/<PhoneLandscapeBlur edge /);
assert.doesNotMatch(readFileSync('apps/mobile/src/ArtworkEdgeBlend.tsx','utf8'),/MaskedView|artwork-edge-blend-mask/);
console.log('Artwork regression: portrait sharp cover unchanged; phone/tablet landscape share the Gaussian overlap matching the original gradient with screen-aligned blur and native fallback.');
