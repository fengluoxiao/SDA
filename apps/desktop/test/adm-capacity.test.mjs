import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

const main = readFileSync(new URL('../main.cjs', import.meta.url), 'utf8');
const start = main.indexOf('ipcMain.handle("sda:native-renderer-events"');
const end = main.indexOf('ipcMain.handle("sda:native-renderer-reset"', start);
let handler;
const packets = [];
runInNewContext(main.slice(start, end), {
  Buffer,
  ipcMain: { handle(_name, callback) { handler = callback; } },
  writeStartupLog() {},
  async nativeRendererCommandAck(packet) {
    assert.ok(Buffer.byteLength(JSON.stringify(packet)) <= 16384);
    packets.push(packet);
    return true;
  },
});
const events = Array.from({ length: 108 }, (_, id) => ({
  id, samplePos: 0, hasPos: true, pos: [-0.123456789, 0.987654321, 1],
  gainDb: 0, size: [0, 0, 0], diffuse: 1, horizontalOnly: false,
  anchor: 'room', distanceM: null, distanceInfinite: false,
  screenFactor: null, depthFactor: null, rampDuration: 0,
}));
assert.equal(await handler(null, events), true);
assert.equal(packets.length, 4);
assert.deepEqual(packets.flatMap(packet => packet.events), events);
packets.length = 0;
const complex = events.map(event => ({ ...event, zoneExclusion: Array.from({ length: 20 }, () => ({ type: 'cartesian', min: [-1, -1, -1], max: [0.4, 0.6, 0.8] })) }));
assert.equal(await handler(null, complex), true);
assert.ok(packets.length > 4, 'large metadata uses byte-bounded packets');
assert.deepEqual(packets.flatMap(packet => packet.events), complex);
assert.equal(await handler(null, [{ ...events[0], zoneExclusion: Array(1000).fill(complex[0].zoneExclusion[0]) }]), false);
console.log('Desktop ADM events preserve all 108 objects within the 16 KiB protocol limit');
