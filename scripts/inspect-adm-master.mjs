import { open, mkdir, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';

const input = resolve(process.argv[2] ?? 'tmp/dolby-natures-fury/Exercise_Content_2-3/NaturesFuryADM.wav');
const output = resolve(process.argv[3] ?? 'tmp/dolby-natures-fury/inspection');
const file = await open(input, 'r');
try {
  const { size } = await file.stat();
  async function read(offset, length) {
    const buffer = Buffer.alloc(length);
    const { bytesRead } = await file.read(buffer, 0, length, offset);
    if (bytesRead !== length) throw new Error(`Truncated read at ${offset}`);
    return buffer;
  }
  const header = await read(0, 12);
  const container = header.toString('ascii', 0, 4);
  if (!['RIFF', 'RF64', 'BW64'].includes(container) || header.toString('ascii', 8) !== 'WAVE') throw new Error('Not a WAVE master');
  await mkdir(output, { recursive: true });
  const report = { input, bytes: size, container, chunks: [] };
  let largeDataSize;
  for (let offset = 12; offset + 8 <= size;) {
    const chunk = await read(offset, 8);
    const id = chunk.toString('ascii', 0, 4);
    let length = chunk.readUInt32LE(4);
    if (length === 0xffffffff && id === 'data') length = largeDataSize;
    if (!Number.isSafeInteger(length) || offset + 8 + length > size) throw new Error(`Invalid ${id} chunk size`);
    report.chunks.push({ id, offset, bytes: length });
    if (id === 'ds64') {
      const bytes = await read(offset + 8, Math.min(length, 28));
      largeDataSize = Number(bytes.readBigUInt64LE(8));
    }
    if (id === 'fmt ') {
      const bytes = await read(offset + 8, length);
      report.pcm = { format: bytes.readUInt16LE(0), channels: bytes.readUInt16LE(2), sampleRate: bytes.readUInt32LE(4), blockAlign: bytes.readUInt16LE(12), bits: bytes.readUInt16LE(14) };
    }
    if (['axml', 'chna', 'dbmd'].includes(id)) {
      if (length > 128 * 1024 * 1024) throw new Error('Metadata is unexpectedly large');
      await writeFile(join(output, id === 'axml' ? 'axml.xml' : `${id}.bin`), await read(offset + 8, length));
    }
    offset += 8 + length + (length % 2);
  }
  const data = report.chunks.find(chunk => chunk.id === 'data');
  if (data && report.pcm) report.durationSeconds = data.bytes / report.pcm.blockAlign / report.pcm.sampleRate;
  await writeFile(join(output, 'container.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report, null, 2));
} finally {
  await file.close();
}
