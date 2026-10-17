import { build } from 'esbuild';

// Test the real shared session while the playback fixtures stub native UI/IO.
const bundle = await build({ entryPoints: ['apps/web/src/head-tracking-session.ts'],
  bundle: true, platform: 'node', format: 'cjs', write: false });
const module = { exports: {} };
new Function('module', 'exports', bundle.outputFiles[0].text)(module, module.exports);
export const trackingModule = module.exports;
