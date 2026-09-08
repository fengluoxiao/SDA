import assert from 'node:assert/strict';
import { BwfDemuxer, readBwfMetadata } from '../src/bwf.ts';
import { parseAdmMetadata, parseAdmMetadataStream } from '../src/adm.ts';

const xml = `<ebuCoreMain><coreMetadata><format><audioFormatExtended>
<audioProgramme audioProgrammeID="APR_1001" audioProgrammeName="Fixture" start="00:59:58.00000" end="00:59:59.00000"><audioContentIDRef>ACO_1001</audioContentIDRef></audioProgramme>
<audioContent audioContentID="ACO_1001"><audioObjectIDRef>AO_1001</audioObjectIDRef></audioContent>
<audioObject audioObjectID="AO_1001" start="00:00:00.00000" duration="00:00:01.00000"><audioTrackUIDRef>ATU_00000001</audioTrackUIDRef></audioObject>
<audioChannelFormat audioChannelFormatID="AC_00031001" typeDefinition="Objects">
<audioBlockFormat rtime="00:00:00.00000" duration="00:00:00.50000"><cartesian>1</cartesian><position coordinate="X">-1</position><position coordinate="Y">1</position><diffuse>1.000000</diffuse><jumpPosition>1</jumpPosition></audioBlockFormat>
<audioBlockFormat rtime="00:00:00.50000" duration="00:00:00.50000"><cartesian>1</cartesian><position coordinate="X">1</position><position coordinate="Y">-1</position><jumpPosition interpolationLength="0.005">1</jumpPosition></audioBlockFormat>
</audioChannelFormat>
<audioTrackUID UID="ATU_00000001"><audioChannelFormatIDRef>AC_00031001</audioChannelFormatIDRef><audioPackFormatIDRef>AP_00031001</audioPackFormatIDRef></audioTrackUID>
</audioFormatExtended></format></coreMetadata></ebuCoreMain>`;
const chna = Buffer.alloc(44);
chna.writeUInt16LE(1, 0); chna.writeUInt16LE(1, 2); chna.writeUInt16LE(1, 4);
chna.write('ATU_00000001', 6); chna.write('AC_00031001', 18); chna.write('AP_00031001', 32);
const format = Buffer.alloc(16);
format.writeUInt16LE(1, 0); format.writeUInt16LE(1, 2); format.writeUInt32LE(48000, 4);
format.writeUInt32LE(144000, 8); format.writeUInt16LE(3, 12); format.writeUInt16LE(24, 14);
const pcm = Buffer.from([0, 0, 128, 255, 255, 127, 0, 0, 0, 0, 0, 64]);
function chunk(id, data) {
  const header = Buffer.alloc(8); header.write(id); header.writeUInt32LE(data.length, 4);
  return Buffer.concat([header, data, Buffer.alloc(data.length % 2)]);
}
function wave(parts) {
  const body = Buffer.concat(parts), header = Buffer.alloc(12);
  header.write('RIFF'); header.writeUInt32LE(body.length + 4, 4); header.write('WAVE', 8);
  return Buffer.concat([header, body]);
}
for (const after of [false, true]) {
  const metadata = [chunk('axml', Buffer.from(xml)), chunk('chna', chna)];
  const file = wave([chunk('JUNK', Buffer.from('odd')), chunk('fmt ', format),
    ...(!after ? metadata : []), chunk('data', pcm), ...(after ? metadata : [])]);
  const reads = [];
  const info = await readBwfMetadata(async (offset, length) => { reads.push([offset, length]); return file.subarray(offset, offset + length); }, file.length);
  assert.equal(info.adm.objectChannels.length, 1);
  assert.equal(info.adm.events[0].samplePos, 0);
  assert.equal(info.adm.events[0].diffuse, 1);
  assert.equal(info.adm.events[1].samplePos, 24000);
  assert.equal(info.adm.events[1].rampDuration, 240);
  assert.ok(!reads.some(([offset]) => offset === info.dataOffset), 'preflight must skip PCM');
  const frames = [], tracks = [];
  const demux = new BwfDemuxer({ onPcmFrame: frame => frames.push(frame), onTrack: track => tracks.push(track) }, info);
  for (let offset = 0; offset < file.length; offset += 7) demux.push(file.subarray(offset, offset + 7));
  demux.flush();
  assert.deepEqual(frames.flatMap(frame => [...frame.channels[0]]), [-1, 8388607 / 8388608, 0, 0.5]);
  assert.equal(tracks[0].codec, 'adm');
  assert.equal(frames.reduce((sum, frame) => sum + frame.events.length, 0), 1);
  const truncated = new BwfDemuxer({}, info);
  truncated.push(file.subarray(0, info.dataOffset + 2));
  assert.throws(() => truncated.flush(), /truncated PCM/);
}

// Sparse RF64/BW64 fixture: metadata after a >4 GiB data chunk, no huge allocation.
for (const tag of ['RF64', 'BW64']) {
  const dataSize = 3 * 1500000000, dataOffset = 80;
  const tail = Buffer.concat([chunk('axml', Buffer.from(xml)), chunk('chna', chna)]);
  const fileSize = dataOffset + dataSize + tail.length;
  const header = Buffer.alloc(dataOffset);
  header.write(tag); header.writeUInt32LE(0xffffffff, 4); header.write('WAVE', 8);
  header.write('ds64', 12); header.writeUInt32LE(28, 16); header.writeBigUInt64LE(BigInt(fileSize - 8), 20);
  header.writeBigUInt64LE(BigInt(dataSize), 28); header.writeBigUInt64LE(1500000000n, 36);
  chunk('fmt ', format).copy(header, 48); header.write('data', 72); header.writeUInt32LE(0xffffffff, 76);
  let totalRead = 0;
  const info = await readBwfMetadata(async (offset, length) => {
    totalRead += length;
    if (offset < header.length) return header.subarray(offset, offset + length);
    assert.ok(offset >= dataOffset + dataSize, 'must seek across large PCM');
    return tail.subarray(offset - dataOffset - dataSize, offset - dataOffset - dataSize + length);
  }, fileSize);
  assert.equal(info.dataSize, dataSize); assert.equal(info.dataOffset, dataOffset);
  assert.ok(totalRead < 5000);
}
assert.throws(() => parseAdmMetadata(xml.replace('<diffuse>1.000000</diffuse>', '<diffuse>2</diffuse>'), chna, 48000, 1), /diffuse/);
// BBC-style exports leave TrackUID references in CHNA and use boolean diffuse.
const legacyXml = xml.replace(/<audioTrackUID UID="ATU_00000001">.*?<\/audioTrackUID>/,
  `<audioTrackUID UID="ATU_00000001"/>
<audioTrackFormat audioTrackFormatID="AT_00011001_01"><audioStreamFormatIDRef>AS_00011001</audioStreamFormatIDRef></audioTrackFormat>
<audioStreamFormat audioStreamFormatID="AS_00011001"><audioChannelFormatIDRef>AC_00031001</audioChannelFormatIDRef></audioStreamFormat>`);
const legacyChna = Buffer.from(chna);
const gapXml = xml.replace('duration="00:00:00.50000"', 'duration="00:00:00.25000"');
const gapEvents = parseAdmMetadata(gapXml, chna, 48000, 1).events;
const originalBlocks = xml.match(/<audioBlockFormat\b[\s\S]*?<\/audioBlockFormat>/g);
assert.deepEqual(parseAdmMetadata(xml.replace(originalBlocks.join('\n'), [...originalBlocks].reverse().join('\n')), chna, 48000, 1), parseAdmMetadata(xml, chna, 48000, 1));
assert.equal(gapEvents[1].samplePos, 12000);
assert.equal(gapEvents[1].gainDb, -200);
assert.equal(gapEvents[2].samplePos, 24000);
assert.equal(gapEvents[2].rampDuration, 0);
assert.throws(() => parseAdmMetadata(xml.replace('duration="00:00:00.50000"', 'duration="00:00:00.60000"'), chna, 48000, 1), /overlap/);
assert.equal(parseAdmMetadata(xml.replace('duration="00:00:00.50000"', 'duration="00:00:00.50001"'), chna, 48000, 1).events.length, 3);
const recovery = { overlapPolicy: 'latest-start' };
const interpolationRecovery = { interpolationPolicy: 'clamp' };
const longInterpolationXml = xml.replace('<jumpPosition>1</jumpPosition>', '<jumpPosition interpolationLength="1">1</jumpPosition>')
  .replace('interpolationLength="0.005"', 'interpolationLength="1"');
assert.throws(() => parseAdmMetadata(longInterpolationXml, chna, 48000, 1), /interpolationLength/);
const clamped = parseAdmMetadata(longInterpolationXml, chna, 48000, 1, interpolationRecovery);
assert.equal(clamped.events[0].rampDuration, 0);
assert.equal(clamped.events[1].rampDuration, 24000);
assert.equal(clamped.warnings.length, 1);
assert.match(clamped.warnings[0], /in 2 blocks/);
assert.deepEqual(parseAdmMetadata(xml, chna, 48000, 1, interpolationRecovery), parseAdmMetadata(xml, chna, 48000, 1));
for (const invalid of ['-1', 'NaN', 'Infinity']) {
  assert.throws(() => parseAdmMetadata(xml.replace('interpolationLength="0.005"', `interpolationLength="${invalid}"`), chna, 48000, 1, interpolationRecovery), /interpolationLength/);
}
const recoveredGap = parseAdmMetadata(longInterpolationXml.replace('duration="00:00:00.50000"', 'duration="00:00:00.25000"'), chna, 48000, 1, interpolationRecovery);
assert.equal(recoveredGap.events[2].rampDuration, 0);
const interpolationWave = wave([chunk('fmt ', format), chunk('data', pcm), chunk('axml', Buffer.from(longInterpolationXml)), chunk('chna', chna)]);
const interpolationInfo = await readBwfMetadata(async (offset, length) => interpolationWave.subarray(offset, offset + length), interpolationWave.length);
assert.deepEqual(interpolationInfo.adm, clamped);
const overlapXml = xml.replace('duration="00:00:00.50000"', 'duration="00:00:02.00000"');
const recovered = parseAdmMetadata(overlapXml, chna, 48000, 1, recovery);
assert.equal(recovered.warnings.length, 1);
assert.match(recovered.warnings[0], /truncated at 0.5s/);
assert.deepEqual(recovered.events.map(event => [event.samplePos, event.gainDb]), [[0, 0], [24000, 0], [48000, -200]]);
assert.equal(recovered.events[1].rampDuration, 240);
const nestedXml = overlapXml.replace('rtime="00:00:00.50000" duration="00:00:00.50000"', 'rtime="00:00:00.50000" duration="00:00:00.10000"');
assert.equal(parseAdmMetadata(nestedXml, chna, 48000, 1, recovery).events.at(-1).samplePos, 28800, 'superseded block must not resume');
assert.throws(() => parseAdmMetadata(xml.replace('rtime="00:00:00.50000"', 'rtime="00:00:00.00000"'), chna, 48000, 1, recovery), /duplicate block start/);
assert.throws(() => parseAdmMetadata(xml.replace('duration="00:00:00.50000"', 'duration="00:00:00.00000"'), chna, 48000, 1, recovery), /invalid duration/);
const recoveredFile = wave([chunk('fmt ', format), chunk('data', pcm), chunk('axml', Buffer.from(overlapXml)), chunk('chna', chna)]);
const recoveredInfo = await readBwfMetadata(async (offset, length) => recoveredFile.subarray(offset, offset + length), recoveredFile.length);
assert.deepEqual(recoveredInfo.adm.warnings, recovered.warnings);
legacyChna.fill(0, 18, 32); legacyChna.write('AT_00011001_01', 18);
for (const [text, expected] of [['false', 0], ['true', 1], ['0.25', 0.25]]) {
  const parsed = parseAdmMetadata(legacyXml.replace('1.000000', text), legacyChna, 48000, 1);
  assert.equal(parsed.events[0].diffuse, expected);
  assert.deepEqual(parsed.objectChannels, [{ id: 0, channel: 0 }]);
  assert.deepEqual(parsed.events[0].pos, [-1, 1, 0]);
}
assert.equal(parseAdmMetadata(legacyXml, chna, 48000, 1).objectChannels.length, 1);
assert.throws(() => parseAdmMetadata(xml, legacyChna, 48000, 1), /track reference mismatch/);
const missingRef = Buffer.from(chna); missingRef.fill(0, 18, 32);
assert.throws(() => parseAdmMetadata(legacyXml, missingRef, 48000, 1), /missing channel reference/);
assert.throws(() => parseAdmMetadata(xml.replace('<audioChannelFormatIDRef>AC_00031001</audioChannelFormatIDRef>',
  '<audioChannelFormatIDRef>AC_00031001</audioChannelFormatIDRef><audioTrackFormatIDRef>AT_00011001_01</audioTrackFormatIDRef>'), chna, 48000, 1), /ambiguous/);
console.log('ADM/BWF: reordered metadata, fragmented PCM, signed 24-bit, timecode, truncation, sparse RF64/BW64 passed');

const zonesXml = xml.replace('<diffuse>1.000000</diffuse>', `<zoneExclusion>
<zone minX="-1" maxX="0" minY="-1" maxY="1" minZ="-1" maxZ="1"/>
<zone minAzimuth="150" maxAzimuth="-150" minElevation="-20" maxElevation="90"/>
</zoneExclusion>`);
const zoneMetadata = parseAdmMetadata(zonesXml, chna, 48000, 1);
assert.deepEqual(zoneMetadata.events[0].zoneExclusion, [
  {type:'cartesian', min:[-1,-1,-1], max:[0,1,1]}, {type:'polar', min:[150,-20], max:[-150,90]},
]);
assert.deepEqual(zoneMetadata.events[1].zoneExclusion, [], 'later block can clear exclusions');
assert.throws(() => parseAdmMetadata(zonesXml.replace('maxX="0"', 'maxX="-2"'), chna, 48000, 1), /bounds/);
assert.throws(() => parseAdmMetadata(zonesXml.replace('maxX="0"', ''), chna, 48000, 1), /incomplete/);
assert.throws(() => parseAdmMetadata(zonesXml.replace('maxX="0"', 'maxX="0" minAzimuth="0"'), chna, 48000, 1), /mixed/);
const bedsXml = `<audioFormatExtended>
<audioChannelFormat audioChannelFormatID="AC_00011001" typeDefinition="DirectSpeakers"><audioBlockFormat><speakerLabel>M+030</speakerLabel></audioBlockFormat></audioChannelFormat>
<audioTrackUID UID="ATU_00000001"><audioChannelFormatIDRef>AC_00011001</audioChannelFormatIDRef></audioTrackUID>
<audioTrackUID UID="ATU_00000002"><audioChannelFormatIDRef>AC_00011001</audioChannelFormatIDRef></audioTrackUID>
</audioFormatExtended>`;
const bedsChna = Buffer.alloc(84); bedsChna.writeUInt16LE(2,0); bedsChna.writeUInt16LE(2,2);
for(let i=0;i<2;i++) { bedsChna.writeUInt16LE(i+1,4+i*40); bedsChna.write(`ATU_0000000${i+1}`,6+i*40); bedsChna.write('AC_00011001',18+i*40); }
const beds = parseAdmMetadata(bedsXml,bedsChna,48000,2);
assert.deepEqual(beds.labels,['L','L']); assert.deepEqual(beds.rawBedLabels,['L']);
const bedFormat=Buffer.from(format);bedFormat.writeUInt16LE(2,2);bedFormat.writeUInt32LE(288000,8);bedFormat.writeUInt16LE(6,12);
const bedFile=wave([chunk('fmt ',bedFormat),chunk('axml',Buffer.from(bedsXml)),chunk('chna',bedsChna),chunk('data',pcm)]);
const bedInfo=await readBwfMetadata(async(o,n)=>bedFile.subarray(o,o+n),bedFile.length);
const bedFrames=[];const bedDemux=new BwfDemuxer({onPcmFrame:frame=>bedFrames.push(frame)},bedInfo);
bedDemux.push(bedFile);bedDemux.flush();
assert.deepEqual([...bedFrames[0].channels[0]],[-1,0]);
assert.deepEqual([...bedFrames[0].channels[1]],[8388607/8388608,0.5]);

const streamSource = (input, step = 7) => async function* () {
  const bytes = typeof input === 'string' ? Buffer.from(input) : input;
  for (let offset = 0; offset < bytes.length; offset += step) yield bytes.subarray(offset, offset + step);
};
const compatibility = { ...recovery, ...interpolationRecovery };
const namespacedXml = xml.replace(/<(\/?)([A-Za-z][\w]*)/g, '<$1adm:$2')
  .replace('<adm:ebuCoreMain>', '<adm:ebuCoreMain xmlns:adm="urn:ebu:test">')
  .replace('audioProgrammeName="Fixture"', 'audioProgrammeName="&#x6B4C;&#x66F2; &amp; test"')
  .replace('<adm:diffuse>1.000000</adm:diffuse>', '<adm:diffuse><![CDATA[1.000000]]></adm:diffuse>');
const offsetXml = xml.replace('<audioTrackUIDRef>ATU_00000001</audioTrackUIDRef>', '<positionOffset coordinate="X">0</positionOffset><gain gainUnit="dB">-3</gain><audioTrackUIDRef>ATU_00000001</audioTrackUIDRef>');
for (const input of [xml, gapXml, overlapXml, nestedXml, zonesXml, longInterpolationXml, namespacedXml, offsetXml,
  xml.replace(originalBlocks.join('\n'), [...originalBlocks].reverse().join('\n')),
  xml.replace('Fixture', '\u6b4c\u66f2') + '\0\0']) {
  for (const step of [1, 97]) assert.deepEqual(
    await parseAdmMetadataStream(streamSource(input, step), chna, 48000, 1, compatibility),
    parseAdmMetadata(input, chna, 48000, 1, compatibility),
  );
}
assert.deepEqual(await parseAdmMetadataStream(streamSource(legacyXml), legacyChna, 48000, 1), parseAdmMetadata(legacyXml, legacyChna, 48000, 1));
assert.deepEqual(await parseAdmMetadataStream(streamSource(bedsXml), bedsChna, 48000, 2), beds);
for (const [input, error] of [
  [xml.slice(0, -5), /malformed XML/],
  [xml.replace('</position>', '</wrong>'), /malformed XML/],
  [xml.replace('Fixture', '&unknown;'), /malformed XML/],
  ['<!DOCTYPE ebuCoreMain [<!ENTITY x "value">]>' + xml, /document types/],
  [xml + '\0x', /malformed XML|padding/],
  [xml.replace('rtime="00:00:00.50000"', 'rtime="00:00:00.00000"'), /duplicate block start/],
  [xml.replace('interpolationLength="0.005"', 'interpolationLength="-1"'), /interpolationLength/],
  [xml.replace('AC_00031001</audioChannelFormatIDRef>', 'AC_00031002</audioChannelFormatIDRef>'), /mismatch/],
  [xml.replace('Fixture', 'x'.repeat(1024 * 1024 + 65536)), /resource budget/],
]) await assert.rejects(() => parseAdmMetadataStream(streamSource(input, 65536), chna, 48000, 1, compatibility), error);
await assert.rejects(() => parseAdmMetadataStream(streamSource(Buffer.from([0xc3, 0x28]), 1), chna, 48000, 1), /encoded data/);

// Virtual 70 MiB AXML: exercise bounded reads above the former limit without a huge allocation.
const comment = Buffer.from('<!--' + 'x'.repeat(65529) + '-->');
const fillerSize = comment.length * 1120;
const xmlPayloadSize = fillerSize + Buffer.byteLength(xml);
const axmlHeader = Buffer.alloc(8); axmlHeader.write('axml'); axmlHeader.writeUInt32LE(xmlPayloadSize, 4);
const largePrefix = wave([chunk('fmt ', format), chunk('data', pcm), chunk('chna', chna)]);
const prefix = Buffer.concat([largePrefix, axmlHeader]);
const suffix = Buffer.concat([Buffer.from(xml), Buffer.alloc(xmlPayloadSize % 2)]);
const virtualSize = prefix.length + fillerSize + suffix.length;
prefix.writeUInt32LE(virtualSize - 8, 4);
const largeInfo = await readBwfMetadata(async (offset, length) => {
  assert.ok(length <= 65536, 'XML must be read in bounded chunks');
  const bytes = Buffer.alloc(length);
  for (let i = 0; i < length; i++) {
    const position = offset + i;
    bytes[i] = position < prefix.length ? prefix[position]
      : position < prefix.length + fillerSize ? comment[(position - prefix.length) % comment.length]
        : suffix[position - prefix.length - fillerSize];
  }
  return bytes;
}, virtualSize);
assert.deepEqual(largeInfo.adm, parseAdmMetadata(xml, chna, 48000, 1));
console.log('ADM streaming: DOM parity, fragmented UTF-8, namespaces, invalid XML and 70 MiB bounded reads passed');
