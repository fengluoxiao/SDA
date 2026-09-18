'use strict';
// Separate experimental receiver; no changes to the desktop file-player state.
const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { once } = require('node:events');
const { CaptureRecords } = require('./iec61937.cjs');
const { SystemDecoder } = require('./decoder.cjs');
const { NativeSink } = require('./native-sink.cjs');
const { SdaDecoder } = require('../../packages/core/pkg-node/sda_core.cjs');

async function main() {
  const options = {};
  for (const arg of process.argv.slice(2)) {
    const match = /^--(replay|output|seconds|volume|save)=(.+)$/.exec(arg);
    if (match) options[match[1]] = match[2];
    else if (arg === '--verify-only') options.verify = true;
    else throw Error('Usage: receive.cjs --output=<physical endpoint ID> [--seconds=60] [--save=capture.sdac] OR --replay=capture.sdac --verify-only');
  }
  const volume = Number(options.volume ?? 0.5), seconds = Number(options.seconds ?? 3600);
  if (!(volume >= 0 && volume <= 1) || !Number.isInteger(seconds) || seconds < 1 || seconds > 86400) throw Error('Invalid volume/duration');
  const root = path.resolve(__dirname, '../..');
  let sink, helper, helperDone, input, save, saveError, stopping = false;
  const stop = () => { stopping = true; input?.destroy(); };
  process.once('SIGINT', stop); process.once('SIGTERM', stop);
  const stats = { frames: 0, events: 0, objectFrames: 0, resets: 0, diagnostics: 0 };
  const steps = [];
  const decoder = new SystemDecoder({ createDecoder: c => new SdaDecoder(c),
    onReset(info) { stats.resets++; steps.push({ reset: true }); console.error(`epoch=${info.epoch} format=${info.kind} overflows=${info.overflows}`); },
    onFrame(frame) { stats.frames++; stats.events += frame.events.length; if (frame.labels.some(l => l.startsWith('Obj_'))) stats.objectFrames++; if (sink) steps.push({ frame }); },
    onDiagnostic(message) { stats.diagnostics++; if (stats.diagnostics <= 10) console.error(message); },
  });
  const records = new CaptureRecords(r => decoder.accept(r));
  try {
    if (!options.verify) { sink = new NativeSink({ root, outputDevice: options.output, volume }); await sink.initialize(); }
    if (options.save) {
      save = fs.createWriteStream(options.save, { flags: 'wx' });
      save.on('error', e => { saveError = e; input?.destroy(e); });
    }
    if (options.replay) input = fs.createReadStream(options.replay, { highWaterMark: 65536 });
    else {
      helper = spawn(path.join(root, 'tools/windows-audio-probe/target/debug/capture.exe'), [String(seconds)], { windowsHide: true, stdio: ['ignore', 'pipe', 'inherit'] });
      input = helper.stdout;
      helper.on('error', e => input.destroy(e));
      helperDone = new Promise(resolve => helper.once('close', resolve));
    }
    for await (const chunk of input) {
      if (saveError) throw saveError;
      if (save && !save.write(chunk)) await once(save, 'drain');
      records.push(chunk);
      for (const step of steps.splice(0)) {
        if (step.reset && sink) await sink.reset();
        if (step.frame) await sink.frame(step.frame);
      }
    }
    records.finish();
    if (helper && !stopping && (await helperDone) !== 0) throw Error('Capture failed; driver must be installed and the reader requires administrator access');
    if (!stopping) {
      decoder.finish();
      for (const step of steps.splice(0)) if (step.frame) await sink?.frame(step.frame);
      await sink?.drain();
    }
    if (save) { save.end(); await once(save, 'finish'); }
    if (saveError) throw saveError;
    if (options.verify && !stats.frames) throw Error('No decoded audio in capture; verification has not passed');
    console.log(JSON.stringify(stats));
  } finally {
    process.removeListener('SIGINT', stop); process.removeListener('SIGTERM', stop);
    decoder.close(); save?.destroy(); input?.destroy(); helper?.kill(); await sink?.close();
  }
}
main().catch(e => { console.error(e.message); process.exitCode = 1; });
