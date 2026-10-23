import assert from 'node:assert/strict';
import {build} from 'esbuild';
const result=await build({entryPoints:['apps/mobile/index.js'],bundle:true,platform:'node',format:'cjs',packages:'external',external:['*.png','*.jpg','*.jpeg','*.webp','*.ttf'],define:{__DEV__:'false','process.env.NODE_ENV':'"production"'},minifySyntax:true,metafile:true,write:false});
assert(!Object.keys(result.metafile.inputs).some(name=>name.includes('dev-preview/')),'development preview fixtures entered the production bundle');
assert(!result.outputFiles[0].text.includes('mock://'),'mock media entered the production bundle');
console.log('Production JavaScript bundle excludes development preview fixtures and mock media.');
