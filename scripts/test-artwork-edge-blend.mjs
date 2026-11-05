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
 const blur=load(blurCode,name=>name==='react'?React:name==='react-native'?{Platform:{OS:platform},StyleSheet:{absoluteFill:{position:'absolute',top:0,left:0,right:0,bottom:0}},View:'View',Image:'Image'}:name==='./phone-landscape-blur-model'?model:name==='@expo/ui/community/masked-view'?{MaskedView:'MaskedView'}:name.endsWith('fade-horizontal.png')?'existing-horizontal-fade':name.endsWith('fade.png')?'existing-vertical-fade':{});
 for(const localParent of [false,true]) {
  const el=blur.PhoneLandscapeBlur({...overlay.props,...(!localParent?{parentLeft:0,parentTop:0}:{})});
  const continuation = blur.ArtworkRightFeather(overlay.props);
  if(platform==='web'||!available){assert.equal(el,null);assert.equal(continuation,null);continue;}
  assert.equal(continuation.props.style.left, 460, 'only the outer tenth of the artwork is softened');
  assert.equal(continuation.props.style.width, 140, 'the continuation mainly occupies the area outside the original cover');
  const sideMask = continuation.props.maskElement.props.children;
  assert.equal(sideMask[0].props.style.width, 40);
  assert.equal(sideMask[1].props.style.width, 100);
  assert.deepEqual(sideMask[1].props.style.transform, [{scaleX:-1}], 'blur must fade OUT toward the lyrics');
  const continuationPaint = continuation.props.children;
  assert.equal(continuationPaint.props.testID, 'tablet-artwork-continuation-paint', 'side continuation must not have a second vertical fade');
  const unified = blur.TabletArtworkFeather(overlay.props);
  const [rightPass, bottomPass] = unified.props.children;
  assert.equal(rightPass.type, blur.ArtworkRightFeather, 'right continuation is painted before the bottom pass');
  assert.equal(bottomPass.type, blur.PhoneLandscapeBlur);
  assert.equal(bottomPass.props.edgeDirection, 'bottom');
  assert.equal(bottomPass.props.artworkWidth, 500, 'one bottom ramp spans the cover plus the entire 100pt side extension');
  assert.equal(bottomPass.props.artworkHeight, 400);
  const bottomRendered = bottomPass.type(bottomPass.props);
  assert.equal(bottomRendered.props.style.width, 500);
  assert.equal(bottomRendered.props.style.height, 140, 'approved bottom fade range stays unchanged');
  const [copy, scrim] = continuationPaint.props.children;
  assert.equal(copy.props.style.width, 400); assert.equal(copy.props.style.height, 400);
  assert.equal(copy.props.resizeMode, 'contain'); assert.equal(copy.props.blurRadius, 48);

  assert.equal(el.type,'MaskedView');assert.equal(el.props.style.width,120);assert.equal(el.props.style.height,400);
  assert.equal(el.props.maskElement.props.source,'existing-horizontal-fade');
  const background=el.props.children;assert.equal(background.props.style.backgroundColor,'#243443');
  assert.equal(continuationPaint.props.style.at(-1).backgroundColor, background.props.style.backgroundColor);
  const backdropImageOpacity = background.props.children[0].props.style.at(-1).opacity;
  const backdropScrimColor = background.props.children[1].props.style.at(-1).backgroundColor;
  assert.equal(copy.props.style.opacity, backdropImageOpacity, 'continuation cannot use a brighter copy than the ambient layer');
  assert.equal(scrim.props.style.at(-1).backgroundColor, backdropScrimColor, 'continuation must share the ambient dimming');
  // A flat-colored cover must not gain a bright strip for any feather alpha.
  // Evaluate the actual rendered paint parameters against the ambient paint.
  const scrimOpacity = Number(backdropScrimColor.match(/,([.\d]+)\)/)[1]);
  for (const base of [0, .2, .5, 1]) for (const sample of [0, .1, .7, 1]) for (const alpha of [0, .25, .5, .75, 1]) {
    const ambient = (sample * backdropImageOpacity + base * (1 - backdropImageOpacity)) * (1 - scrimOpacity);
    const edge = (sample * copy.props.style.opacity + base * (1 - copy.props.style.opacity)) * (1 - scrimOpacity);
    assert(Math.abs(edge * alpha + ambient * (1 - alpha) - ambient) < 1e-10, 'feather compositing must preserve uniform backdrop brightness');
  }

  assert.equal(background.props.style.width,1200);assert.equal(background.props.style.height,800);
  assert.equal(background.props.children[0].props.blurRadius,48);
  assert.equal((localParent?100:0)+el.props.style.left+background.props.style.left,0,'blur image must remain aligned to the screen after nesting in a tablet cover');
  assert.equal((localParent?80:0)+el.props.style.top+background.props.style.top,0);
  const narrow = blur.PhoneLandscapeBlur({...overlay.props, edgeWidth: 400 * model.ARTWORK_BACKDROP_FEATHER_FRACTION, testID: 'tablet-artwork-lyrics-feather', ...(!localParent ? {parentLeft:0,parentTop:0} : {})});
  assert.equal(narrow.props.testID, 'tablet-artwork-lyrics-feather');
  assert.equal(narrow.props.style.width, 140);
  assert.equal(narrow.props.style.height, 400);
  assert.equal((localParent ? 100 : 0) + narrow.props.style.left + narrow.props.children.props.style.left, 0, 'tablet feather samples the same background pixels as the lyric backdrop');
  assert.equal(narrow.props.children.props.children[0].props.blurRadius, 48);
  const bottom = blur.PhoneLandscapeBlur({...overlay.props, edgeDirection:'bottom', testID:'tablet-artwork-controls-feather', ...(!localParent ? {parentLeft:0,parentTop:0} : {})});
  assert.equal(bottom.props.style.width, 400); assert.equal(bottom.props.style.height, narrow.props.style.width, 'tablet bottom and right feather extents match');
  assert.equal(bottom.props.maskElement.props.source, 'existing-vertical-fade', 'bottom blend must use a vertical alpha ramp');
  assert.equal((localParent ? 100 : 0) + bottom.props.style.left + bottom.props.children.props.style.left, 0);
  assert.equal((localParent ? 80 : 0) + bottom.props.style.top + bottom.props.children.props.style.top, 0, 'bottom and right edges sample the same screen pixels');
  assert.deepEqual(bottom.props.children.props.children[0].props, narrow.props.children.props.children[0].props, 'both feather directions must use the identical ambient image, blur and opacity');


 }
}
const player=readFileSync('apps/mobile/src/IOSPlayer.tsx','utf8');
assert.match(player,/<ArtworkEdgeBlend landscape=\{landscapePlayer\}/);assert.match(player,/measureLayout\(surface,/);
assert.match(player,/testID="tablet-ambient-artwork"/);assert.match(player,/<PhoneLandscapeBlur edge /);
assert.doesNotMatch(readFileSync('apps/mobile/src/ArtworkEdgeBlend.tsx','utf8'),/MaskedView|artwork-edge-blend-mask/);
console.log('Artwork regression: portrait sharp cover unchanged; phone/tablet landscape share the Gaussian overlap matching the original gradient with screen-aligned blur and native fallback.');
