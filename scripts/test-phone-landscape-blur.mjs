import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {build} from 'esbuild';
const result=await build({entryPoints:['apps/mobile/src/phone-landscape-blur-model.ts'],bundle:true,platform:'node',format:'esm',write:false});
const {phoneLandscapeBlurEdge}=await import('data:text/javascript;base64,'+Buffer.from(result.outputFiles[0].text).toString('base64'));
for(const width of [320,375,390,430,500])for(const left of [0,24,44]) {
 const frame=phoneLandscapeBlurEdge(left,0,width,width);
 assert(frame.width<=32);assert(frame.width<=width*.08);
 assert.equal(frame.left+frame.width,left+width);assert.equal(frame.height,width);
 assert(frame.left+1e-8>=left+width*.92);
}
const source=readFileSync('apps/mobile/src/IOSPlayer.tsx','utf8');
assert(source.indexOf('<PhoneLandscapeBlur uri=')<source.indexOf('key={artworkUri}'));
assert(source.indexOf('<PhoneLandscapeBlur edge ')>source.indexOf('key={artworkUri}'));
const edge=readFileSync('apps/mobile/src/PhoneLandscapeBlur.tsx','utf8');
assert.match(edge,/community\/masked-view/);assert.match(edge,/p.edge \? -frame.left : 0/);
assert.doesNotMatch(edge,/writeAsString|saveTo|captureRef/);
console.log('Phone landscape blur: narrow masked edge, unchanged sharp cover, screen-aligned backdrop and no generated image/cache files passed.');
