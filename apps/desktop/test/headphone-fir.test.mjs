import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';

const main = fs.readFileSync(new URL('../main.cjs', import.meta.url), 'utf8');
const source = main.slice(main.indexOf('function nativeRendererHeadphoneFir('), main.indexOf('\nfunction ', main.indexOf('function nativeRendererHeadphoneFir(') + 1));

test('headphone FIR sends file bytes as f32 taps, including unaligned buffers', async () => {
  const packets = [];
  const pending = new Map();
  const context = vm.createContext({
    Buffer, Float32Array, Number, ArrayBuffer, Promise, setTimeout, clearTimeout,
    nativeRendererWritable: true,
    nativeRendererControlChain: Promise.resolve(),
    nativeRendererPendingCommands: pending,
    NATIVE_RENDERER_COMMAND_ACK_TIMEOUT_MS: 100,
    writeStartupLog() {},
    nativeRenderer: { stdin: { write(packet) {
      packets.push(packet);
      const ack = pending.get('setHeadphoneFir');
      clearTimeout(ack.timeout);
      pending.delete('setHeadphoneFir');
      ack.resolve(true);
      return true;
    } } },
  });
  vm.runInContext(source, context);
  const bytes = Buffer.alloc(13).subarray(1);
  [0.25, -0.5, 1].forEach((value, i) => bytes.writeFloatLE(value, i * 4));
  assert.equal(await context.nativeRendererHeadphoneFir(0.5, bytes, bytes), true);
  assert.equal(packets[0].readUInt32LE(5), 3);
  assert.deepEqual(packets[0].subarray(13, 25), bytes);
  assert.equal(await context.nativeRendererHeadphoneFir(1, new Float32Array([1, 0]), new Float32Array([1, 0])), true);
  assert.equal(await context.nativeRendererHeadphoneFir(1, Buffer.alloc(3), bytes), false);
  const invalid = Buffer.alloc(8);
  invalid.writeFloatLE(NaN);
  assert.equal(await context.nativeRendererHeadphoneFir(1, invalid, bytes), false);
});
