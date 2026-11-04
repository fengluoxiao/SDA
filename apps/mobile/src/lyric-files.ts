import * as FileSystem from "expo-file-system/legacy";
import { toByteArray } from "base64-js";
import { decodeLyricBytes, readEmbeddedLyrics } from "./embedded-lyrics";
import { parseLyrics, type LyricSource } from "./mobile-lyrics";

export async function readExternalLyric(uri: string, name: string, size?: number): Promise<LyricSource> {
  if (!/\.(lrc|ttml|xml|txt)$/i.test(name)) throw Error("请选择 LRC 或 TTML 歌词文件");
  const info = await FileSystem.getInfoAsync(uri);
  if (!info.exists || info.isDirectory) throw Error("无法读取歌词文件");
  const length = size ?? info.size;
  if (!Number.isFinite(length) || length > 2 * 1024 * 1024) throw Error("歌词文件不能超过 2 MiB");
  const bytes = toByteArray(await FileSystem.readAsStringAsync(uri, { encoding: FileSystem.EncodingType.Base64 }));
  if (bytes.length > 2 * 1024 * 1024) throw Error("歌词文件不能超过 2 MiB");
  const source: LyricSource = { text: decodeLyricBytes(bytes), name, source: "external", ...(/\.ttml$/i.test(name) ? { format: "ttml" } : {}) };
  const doc = parseLyrics(source); if (!doc.lines.some(l => l.text.trim())) throw Error("歌词文件中没有可显示的文本");
  return source;
}
export async function releaseLyricPickerCopy(uri: string) {
  // Never delete the caller's original external file or a retained song.
  if (FileSystem.cacheDirectory && uri.startsWith(FileSystem.cacheDirectory) && /\/DocumentPicker\//i.test(uri)) await FileSystem.deleteAsync(uri, { idempotent: true }).catch(() => {});
}
export async function readFileLyrics(uri: string, cancelled: () => boolean = () => false): Promise<LyricSource[]> {
  if (!uri.startsWith("file:")) return [];
  const info = await FileSystem.getInfoAsync(uri);
  if (!info.exists || info.isDirectory || !Number.isFinite(info.size)) return [];
  return readEmbeddedLyrics({ size: info.size, read: async (position, length) => {
    if (cancelled()) throw Error("歌曲已经切换");
    return toByteArray(await FileSystem.readAsStringAsync(uri, { encoding: FileSystem.EncodingType.Base64, position, length }));
  } });
}
