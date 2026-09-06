#!/usr/bin/env node
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { parseWav } from './lib/hrtf-source.mjs';
import profiles from '../apps/desktop/cinema-profiles.cjs';

const [input, output] = process.argv.slice(2);
if (!input || !output) throw new Error('Usage: node scripts/prepare-room-profile.mjs measurements.json room-profile.json');
const measurement = JSON.parse(readFileSync(input, 'utf8'));
const speakers = measurement.speakers.map(entry => {
  const {left,right,sampleRate,channels,format,bitsPerSample} = parseWav(readFileSync(resolve(dirname(input), entry.file)));
  if (sampleRate !== 48000 || channels !== 2 || !((format===1 && [16,24,32].includes(bitsPerSample)) || (format===3 && bitsPerSample===32))) throw new Error('Requires stereo 48 kHz PCM or float32 WAV');
  if (left.length < 512 || left.length > 32768) throw new Error('Responses must be 512..32768 taps; choose an intentional IR window before import');
  if (![...left,...right].every(Number.isFinite)) throw new Error('Non-finite samples');
  const onset = values => {
    const peak = values.reduce((a,b)=>Math.max(a,Math.abs(b)),0);
    if (peak < 1e-9) throw new Error('Silent ear response');
    return values.findIndex(v=>Math.abs(v)>=peak*0.1);
  };
  const onsetSample = Math.min(onset(left),onset(right));
  const end = Math.max(onset(left),onset(right)) + 192;
  // Use one common 4 ms direct window, preserving ear-to-ear delay. The 1 ms
  // fade is a time window, not a claimed physical separation of reflections.
  const direct = values => Array.from(values, (v,i) => v * Math.max(0,Math.min(1,(end-i)/48)));
  return {name:entry.name,azimuth:entry.azimuth,elevation:entry.elevation,onsetSample,
    directLeft:direct(left),directRight:direct(right),roomLeft:Array.from(left),roomRight:Array.from(right)};
});
const profile = profiles.validateRoom({version:1,name:measurement.name,source:measurement.source,license:measurement.license,
  measurement:measurement.measurement,sampleRate:48000,layout:measurement.layout,speakers});
writeFileSync(output, JSON.stringify(profile));
writeFileSync(`${output}.report.json`, JSON.stringify(profiles.analyzeRoom(profile),null,2));
console.log(`Prepared ${profile.layout} measured room: ${profile.name}`);
