import { Mp4Demuxer } from "../../../packages/demux/src/mp4";
import { mhasPacket } from "../../../packages/core/src/mhas";
import { fromByteArray, toByteArray } from "base64-js";

export interface MpeghMp4Host {
  beginMp4Import(uri: string): Promise<string>;
  readMp4Import(token: string, offset: number, count: number): Promise<string>;
  appendMp4Import(token: string, bytes: string): Promise<void>;
  finishMp4Import(token: string): Promise<string>;
  discardMp4Import(token: string): Promise<void>;
}
const CHUNK = 256 * 1024;
/** Use Windows's actual MP4 parser and mhaC/AU packet writer; the Android host
 * only reads/writes bounded file chunks. Decoding remains entirely native. */
export async function prepare360RaMp4(host: MpeghMp4Host, uri: string, name: string) {
  if (!/\.(?:m4a|mp4)$/i.test(name)) return null;
  const { token, size } = JSON.parse(await host.beginMp4Import(uri)) as { token: string; size: number };
  let selected = false, otherCodec = false, raw = false, durationMs = 0, failure: string | undefined;
  let packetCount = 0;
  const packets: Uint8Array[] = [];
  const demuxer = new Mp4Demuxer({
    onTrack(track) {
      if (track.codec !== "mha1" && track.codec !== "mhm1") { otherCodec = true; return; }
      selected = true;
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
    for (let offset = 0; offset < size; offset += CHUNK) {
      const bytes = toByteArray(await host.readMp4Import(token, offset, CHUNK));
      if (!bytes.length) throw new Error("MP4 文件读取提前结束");
      demuxer.push(bytes, offset);
      if (otherCodec) { await host.discardMp4Import(token); return null; }
      await writePackets();
    }
    demuxer.flush();
    await writePackets();
    if (!selected || !packetCount) throw new Error("MP4 中没有可播放的 Atmos/360RA 音轨");
    const playbackUri = await host.finishMp4Import(token);
    return { uri: playbackUri, name: `${name}.mhas`, durationMs,
      release: () => host.discardMp4Import(token) };
  } catch (error) {
    await host.discardMp4Import(token);
    throw error;
  }
}
