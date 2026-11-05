import {build} from 'esbuild';
import {fileURLToPath} from 'node:url';
const root=fileURLToPath(new URL('..',import.meta.url));
await build({entryPoints:[root+'/packages/demux/src/bwf.ts'],outfile:root+'/apps/desktop/bwf-metadata.cjs',bundle:true,platform:'node',format:'cjs',target:'node24',logLevel:'warning'});
