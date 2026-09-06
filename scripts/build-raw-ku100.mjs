#!/usr/bin/env node
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { collectIrs } from './lib/hrtf-source.mjs';

const archive = resolve(process.argv[2] ?? 'tmp/sadie-source/D1.zip');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const archiveHash = hash(readFileSync(archive));
const baseline = JSON.parse(readFileSync('apps/web/public/hrtf/hrtf-set.json','utf8'));
if (archiveHash !== baseline.source.archiveSha256) throw new Error('KU100 source archive hash mismatch');
const collections = {};
for (const [kind,path] of [['dry',baseline.source.hrPath],['wet',baseline.source.brPath]]) {
  const collection = await collectIrs(archive,path);
  collections[kind] = new Map(collection.impulses.map(ir=>[ir.sourcePath,ir]));
}
for (const name of ['hrtf','hrtf-dense']) {
  const manifest = JSON.parse(readFileSync(`apps/web/public/${name}/hrtf-set.json`,'utf8'));
  const out = resolve(`apps/web/public/${name}-raw`);
  mkdirSync(out,{recursive:true});
  const positions = manifest.positions.map(entry=>{
    const assets = {};
    for (const kind of ['dry','wet']) {
      const ir = collections[kind].get(entry.measurement[kind].sourcePath);
      if (!ir || ir.sampleRate !== 48000) throw new Error(`Missing 48 kHz original: ${entry[kind]}`);
      const packed = new Float32Array(ir.left.length * 2);
      packed.set(ir.left);packed.set(ir.right,ir.left.length);
      const bytes = Buffer.from(packed.buffer);
      writeFileSync(resolve(out,entry[kind]),bytes);
      assets[kind]={tapCountPerEar:ir.left.length,sha256:hash(bytes)};
    }
    return {azimuth:entry.azimuth,elevation:entry.elevation,dry:entry.dry,wet:entry.wet,measurement:entry.measurement,assets};
  });
  writeFileSync(resolve(out,'hrtf-set.json'),JSON.stringify({
    schemaVersion:2,calibrationVersion:0,completeSubject:name==='hrtf',subjectId:'ku100',sampleRate:48000,
    source:baseline.source,azimuthConvention:baseline.azimuthConvention,
    processing:{calibrated:false,preserveMeasurements:true,peakNormalized:false,runtimeEnergyNormalization:false,
      note:'Original complete 48 kHz WAV samples packed as float32. No crop, arrival alignment, gain matching, symmetry, EQ, room gate or decorrelation. Direction mapping retained from the calibrated set.'},positions,
  },null,2)+'\n');
  console.log(`${name}-raw: ${positions.length} original measurement pairs`);
}
