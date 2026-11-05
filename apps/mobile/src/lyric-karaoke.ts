import type { LyricLine } from "./mobile-lyrics";
export interface KaraokeRange { from: number; to: number; startMs: number; endMs: number; }
export interface KaraokeNativeLine { text: string; x: number; y: number; width: number; height: number; }
export interface KaraokeHold { inputRange: number[]; outputRange: number[]; }
export interface KaraokePiece { hold?: KaraokeHold; id: string; x: number; y: number; width: number; height: number; startMs: number; endMs: number; }
function normalized(text: string) {
  const chars: string[] = [], indices: number[] = [];
  for (let i = 0; i < text.length; i++) {
    if (/\s/.test(text[i]!)) { if (chars.length && chars.at(-1) !== " ") { chars.push(" "); indices.push(i); } }
    else { chars.push(text[i]!); indices.push(i); }
  }
  if (chars.at(-1) === " ") { chars.pop(); indices.pop(); }
  return { text: chars.join(""), indices };
}
/** Map authored timings to the exact displayed string, preserving Unicode and spaces. */
export function karaokeWordRanges(line: LyricLine): KaraokeRange[] {
  if (!line.words.length) return [];
  const source = normalized(line.words.map(word => word.text).join("")), display = normalized(line.text);
  if (source.text !== display.text) return [];
  let offset = 0, cursor = 0;
  const ranges: KaraokeRange[] = [];
  for (const word of line.words) {
    const end = offset + word.text.length;
    while (cursor < source.indices.length && source.indices[cursor]! < offset) cursor++;
    const first = cursor;
    while (cursor < source.indices.length && source.indices[cursor]! < end) cursor++;
    if (cursor > first && Number.isFinite(word.startMs) && Number.isFinite(word.endMs) && word.endMs >= word.startMs) {
      ranges.push({ from: display.indices[first]!, to: display.indices[cursor - 1]! + 1, startMs: word.startMs, endMs: word.endMs });
    }
    offset = end;
  }
  return ranges;
}
export function karaokeFraction(startMs: number, endMs: number, positionMs: number) {
  if (endMs <= startMs) return positionMs >= startMs ? 1 : 0;
  return Math.max(0, Math.min(1, (positionMs - startMs) / (endMs - startMs)));
}
/** A held syllable blooms once, then releases; fast multi-character spans stay calm. */
export function karaokeHeldNote(text: string, startMs: number, endMs: number): KaraokeHold | undefined {
  const duration = endMs - startMs;
  if (!Number.isFinite(duration) || duration < 1200) return;
  let units = 0, inWord = false;
  for (const character of text) {
    if (/[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u.test(character)) { units++; inWord = false; }
    else if (/[\p{L}\p{N}]/u.test(character)) { if (!inWord) units++; inWord = true; }
    else if (!/[\p{M}'’\-]/u.test(character)) inWord = false;
  }
  if (!units || duration / units < 1000) return;
  const peak = Math.min(.75, (duration / units - 850) / 1650 * .75);
  return { inputRange: [startMs, startMs + duration * .18, startMs + duration * .55, endMs - Math.min(240, duration * .2), endMs],
    outputRange: [0, peak * .35, peak, peak * .9, 0] };
}

/** Native paragraph wrapping is authoritative; measure prefixes in the same font. */
export function karaokeLayout(text: string, ranges: KaraokeRange[], lines: KaraokeNativeLine[], widths: Record<string, number>) {
  const requests = new Map<string, string>(), pieces: KaraokePiece[] = [];
  const located: { line: KaraokeNativeLine; start: number; end: number; index: number }[] = [];
  let cursor = 0;
  lines.forEach((line, index) => {
    const content = line.text.replace(/[\r\n]+$/, "");
    if (!content) return;
    const start = text.indexOf(content, cursor);
    if (start < 0 || line.width <= 0 || line.height <= 0) return;
    located.push({ line: { ...line, text: content }, start, end: start + content.length, index }); cursor = start + content.length;
  });
  const advance = (line: KaraokeNativeLine, index: number, count: number) => {
    if (count === 0) return 0;
    if (count === line.text.length) return line.width;
    const key = `${index}:${count}`;
    if (widths[key] !== undefined) return widths[key];
    requests.set(key, line.text.slice(0, count)); return undefined;
  };
  ranges.forEach((word, wordIndex) => {
    const runs: { id: string; x: number; y: number; width: number; height: number }[] = [];
    let pending = false;
    for (const { line, start, end, index } of located) {
      const from = Math.max(word.from, start), to = Math.min(word.to, end);
      if (from >= to) continue;
      const left = advance(line, index, from - start), right = advance(line, index, to - start);
      if (left === undefined || right === undefined) { pending = true; continue; }
      if (right > left) runs.push({ id: `${wordIndex}:${index}`, x: line.x + left, y: line.y, width: right - left, height: line.height });
    }
    if (pending) return;
    const total = runs.reduce((sum, run) => sum + run.width, 0), duration = Math.max(0, word.endMs - word.startMs);
    const hold = karaokeHeldNote(text.slice(word.from, word.to), word.startMs, word.endMs);
    let consumed = 0;
    for (const run of runs) {
      pieces.push({ ...run, ...(hold ? { hold } : {}), startMs: word.startMs + duration * consumed / total, endMs: word.startMs + duration * (consumed + run.width) / total });
      consumed += run.width;
    }
  });
  return { requests: [...requests].map(([id, text]) => ({ id, text })), pieces };
}
export interface KaraokeClockAnchor { positionMs: number; reportMs: number; at: number; running: boolean; endMs: number; }
export function karaokeClockNeedsSync(anchor: KaraokeClockAnchor | null, positionMs: number, now: number, running: boolean, endMs: number) {
  if (!anchor || anchor.running !== running || anchor.endMs !== endMs || !running) return true;
  const expected = Math.min(anchor.endMs, anchor.positionMs + now - anchor.at);
  return positionMs < anchor.reportMs || Math.abs(positionMs - expected) > 160;
}
