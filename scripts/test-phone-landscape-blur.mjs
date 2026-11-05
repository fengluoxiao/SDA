import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {build} from 'esbuild';
const result=await build({entryPoints:['apps/mobile/src/phone-landscape-blur-model.ts'],bundle:true,platform:'node',format:'esm',write:false});
const {phoneLandscapeBlurEdge,artworkBackdropBottomEdge,artworkRightContinuation}=await import('data:text/javascript;base64,'+Buffer.from(result.outputFiles[0].text).toString('base64'));
for(const width of [320,375,390,430,500])for(const left of [0,24,44]) {
 const frame=phoneLandscapeBlurEdge(left,0,width,width);
 assert.equal(frame.width,width*.30);
 assert.equal(frame.left+frame.width,left+width);assert.equal(frame.height,width);
 assert(Math.abs(frame.left-(left+width*.70))<1e-8);
}
// A narrow tablet blend must end exactly at the original cover edge; it may
// never move/scale the cover or widen into the lyrics' touch targets.
for (const width of [0, 12, 320, 549, 645, 731]) for (const left of [0, 24, 96]) {
 const frame = phoneLandscapeBlurEdge(left, 8, width, width, 24);
 assert.equal(frame.width, Math.min(width, 24));
 assert.equal(frame.left + frame.width, left + width);
 assert.equal(frame.top, 8); assert.equal(frame.height, width);
}
// The bottom blend finishes at the same cover boundary and never moves the
// controls or samples a differently aligned background at the lower corner.
for (const width of [320, 549, 645, 731]) for (const height of [0, 12, 400, 645]) {
 const frame = artworkBackdropBottomEdge(24, 8, width, height);
 assert.equal(frame.left, 24); assert.equal(frame.width, width);
 assert.equal(frame.height, height * .35);
 assert(Math.abs(frame.top + frame.height - (8 + height)) < 1e-8);
 const clamped = artworkBackdropBottomEdge(24, 8, width, height, 1000);
 assert.equal(clamped.height, height); assert.equal(clamped.top, 8);
}
// Most of the side blend must live outside the sharp album, preserving detail.
for (const size of [320, 549, 645, 731]) {
 const continuation = artworkRightContinuation(24, 8, size, size);
 assert(continuation.innerWidth >= 32 && continuation.innerWidth <= 48, 'tablet boundary needs a gradual visible transition');
 assert(24 + size - continuation.left <= size * .10 + 1e-8, 'at least ninety percent of the sharp cover remains intact');
 assert(continuation.outerWidth >= continuation.innerWidth * 2.5, 'the transition primarily extends outside the cover');
 assert.equal(continuation.left + continuation.width, 24 + size + continuation.outerWidth);
 assert.equal(continuation.sourceWidth, size, 'the blurred copy must keep the source dimensions');
 assert(Math.abs(continuation.imageLeft + continuation.sourceWidth - continuation.width) < 1e-8, 'the copy ends exactly where its outer fade reaches zero');
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
assert.match(source, /tabletLandscape && hasArtwork && <TabletArtworkFeather/);
assert.doesNotMatch(edge, /tablet-artwork-continuation-bottom/);
console.log('Phone landscape blur: original-gradient overlap, unchanged sharp cover, screen-aligned backdrop and no generated image/cache files passed.');
