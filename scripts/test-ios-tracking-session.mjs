import { build } from 'esbuild';
import { mkdirSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
mkdirSync('tmp/head-tracking-ci', { recursive: true });
await build({
  entryPoints: ['apps/web/test/head-tracking-session.test.ts'],
  bundle: true, platform: 'node', format: 'cjs',
  alias: { '@sda/renderer': './packages/renderer/src/head-pose.ts' },
  outfile: 'tmp/head-tracking-ci/session-test.cjs',
});
execFileSync(process.execPath, ['tmp/head-tracking-ci/session-test.cjs'], { stdio: 'inherit' });
