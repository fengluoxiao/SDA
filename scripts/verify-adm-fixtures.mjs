import { build } from 'esbuild';
import { open, mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import assert from 'node:assert/strict';

const root = resolve('tmp/adm-regression');
await mkdir(root, { recursive: true });
const bundle = resolve(root, 'fixture-bwf.mjs');
await build({ entryPoints: ['packages/demux/src/bwf.ts'], outfile: bundle, bundle: true, platform: 'node', format: 'esm' });
const { readBwfMetadata } = await import(pathToFileURL(bundle));
const paths = process.argv.slice(2);
if (!paths.length) paths.push(
  'tmp/dolby-natures-fury/Exercise_Content_2-3/NaturesFuryADM.wav',
  'tmp/machine_aer_bwf.wav', 'tmp/flower_duet_bwf.wav',
  'tmp/netflix-adm/SolLevante_ADM.wav', 'tmp/netflix-adm/Nocture_ADM.wav',
  'tmp/netflix-adm/Meridian_ADMFromDAMF_JAN2021.wav',
);
const reports = [];
for (const path of paths) {
  const file = await open(path, 'r');
  try {
    let bytesRead = 0;
    const metadata = await readBwfMetadata(async (offset, length) => {
      assert.ok(length <= 1024 * 1024);
      const buffer = Buffer.alloc(length);
      const result = await file.read(buffer, 0, length, offset);
      bytesRead += result.bytesRead;
      return buffer.subarray(0, result.bytesRead);
    }, (await file.stat()).size);
    assert.equal(metadata.labels.length, metadata.format.channels);
    const objects = new Set(metadata.adm.objectChannels.map(entry => entry.id));
    assert.equal(objects.size, metadata.adm.objectChannels.length);
    assert.ok(metadata.adm.events.every(event => objects.has(event.id) && event.pos.every(Number.isFinite)));
    const report = { path, channels: metadata.format.channels, objects: objects.size,
      bedTracks: metadata.labels.filter(label => !label.startsWith('Obj_')).length,
      bedLabels: metadata.adm.rawBedLabels, events: metadata.adm.events.length,
      zonedEvents: metadata.adm.events.filter(event => event.zoneExclusion?.length).length,
      warnings: metadata.adm.warnings?.length ?? 0, metadataBytesRead: bytesRead };
    reports.push(report); console.log(JSON.stringify(report));
  } catch (error) { reports.push({ path, error: error.message }); console.error(path, error.message); process.exitCode = 1; }
  finally { await file.close(); }
}
await writeFile(resolve(root, 'masters.json'), JSON.stringify(reports, null, 2));
