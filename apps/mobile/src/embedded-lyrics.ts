import type { LyricSource } from "./mobile-lyrics";
export interface LyricByteReader { size: number; read(position: number, length: number): Promise<Uint8Array>; }
const LIMIT = 2 * 1024 * 1024;
const be32 = (b: Uint8Array, i = 0) => (b[i]! * 0x1000000 + (b[i + 1]! << 16) + (b[i + 2]! << 8) + b[i + 3]!) >>> 0;
const le32 = (b: Uint8Array, i = 0) => (b[i]! + (b[i + 1]! << 8) + (b[i + 2]! << 16) + b[i + 3]! * 0x1000000) >>> 0;
const ascii = (b: Uint8Array) => String.fromCharCode(...b);
const syncsafe = (b: Uint8Array, i: number) => ((b[i]! & 127) << 21) | ((b[i + 1]! & 127) << 14) | ((b[i + 2]! & 127) << 7) | (b[i + 3]! & 127);
export function decodeLyricBytes(bytes: Uint8Array, encoding: "utf8" | "utf16le" | "utf16be" | "latin1" = "utf8"): string {
  if (bytes[0] === 0xff && bytes[1] === 0xfe) { encoding = "utf16le"; bytes = bytes.subarray(2); }
  else if (bytes[0] === 0xfe && bytes[1] === 0xff) { encoding = "utf16be"; bytes = bytes.subarray(2); }
  else if (bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) bytes = bytes.subarray(3);
  let result = "";
  if (encoding === "latin1") { for (let i = 0; i < bytes.length; i++) result += String.fromCharCode(bytes[i]!); return result.replace(/\u0000+$/, ""); }
  if (encoding !== "utf8") {
    for (let i = 0; i + 1 < bytes.length; i += 2) result += String.fromCharCode(encoding === "utf16le" ? bytes[i]! | bytes[i + 1]! << 8 : bytes[i]! << 8 | bytes[i + 1]!);
    return result.replace(/\u0000+$/, "");
  }
  // Hermes/Expo Go does not consistently supply all TextDecoder encodings.
  // Decode UTF-8/UTF-16 explicitly; never turn CJK tags into Latin-1 mojibake.
  for (let i = 0; i < bytes.length;) {
    const a = bytes[i++]!;
    if (a < 128) { result += String.fromCharCode(a); continue; }
    const n = a >= 0xc2 && a <= 0xdf ? 1 : a >= 0xe0 && a <= 0xef ? 2 : a >= 0xf0 && a <= 0xf4 ? 3 : 0;
    if (!n || i + n > bytes.length) { result += "\ufffd"; continue; }
    let code = a & (0x7f >> n), valid = true;
    for (let j = 0; j < n; j++) { const b = bytes[i + j]!; if ((b & 0xc0) !== 0x80) { valid = false; break; } code = code << 6 | b & 63; }
    if (!valid || code > 0x10ffff || code >= 0xd800 && code <= 0xdfff || code < (n === 1 ? 128 : n === 2 ? 2048 : 65536)) { result += "\ufffd"; continue; }
    i += n; result += String.fromCodePoint(code);
  }
  return result.replace(/\u0000+$/, "");
}
function terminated(bytes: Uint8Array, start: number, wide: boolean): number {
  for (let i = start; i + (wide ? 1 : 0) < bytes.length; i += wide ? 2 : 1) if (bytes[i] === 0 && (!wide || bytes[i + 1] === 0)) return i;
  return bytes.length;
}
function deunsync(bytes: Uint8Array): Uint8Array { return bytes.filter((_, i) => !(i > 0 && bytes[i] === 0 && bytes[i - 1] === 0xff)); }
function textFrame(bytes: Uint8Array, kind: string): string | null {
  if (bytes.length < 2) return null;
  const e = bytes[0]!, wide = e === 1 || e === 2;
  const start = kind === "USLT" || kind === "ULT" || kind === "SYLT" ? (kind === "SYLT" ? 6 : 4) : 1;
  const encoding = e === 0 ? "latin1" : e === 2 || e === 1 && bytes[start] === 0xfe && bytes[start + 1] === 0xff ? "utf16be" : e === 1 ? "utf16le" : "utf8";
  const descriptorEnd = terminated(bytes, start, wide), textStart = Math.min(bytes.length, descriptorEnd + (wide ? 2 : 1));
  if (kind === "TXXX" || kind === "TXX") {
    const key = decodeLyricBytes(bytes.subarray(start, descriptorEnd), encoding);
    if (!/lyric|ttml|^lrc$/i.test(key)) return null;
  }
  if (kind === "SYLT") {
    if (bytes[4] !== 2) return null; // Timestamp format 2 is milliseconds, 1 is MPEG frames.
    const rows: string[] = []; let p = textStart;
    while (p < bytes.length) {
      const end = terminated(bytes, p, wide), at = end + (wide ? 2 : 1); if (at + 4 > bytes.length) break;
      const text = decodeLyricBytes(bytes.subarray(p, end), encoding); const time = be32(bytes, at);
      rows.push(`[${Math.floor(time / 60000)}:${((time % 60000) / 1000).toFixed(3).padStart(6, "0")}]${text}`); p = at + 4;
    }
    return rows.join("\n");
  }
  return decodeLyricBytes(bytes.subarray(textStart), encoding);
}
export async function readEmbeddedLyrics(input: LyricByteReader): Promise<LyricSource[]> {
  const found: LyricSource[] = []; let budget = 0, atoms = 0;
  const read = async (at: number, count: number) => {
    if (!Number.isSafeInteger(at) || at < 0 || count < 0 || at + count > input.size || count > LIMIT || (budget += count) > 8 * LIMIT) throw Error("歌词元数据超出读取范围");
    const data = await input.read(at, count); if (data.length !== count) throw Error("歌词元数据不完整"); return data;
  };
  const add = (text: string | null, name?: string) => {
    if (!text?.trim() || found.some(s => s.text === text) || found.length >= 4) return;
    found.push({ text: text.replace(/^\ufeff/, ""), source: "embedded", name });
  };
  async function id3(start: number, limit: number) {
    if (limit - start < 10) return;
    const head = await read(start, 10); if (ascii(head.subarray(0, 3)) !== "ID3") return;
    const version = head[3]!; let end = Math.min(limit, start + 10 + syncsafe(head, 6));
    if (![2, 3, 4].includes(version)) return;
    let readTag = read;
    // In ID3v2.2/2.3 global unsynchronization also shifts later frame headers.
    // Decode a bounded tag before interpreting its logical frame offsets.
    if (version < 4 && head[5]! & 0x80) {
      if (end - start - 10 > LIMIT) return;
      const tag = deunsync(await read(start + 10, end - start - 10));
      end = start + 10 + tag.length;
      readTag = async (at, count) => tag.subarray(at - start - 10, at - start - 10 + count);
    }
    let at = start + 10;
    if (head[5]! & 0x40 && version >= 3) { const ext = await readTag(at, 4); at += version === 4 ? syncsafe(ext, 0) : be32(ext) + 4; }
    while (at + (version === 2 ? 6 : 10) <= end && ++atoms < 4096) {
      const headerSize = version === 2 ? 6 : 10, frame = await readTag(at, headerSize);
      const name = ascii(frame.subarray(0, version === 2 ? 3 : 4)); if (!/^[A-Z0-9]+$/.test(name)) break;
      const size = version === 2 ? frame[3]! << 16 | frame[4]! << 8 | frame[5]! : version === 4 ? syncsafe(frame, 4) : be32(frame, 4);
      const next = at + headerSize + size; if (!size || next > end) break;
      if (["USLT", "ULT", "SYLT", "TXXX", "TXX"].includes(name) && size <= LIMIT && !(version >= 3 && frame[9]! & (version === 3 ? 0xc0 : 0x0c))) {
        let payload = await readTag(at + headerSize, size);
        if (version === 4 && (head[5]! & 0x80 || frame[9]! & 2)) payload = deunsync(payload);
        if (version === 4 && frame[9]! & 1) payload = payload.subarray(4);
        add(textFrame(payload, name), name);
      }
      at = next;
    }
  }
  interface Box { type: string; start: number; data: number; end: number; }
  async function boxes(start: number, end: number): Promise<Box[]> {
    const result: Box[] = [];
    for (let p = start; p + 8 <= end && ++atoms < 4096;) {
      const h = await read(p, 8); let size = be32(h), header = 8;
      if (size === 1) { const ext = await read(p + 8, 8); size = be32(ext) * 0x100000000 + be32(ext, 4); header = 16; }
      else if (size === 0) size = end - p;
      if (!Number.isSafeInteger(size) || size < header || p + size > end) break;
      result.push({ type: ascii(h.subarray(4)), start: p, data: p + header, end: p + size }); p += size;
    }
    return result;
  }
  async function dataText(box: Box) {
    const children = await boxes(box.data, box.end);
    for (const c of children) if (c.type === "data" && c.end - c.data >= 8 && c.end - c.data <= LIMIT) {
      const b = await read(c.data, c.end - c.data), type = be32(b) & 0xffffff;
      add(decodeLyricBytes(b.subarray(8), type === 2 ? "utf16be" : "utf8"), box.type);
    }
  }
  async function mp4(start: number, end: number, depth = 0) {
    if (depth > 8) return;
    for (const box of await boxes(start, end)) {
      if (["moov", "udta", "meta", "ilst"].includes(box.type)) {
        if (box.type === "meta" && box.end - box.data >= 4) await mp4(box.data + 4, box.end, depth + 1);
        else await mp4(box.data, box.end, depth + 1);
      } else if (box.type === "©lyr") await dataText(box);
      else if (box.type === "----") {
        const children = await boxes(box.data, box.end); let key = "";
        for (const c of children) if (c.type === "name" && c.end - c.data > 4 && c.end - c.data < 1024) key = decodeLyricBytes((await read(c.data, c.end - c.data)).subarray(4));
        if (/lyric|ttml|^lrc$/i.test(key)) await dataText(box);
      } else if (box.type === "ID32" && box.end - box.data > 10) {
        const h = await read(box.data, Math.min(16, box.end - box.data));
        for (let n = 0; n + 3 <= h.length; n++) if (ascii(h.subarray(n, n + 3)) === "ID3") { await id3(box.data + n, box.end); break; }
      }
    }
  }
  if (input.size < 10) return found;
  const magic = await read(0, Math.min(16, input.size));
  if (ascii(magic.subarray(0, 3)) === "ID3") await id3(0, input.size);
  else if (["ftyp", "moov", "free", "wide"].includes(ascii(magic.subarray(4, 8)))) await mp4(0, input.size);
  else if (ascii(magic.subarray(0, 4)) === "fLaC") {
    for (let at = 4; at + 4 <= input.size && ++atoms < 4096;) {
      const h = await read(at, 4), length = h[1]! << 16 | h[2]! << 8 | h[3]!, end = at + 4 + length; if (end > input.size) break;
      if ((h[0]! & 127) === 4 && length <= LIMIT) {
        const b = await read(at + 4, length); let p = 4 + le32(b); if (p + 4 > b.length) break; const count = le32(b, p); p += 4;
        for (let i = 0; i < count && i < 10000 && p + 4 <= b.length; i++) {
          const n = le32(b, p); p += 4; if (p + n > b.length) break;
          const text = decodeLyricBytes(b.subarray(p, p + n)), eq = text.indexOf("=");
          if (/^(?:lyrics|unsyncedlyrics|lrc|ttml)$/i.test(text.slice(0, eq))) add(text.slice(eq + 1), text.slice(0, eq)); p += n;
        }
      }
      if (h[0]! & 128) break; at = end;
    }
  } else if (["RIFF", "RF64"].includes(ascii(magic.subarray(0, 4))) && ascii(magic.subarray(8, 12)) === "WAVE") {
    for (let at = 12; at + 8 <= input.size && ++atoms < 4096;) {
      const h = await read(at, 8), size = le32(h, 4), end = at + 8 + size; if (end > input.size) break;
      if (["id3 ", "ID3 "].includes(ascii(h.subarray(0, 4)))) await id3(at + 8, end);
      at = end + (size & 1);
    }
  }
  return found;
}
