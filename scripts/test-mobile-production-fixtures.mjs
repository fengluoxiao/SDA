import assert from 'node:assert/strict';
import {build} from 'esbuild';
const result=await build({entryPoints:['apps/mobile/index.js'],bundle:true,platform:'node',format:'cjs',packages:'external',external:['*.png','*.jpg','*.jpeg','*.webp','*.ttf'],define:{__DEV__:'false','process.env.NODE_ENV':'"production"'},minifySyntax:true,metafile:true,write:false});
assert(!Object.keys(result.metafile.inputs).some(name=>name.includes('dev-preview/')),'development preview fixtures entered the production bundle');
assert(!result.outputFiles[0].text.includes('mock://'),'mock media entered the production bundle');
console.log('Production JavaScript bundle excludes development preview fixtures and mock media.');

const productionInputs = new Set(Object.keys(result.metafile.inputs));
for (const source of ['apps/mobile/src/TabletLandscapePlayer.tsx', 'apps/mobile/src/KaraokeText.tsx', 'apps/mobile/src/lyric-karaoke.ts']) {
  assert(productionInputs.has(source), `shared phone/tablet renderer excluded from production: ${source}`);
}
for (const marker of ['tablet-landscape-lyrics', 'lyric-word-fill', 'lyric-held-note-glow']) {
  assert(result.outputFiles[0].text.includes(marker), `approved mobile lyric UI missing from production: ${marker}`);
}
console.log('Production retains tablet landscape lyrics, continuous word fill and held-note glow in the shared mobile UI.');

for (const source of ['cardboard-config.ts', 'cardboard-renderer.ts', 'cardboard-pose.ts', 'cardboard-session.ts', 'use-cardboard-session.ts', 'CardboardControls.tsx']) {
  assert(productionInputs.has('apps/mobile/src/' + source), `shared VR source excluded from production: ${source}`);
}
for (const marker of ['cardboard-vr-controls', 'cardboard-calibration-panel']) assert(result.outputFiles[0].text.includes(marker), `VR UI missing from production: ${marker}`);
console.log('Production retains the same cardboard VR cameras, warp renderer, calibration controls and native sensor session used by the mobile preview.');
