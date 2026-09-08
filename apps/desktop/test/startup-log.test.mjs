import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

const main = readFileSync(new URL('../main.cjs', import.meta.url), 'utf8');
const timers = [];
const writes = [];
let finishWrite;
const context = {
  path: { join: (...parts) => parts.join('/'), dirname: () => '/tmp' },
  process: { cwd: () => '/workspace' },
  setTimeout: (callback) => { timers.push(callback); return timers.length; },
  fs: {
    mkdir: (_path, _options, callback) => callback(null),
    appendFile: (_path, text, _encoding, callback) => { writes.push(text); finishWrite = callback; },
  },
};
runInNewContext(main.slice(main.indexOf('const startupLogPath'), main.indexOf('function logRenderer')), context);
for (let i = 0; i < 100; i++) context.writeStartupLog(`message ${i}`);
assert.equal(writes.length, 0);
assert.equal(timers.length, 1);
timers.shift()();
assert.equal(writes.length, 1);
assert.equal(writes[0].trim().split('\n').length, 100);
context.writeStartupLog('during write');
assert.equal(timers.length, 0);
finishWrite(null);
timers.shift()();
assert.match(writes[1], /during write/);
context.writeStartupLog('after failed write');
finishWrite(new Error('disk unavailable'));
timers.shift()();
assert.match(writes[2], /after failed write/);
finishWrite(null);
console.log('Startup diagnostics batch asynchronously and recover after write errors');
