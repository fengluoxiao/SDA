const assert = require('node:assert/strict');
const vm = require('node:vm');
const esbuild = require('esbuild');

(async () => {
  const output = [];
  const probe = { emittedDuringDecode: false };
  const build = await esbuild.build({
    entryPoints: ['packages/player/src/decoder.worker.ts'], bundle: true,
    platform: 'node', format: 'cjs', write: false,
    plugins: [{ name: 'decoder-fixture', setup(b) {
      b.onResolve({ filter: /^@sda\/(core|demux)$/ }, a => ({ path: a.path, namespace: 'fixture' }));
      b.onLoad({ filter: /.*/, namespace: 'fixture' }, a => ({ contents: a.path.endsWith('core') ? `
        export async function initCore() {}
        export class SdaDecoder {
          frames = []; free() {} drainErrors() { return []; }
          push(frame) { this.frames.push(frame); }
          nextFrame() { return this.frames.shift(); }
        }
      ` : `
        export function sniffContainer() { return 'mp4'; }
        export function createDemuxer(kind, cb) {
          return { flush() {}, push() {
            for (let i = 0; i < 2; i++) {
              cb.onPacket({ frames: [{ codec: 'eac3', sampleRate: 48000, samplePos: i * 1536,
                channels: [new Float32Array(1536)], labels: ['Obj_10'], rawBedLabels: [],
                objectChannels: i ? [] : [{ id: 10, channel: 0 }],
                events: [{ id: 10, samplePos: i * 1536, gainDb: -i, pos: [i, 0, 0], size: [0, 0, 0], hasPos: true }], programLoudness: null }] });
              if (!i) globalThis.probe.emittedDuringDecode = globalThis.output.some(x => x.type === 'frame');
            }
          } };
        }
      ` }));
    } }],
  });
  const self = { postMessage: message => output.push(message) };
  vm.runInNewContext(build.outputFiles[0].text, { self, output, probe, module: { exports: {} }, exports: {}, require, console, Float32Array });
  self.onmessage({ data: { type: 'open', codec: 'eac3', outputSampleRate: 48000 } });
  self.onmessage({ data: { type: 'push', chunk: new ArrayBuffer(1), sequence: 7 } });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(output.find(x => x.type === 'error'), undefined);
  assert.equal(probe.emittedDuringDecode, true, 'first PCM must leave before the next AU is decoded');
  const frames = output.filter(x => x.type === 'frame').map(x => x.frame);
  assert.deepEqual(frames.map(f => f.samplePos), [0, 1536]);
  assert.equal(frames[0].objectChannels[0].id, 10);
  assert.equal(frames[1].events[0].samplePos, 1536);
  assert.equal(output.at(-1).type, 'push-ack');
  assert.equal(output.at(-1).sequence, 7);
  console.log('Worker streams native-rate PCM during decode and preserves object clocks and ACK order');
})().catch(error => { console.error(error); process.exitCode = 1; });
