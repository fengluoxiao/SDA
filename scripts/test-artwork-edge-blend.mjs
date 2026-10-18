import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {build} from 'esbuild';
const require=createRequire(import.meta.url);const React=require('react');
const code=(await build({entryPoints:['apps/mobile/src/ArtworkEdgeBlend.tsx'],bundle:true,platform:'node',format:'cjs',write:false,external:['*']})).outputFiles[0].text;
for(const platform of ['ios','android','web'])for(const available of [false,true]){
 globalThis.expo={modules:{ExpoUI:available},getViewConfig:()=>available};
 const m={exports:{}};new Function('require','module','exports',code)(name=>name==='react'?React:name==='react-native'?{Platform:{OS:platform},StyleSheet:{absoluteFill:{position:'absolute',top:0,left:0,right:0,bottom:0}},Image:'Image'}:name==='@expo/ui/community/masked-view'?{MaskedView:'MaskedView'}:name.endsWith('.png')?'edge-mask':{},m,m.exports);
 const image=React.createElement('Image',{resizeMode:'contain',source:{uri:'cover'},style:{width:'100%',height:'100%',borderRadius:18}});const el=m.exports.ArtworkEdgeBlend({children:image});
 if(platform==='web'||!available)assert.equal(el,image);
 else {assert.equal(el.type,'MaskedView');assert.equal(el.props.children,image);assert.equal(el.props.maskElement.props.source,'edge-mask');assert.equal(el.props.maskElement.props.resizeMode,'stretch');assert.deepEqual(el.props.style,{width:'100%',height:'100%'});}
}
const player=readFileSync('apps/mobile/src/IOSPlayer.tsx','utf8');assert.match(player,/<ArtworkEdgeBlend><Image source=\{\{ uri: artworkUri \}\} resizeMode="contain"/);assert.match(player,/testID="tablet-ambient-artwork"/);assert.match(player,/<PhoneLandscapeBlur edge uri=/);
console.log('Tablet feather: original cover geometry retained, actual blurred backdrop revealed at edges, native masks and unavailable-manager fallback passed.');
