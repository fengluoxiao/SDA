import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import vm from 'node:vm';
const root = path.resolve('apps/mobile');
const require = createRequire(path.join(root, 'metro.config.js'));
const module = { exports: {} };
const wrappedRequire = name => name === 'expo/metro-config'
  ? { getDefaultConfig: () => ({ resolver: {} }) } : require(name);
wrappedRequire.resolve = require.resolve;
vm.runInNewContext(readFileSync(path.join(root, 'metro.config.js'), 'utf8'), {
  require: wrappedRequire, module, __dirname: root,
});
const resolve = module.exports.resolver.resolveRequest;
const renderer = path.resolve('packages/renderer/src');
const fallback = { type: 'sourceFile', filePath: 'fallback.js' };
let fallbackCalls = 0;
const context = {
  originModulePath: path.join(renderer, 'head-pose.ts'),
  resolveRequest: () => { fallbackCalls++; return fallback; },
};
assert.equal(resolve(context, './coords.js', 'ios').filePath, path.join(renderer, 'coords.ts'));
assert.equal(fallbackCalls, 0);
assert.equal(resolve(context, './missing.js', 'ios'), fallback);
assert.equal(resolve({ ...context, originModulePath: path.join(root, 'App.tsx') }, './coords.js', 'ios'), fallback);
assert.equal(resolve(context, '../../outside.js', 'ios'), fallback);
console.log('Metro shared-source resolver: .js-to-.ts mapping, missing file and boundary fallback passed.');
