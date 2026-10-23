import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {build} from 'esbuild';
const result=await build({entryPoints:['apps/mobile/src/phone-landscape-blur-model.ts'],bundle:true,platform:'node',format:'esm',write:false});
const {phoneLandscapeBlurEdge}=await import('data:text/javascript;base64,'+Buffer.from(result.outputFiles[0].text).toString('base64'));
for(const width of [320,375,390,430,500])for(const left of [0,24,44]) {
 const frame=phoneLandscapeBlurEdge(left,0,width,width);
 assert.equal(frame.width,width*.30);
 assert.equal(frame.left+frame.width,left+width);assert.equal(frame.height,width);
 assert(Math.abs(frame.left-(left+width*.70))<1e-8);
}
const source=readFileSync('apps/mobile/src/IOSPlayer.tsx','utf8');
assert.match(source,/\.\.\.landscapeFade, tintColor: fadeColor/,'gradient and Gaussian must share the same artwork overlap geometry');
assert(source.indexOf('<PhoneLandscapeBlur backgroundColor=')>=0);
assert(source.indexOf('<PhoneLandscapeBlur backgroundColor=')<source.indexOf('key={artworkUri}'));
assert(source.indexOf('<PhoneLandscapeBlur edge ')>source.indexOf('key={artworkUri}'));
const movingCover=source.indexOf('<SharedMiniPlayerArtwork '), edgeLayer=source.indexOf('testID="player-artwork-edge-overlay"');
assert(movingCover>source.indexOf('key={artworkUri}'),'moving cover overlays the settled artwork');
assert(edgeLayer>movingCover,'the original fade must cover the moving image throughout the return');
assert(source.indexOf('<PhoneLandscapeBlur edge ')>edgeLayer,'Gaussian treatment shares the same layer above both artwork views');
assert(source.indexOf('<SafeAreaView edges=',edgeLayer)>edgeLayer,'page text and controls stay above the artwork and its fade');
const edge=readFileSync('apps/mobile/src/PhoneLandscapeBlur.tsx','utf8');
assert.match(edge,/community\/masked-view/);assert.match(edge,/p.edge \? -frame.left : 0/);
assert.doesNotMatch(edge,/writeAsString|saveTo|captureRef/);
console.log('Phone landscape blur: original-gradient overlap, unchanged sharp cover, screen-aligned backdrop and no generated image/cache files passed.');
