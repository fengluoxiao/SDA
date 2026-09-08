import assert from 'node:assert/strict';
import { BwfDemuxer, readBwfMetadata } from '../src/bwf.ts';
import { parseAdmMetadata } from '../src/adm.ts';

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
console.log('ADM/BWF: reordered metadata, fragmented PCM, signed 24-bit, timecode, truncation, sparse RF64/BW64 passed');
