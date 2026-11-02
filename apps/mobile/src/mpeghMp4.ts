import { Mp4Demuxer } from "../../../packages/demux/src/mp4";
import { mhasPacket } from "../../../packages/core/src/mhas";
import { fromByteArray, toByteArray } from "base64-js";

export interface MpeghMp4Host {
  mp4ImportChunkBytes?(): number;
  beginMp4Import(uri: string): Promise<string>;
  readMp4Import(token: string, offset: number, count: number): Promise<string>;
  appendMp4Import(token: string, bytes: string): Promise<void>;
  finishMp4Import(token: string): Promise<string>;
  discardMp4Import(token: string): Promise<void>;
  adoptMp4Import?(token: string): Promise<void>;
}
const MAX_CHUNK = 1024 * 1024;
export interface ImportProgress { fraction: number; stage: string }
/** Use Windows's actual MP4 parser and AV3A/AC-4/MPEG-H packet writers; the mobile host
 * only reads/writes bounded file chunks. Decoding remains entirely native. */
export async function prepare360RaMp4(host: MpeghMp4Host, uri: string, name: string, onProgress?: (progress: ImportProgress) => void) {
  if (!/\.(?:m4a|mp4)$/i.test(name)) return null;
  const requestedChunk=host.mp4ImportChunkBytes?.() ?? 256*1024;
  const CHUNK=Number.isSafeInteger(requestedChunk)?Math.max(64*1024,Math.min(MAX_CHUNK,requestedChunk)):256*1024;
  onProgress?.({ fraction: 0, stage: "读取容器" });
  const { token, size } = JSON.parse(await host.beginMp4Import(uri)) as { token: string; size: number };
  let selected = false, otherCodec = false, raw = false, ac4 = false, av3a = false, durationMs = 0, failure: string | undefined;
  let packetCount = 0;
  const packets: Uint8Array[] = [];
  const demuxer = new Mp4Demuxer({
    onTrack(track) {
      if (track.codec !== "mha1" && track.codec !== "mhm1" && track.codec !== "ac-4" && track.codec !== "av3a") { otherCodec = true; return; }
      selected = true;
      ac4 = track.codec === "ac-4";
      av3a = track.codec === "av3a";
      raw = track.codec === "mha1";
      durationMs = (track.durationSec ?? 0) * 1000;
      if (raw) {
        if (!track.decoderConfig?.length) throw new Error("360RA mha1 音轨缺少 mhaC 配置");
        packets.push(mhasPacket(1, track.decoderConfig));
      }
    },
    onPacket(packet) {
      if (!selected) return;
      packets.push(raw ? mhasPacket(2, packet.data) : packet.data);
      packetCount++;
    },
    onError(message) { failure = message; },
  });
  const writePackets = async () => {
    // MPEG-H AUs are small: one native call per AU stalls whole-song import.
    // Coalesce bytes without changing any packet boundaries in the MHAS stream.
    const batch = new Uint8Array(CHUNK);
    let used = 0;
    for (const packet of packets) for (let offset = 0; offset < packet.length;) {
      const count = Math.min(CHUNK - used, packet.length - offset);
      batch.set(packet.subarray(offset, offset + count), used);
      used += count; offset += count;
      if (used === CHUNK) {
        await host.appendMp4Import(token, fromByteArray(batch));
        used = 0;
      }
    }
    if (used) await host.appendMp4Import(token, fromByteArray(batch.subarray(0, used)));
    packets.length = 0;
    if (failure) throw new Error(`MP4 解析失败：${failure}`);
  };
  try {
    let offset=0,readBytes=0,lastProgress=0;let fraction=0,lastStage="";
    const visits=new Map<number,number>();
    while (offset < size && !demuxer.extractionComplete) {
      const visit=(visits.get(offset)??0)+1;visits.set(offset,visit);
      if(visit>3)throw new Error("MP4 提取位置没有推进");
      const bytes = toByteArray(await host.readMp4Import(token, offset, CHUNK));
      if (!bytes.length) throw new Error("MP4 文件读取提前结束");
      readBytes+=bytes.length;
      const next=demuxer.push(bytes, offset);
      if (otherCodec) { await host.discardMp4Import(token); return null; }
      await writePackets();
      const now=Date.now();fraction=Math.max(fraction,.95*Math.min(1,readBytes/size));
      const stage=av3a?"提取 Audio Vivid 音轨":ac4?"提取 AC-4 音轨":selected?"提取 360 Reality Audio 音轨":"解析音轨";
      if(now-lastProgress>=80||stage!==lastStage){onProgress?.({fraction,stage});lastProgress=now;lastStage=stage;}
      offset=next!==undefined&&next!==offset&&next<size?next:offset+bytes.length;
    }
    demuxer.flush();
    await writePackets();
    if (!selected || !packetCount) throw new Error("MP4 中没有可播放的 Atmos/360RA/Audio Vivid 音轨");
    const playbackUri = await host.finishMp4Import(token);
    onProgress?.({ fraction: 1, stage: "音轨已就绪" });
    return { uri: playbackUri, name: `${name}.${av3a ? "av3a" : ac4 ? "ac4" : "mhas"}`, durationMs,
      adopt: () => host.adoptMp4Import?.(token),
      release: () => host.discardMp4Import(token) };
  } catch (error) {
    await host.discardMp4Import(token);
    throw error;
  }
}
