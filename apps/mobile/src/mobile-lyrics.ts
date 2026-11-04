export interface LyricSource { text: string; format?: "lrc" | "ttml" | "text"; name?: string; source?: "embedded" | "external" | "preview"; }
export interface LyricAgent { id: string; type: "person" | "group" | "other"; name?: string; }
export type LyricVocal = "lead" | "background" | "chorus";
export interface LyricWord { text: string; startMs: number; endMs: number; agentIds?: string[]; }
export interface LyricLine { id: string; text: string; startMs: number; endMs: number; words: LyricWord[]; translation?: string; agentIds?: string[]; vocal?: LyricVocal; background?: LyricLine[]; }
export interface LyricDocument { format: "lrc" | "ttml" | "text"; lines: LyricLine[]; timed: boolean; source: LyricSource; agents?: LyricAgent[]; }
const MAX_TEXT = 2 * 1024 * 1024;
const entity = (s: string) => s.replace(/&(#x[\da-f]+|#\d+|amp|lt|gt|quot|apos);/gi, (all, key: string) => {
  const named: Record<string, string> = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'" };
  if (key[0] !== "#") return named[key.toLowerCase()] ?? all;
  const n = key[1]?.toLowerCase() === "x" ? parseInt(key.slice(2), 16) : parseInt(key.slice(1), 10);
  return n > 0 && n <= 0x10ffff && !(n >= 0xd800 && n <= 0xdfff) ? String.fromCodePoint(n) : "\ufffd";
});

/** Decimal fractions have their actual precision: .1=100ms, .01=10ms, .001=1ms. */
export function lyricTime(value: string, frameRate = 30, tickRate = 1): number | null {
  const text = value.trim().replace(",", ".");
  const clock = /^(?:(\d+):)?(\d+):(\d+(?:\.\d+)?)$/.exec(text);
  if (clock) {
    const seconds = Number(clock[3]), minutes = Number(clock[2]);
    if (seconds >= 60 || clock[1] !== undefined && minutes >= 60) return null;
    return Math.round((Number(clock[1] ?? 0) * 3600 + minutes * 60 + seconds) * 1000);
  }
  const frames = /^(\d+):(\d+):(\d+):(\d+)(?:\.(\d+))?$/.exec(text);
  if (frames && Number(frames[2]) < 60 && Number(frames[3]) < 60 && Number(frames[4]) < frameRate) {
    return Math.round((Number(frames[1]) * 3600 + Number(frames[2]) * 60 + Number(frames[3]) + (Number(frames[4]) + Number(`0.${frames[5] ?? 0}`)) / frameRate) * 1000);
  }
  if (/^\d+(?:\.\d+)?$/.test(text)) return Math.round(Number(text) * 1000);
  const offset = /^([+-]?\d+(?:\.\d+)?)(h|m|s|ms|f|t)$/.exec(text);
  if (!offset) return null;
  const scales: Record<string, number> = { h: 3600000, m: 60000, s: 1000, ms: 1, f: 1000 / frameRate, t: 1000 / tickRate };
  return Math.round(Number(offset[1]) * scales[offset[2]!]!);
}
function finish(lines: LyricLine[], source: LyricSource, format: LyricDocument["format"], timed: boolean): LyricDocument {
  lines.sort((a, b) => a.startMs - b.startMs);
  const merged: LyricLine[] = [];
  for (const line of lines) {
    const previous = merged.at(-1);
    // Bilingual LRC uses repeated timestamps: first text is the original,
    // later distinct text is its translation, sharing the same seek target.
    if (previous && format === "lrc" && timed && previous.startMs === line.startMs) {
      if (!line.text || previous.text === line.text) continue;
      if (previous.text) {
        previous.translation = [...new Set([...(previous.translation?.split("\n") ?? []), line.text])].join("\n");
        continue;
      }
      merged.pop(); // A blank marker at the same time must not hide real lyrics.
    }
    line.id = `${format}-${merged.length}-${line.startMs}`;
    merged.push(line);
  }
  for (let i = 0; i < merged.length; i++) {
    const line = merged[i]!;
    if (!Number.isFinite(line.endMs)) {
      const next = merged.slice(i + 1).find(n => n.startMs > line.startMs);
      line.endMs = next?.startMs ?? Number.POSITIVE_INFINITY;
    }
    line.endMs = Math.max(line.startMs, line.endMs);
    if (line.background) line.background.forEach((backing, n) => {
      backing.id = `${line.id}-background-${n}`;
      backing.endMs = Math.max(backing.startMs, backing.endMs);
      backing.words = backing.words.map((word, i, words) => ({ ...word, endMs: Math.max(word.startMs, Math.min(word.endMs, words[i + 1]?.startMs ?? backing.endMs, backing.endMs)) }));
    });
    line.words = line.words.map((word, n, words) => ({ ...word, endMs: Math.max(word.startMs, Math.min(word.endMs, words[n + 1]?.startMs ?? line.endMs, line.endMs)) }));
  }
  return { format, lines: merged, timed, source };
}
function lrc(source: LyricSource): LyricDocument {
  const lines: LyricLine[] = [];
  const offset = Number(/\[offset\s*:\s*([+-]?\d+)\]/i.exec(source.text)?.[1] ?? 0);
  for (const raw of source.text.split(/\r?\n/)) {
    const tags = [...raw.matchAll(/\[((?:\d+:){1,2}\d+(?:[.,]\d+)?)\]/g)];
    if (!tags.length) continue;
    const body = raw.replace(/\[((?:\d+:){1,2}\d+(?:[.,]\d+)?)\]/g, "").trim();
    const wordTags = [...body.matchAll(/<((?:\d+:){1,2}\d+(?:[.,]\d+)?)>([^<]*)/g)];
    const text = entity(body.replace(/<((?:\d+:){1,2}\d+(?:[.,]\d+)?)>/g, "")).trim();
    for (const tag of tags) {
      const time = lyricTime(tag[1]!); if (time === null) continue;
      const first = lyricTime(tags[0]![1]!) ?? time;
      const words: LyricWord[] = wordTags.flatMap(w => {
        const start = lyricTime(w[1]!); return start === null ? [] : [{ text: entity(w[2]!), startMs: Math.max(0, start + time - first - offset), endMs: Infinity }];
      });
      lines.push({ id: "", text, startMs: Math.max(0, time - offset), endMs: Infinity, words });
    }
  }
  // A tagged blank row marks an instrumental gap; retain its boundary.
  return finish(lines, source, "lrc", lines.length > 0);
}
interface Node { tag: string; attrs: Record<string, string>; children: (Node | string)[]; }
function xml(text: string): Node {
  if (/<!DOCTYPE|<!ENTITY/i.test(text)) throw Error("TTML 不支持外部实体或 DTD");
  const root: Node = { tag: "root", attrs: {}, children: [] }, stack = [root];
  const tokens = text.match(/<!--[\s\S]*?-->|<!\[CDATA\[[\s\S]*?\]\]>|<\?[\s\S]*?\?>|<(?:[^>"']|"[^"]*"|'[^']*')*>|[^<]+/g) ?? [];
  let count = 0;
  for (const token of tokens) {
    if (token.startsWith("<!--") || token.startsWith("<?")) continue;
    if (token.startsWith("<![CDATA[")) { stack.at(-1)!.children.push(token.slice(9, -3)); continue; }
    if (token.startsWith("</")) {
      const tag = token.slice(2, -1).trim().split(":").at(-1);
      if (stack.length < 2 || stack.at(-1)!.tag !== tag) throw Error("TTML 标签没有正确闭合");
      stack.pop(); continue;
    }
    if (token.startsWith("<")) {
      if (++count > 30000 || stack.length > 64) throw Error("TTML 文件结构过大");
      const match = /^<([\w:.-]+)/.exec(token); if (!match) throw Error("TTML 标签无效");
      const node: Node = { tag: match[1]!.split(":").at(-1)!, attrs: {}, children: [] };
      for (const a of token.matchAll(/([\w:.-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')/g)) node.attrs[a[1]!] = entity(a[2] ?? a[3] ?? "");
      stack.at(-1)!.children.push(node);
      if (!/\/\s*>$/.test(token)) stack.push(node);
    } else stack.at(-1)!.children.push(entity(token));
  }
  if (stack.length !== 1) throw Error("TTML 标签没有正确闭合");
  return root;
}
const attr = (node: Node, key: string) => node.attrs[key] ?? Object.entries(node.attrs).find(([name]) => name.split(":").at(-1) === key)?.[1];
function ttml(source: LyricSource): LyricDocument {
  const root = xml(source.text), tt = root.children.find((c): c is Node => typeof c !== "string" && c.tag === "tt");
  if (!tt) throw Error("没有找到 TTML 的 tt 根节点");
  let rate = Number(attr(tt, "frameRate") ?? 30), ticks = Number(attr(tt, "tickRate") ?? 1);
  const multiplier = attr(tt, "frameRateMultiplier")?.split(/\s+/).map(Number);
  if (multiplier?.length === 2 && multiplier[1]) rate *= multiplier[0]! / multiplier[1];
  if (!Number.isFinite(rate) || rate <= 0 || !Number.isFinite(ticks) || ticks <= 0) throw Error("TTML 时间基准无效");
  const apple = /music\.apple\.com\/lyric|ttml\.amll\.dev/i.test(source.text);
  const lines: LyricLine[] = [], agents = new Map<string, LyricAgent>(), sidecarText = new Map<string, string[]>();
  const ids = (value: string | undefined) => value?.trim().split(/\s+/).filter(Boolean).map(id => id.replace(/^#/, ""));
  const normalize = (value: string) => value.replace(/[\t\r ]+/g, " ").replace(/ *\n */g, "\n").trim();
  const plain = (n: Node): string => n.children.map(c => typeof c === "string" ? c : c.tag === "br" ? "\n" : plain(c)).join("");
  const translated = (n: Node) => /translation|roman/i.test(attr(n, "role") ?? "");
  const background = (n: Node) => /(?:^|\s)(?:x-bg|background|x-background)(?:\s|$)/i.test(attr(n, "role") ?? "");
  const chorus = (n: Node) => /(?:^|\s)(?:x-)?chorus(?:\s|$)/i.test(attr(n, "role") ?? "");
  const supporting = (n: Node) => background(n) || chorus(n);
  function metadata(n: Node, inSidecar = false) {
    const sidecar = inSidecar || n.tag === "translations" || n.tag === "transliterations";
    if (sidecar && n.tag === "text") {
      const text = normalize(plain(n));
      if (text) for (const key of ids(attr(n, "for")) ?? []) sidecarText.set(key, [...(sidecarText.get(key) ?? []), text]);
    }
    if (n.tag === "agent") {
      const id = attr(n, "id"), type = attr(n, "type");
      const name = n.children.find((c): c is Node => typeof c !== "string" && c.tag === "name");
      if (id) agents.set(id, { id, type: type === "person" || type === "group" ? type : "other", ...(name && normalize(plain(name)) ? { name: normalize(plain(name)) } : {}) });
    }
    for (const c of n.children) if (typeof c !== "string") metadata(c, sidecar);
  }
  metadata(tt);
  function voiceIds(n: Node, inherited: string[]) {
    const result = ids(attr(n, "agent")) ?? inherited;
    for (const id of result) if (!agents.has(id)) agents.set(id, { id, type: "other" });
    return result;
  }
  function vocalRole(n: Node, inherited: LyricVocal): LyricVocal {
    return background(n) ? "background" : chorus(n) ? "chorus" : inherited;
  }
  function times(n: Node, parentStart: number, parentEnd: number, fallback: number): [number, number] {
    const begin = attr(n, "begin"), end = attr(n, "end"), dur = attr(n, "dur");
    const value = (s: string) => { const t = lyricTime(s, rate, ticks); if (t === null) throw Error(`无法识别 TTML 时间：${s}`); return t; };
    const base = (s: string) => apple && (s.includes(":") || /^\d+(?:\.\d+)?$/.test(s)) ? 0 : fallback;
    const start = begin === undefined ? fallback : base(begin) + value(begin);
    let finish = end === undefined ? Infinity : base(end) + value(end);
    if (dur !== undefined) finish = Math.min(finish, start + value(dur));
    return [Math.max(parentStart, start), Math.min(parentEnd, finish)];
  }
  function collect(n: Node, start: number, end: number, words: LyricWord[], secondary: string[], backing: LyricLine[], inherited: string[], inTranslation = false, inBackground = false): string {
    const here = voiceIds(n, inherited), isTranslation = inTranslation || translated(n);
    if (attr(n, "ruby") === "text" || n.tag === "metadata") return "";
    if (n.tag === "br") return "\n";
    if (supporting(n) && !inBackground && !isTranslation) {
      const line = buildLine(n, start, end, here, vocalRole(n, "background"), true);
      if (line) backing.push(line);
      return "";
    }
    const parts: string[] = []; let cursor = start;
    for (const child of n.children) {
      if (typeof child === "string") { parts.push(child); continue; }
      const [s, e] = times(child, start, end, attr(n, "timeContainer") === "seq" ? cursor : start);
      if (Number.isFinite(e)) cursor = e;
      const text = collect(child, s, e, words, secondary, backing, here, isTranslation, inBackground);
      if (!isTranslation && !translated(child) && !supporting(child) && (attr(child, "begin") !== undefined || attr(child, "end") !== undefined || attr(child, "dur") !== undefined) && text.trim() && !child.children.some(x => typeof x !== "string" && (attr(x, "begin") !== undefined || attr(x, "dur") !== undefined))) {
        const speaker = voiceIds(child, here);
        words.push({ text, startMs: s, endMs: e, ...(speaker.length ? { agentIds: speaker } : {}) });
      }
      parts.push(text);
    }
    const result = parts.join("");
    if (isTranslation && !inTranslation) { if (result.trim()) secondary.push(result.trim()); return ""; }
    return result;
  }
  function buildLine(n: Node, start: number, end: number, inherited: string[], role: LyricVocal, inBackground = false): LyricLine | null {
    const here = voiceIds(n, inherited), words: LyricWord[] = [], secondary: string[] = [], backing: LyricLine[] = [];
    const text = normalize(collect(n, start, end, words, secondary, backing, here, false, inBackground));
    words.sort((a, b) => a.startMs - b.startMs);
    const first = attr(n, "begin") === undefined ? words[0]?.startMs ?? start : start;
    let last = Number.isFinite(end) ? end : words.length && Number.isFinite(words.at(-1)!.endMs) ? words.at(-1)!.endMs : Infinity;
    if (role === "background" && attr(n, "end") === undefined && attr(n, "dur") === undefined && words.length && Number.isFinite(words.at(-1)!.endMs)) last = Math.min(last, words.at(-1)!.endMs);
    const vocal: LyricVocal = role === "background" ? role : role === "chorus" || here.length > 1 || here.some(id => agents.get(id)?.type === "group") ? "chorus" : "lead";
    const translations = [...new Set([...secondary.map(normalize), ...(sidecarText.get(attr(n, "key") ?? attr(n, "id") ?? "") ?? [])])];
    if (!text) { lines.push(...backing); return null; }
    return { id: "", text, startMs: first, endMs: last, words, vocal, ...(here.length ? { agentIds: here } : {}), ...(translations.length ? { translation: translations.join("\n") } : {}), ...(backing.length ? { background: backing } : {}) };
  }
  function visit(n: Node, parentStart: number, parentEnd: number, fallback: number, inherited: string[], role: LyricVocal): number {
    const [start, end] = times(n, parentStart, parentEnd, fallback), here = voiceIds(n, inherited), vocal = vocalRole(n, role);
    if (n.tag === "p") {
      const voices = n.children.filter((c): c is Node => typeof c !== "string" && c.tag === "span" && attr(c, "agent") !== undefined && !translated(c) && !background(c));
      const splitVoices = new Set(voices.map(c => voiceIds(c, here).join(" "))).size > 1 && n.children.every(c => typeof c === "string" ? !c.trim() : voices.includes(c) || translated(c));
      let extent = start;
      if (splitVoices) {
        const translations = [...new Set([...n.children.filter((c): c is Node => typeof c !== "string" && translated(c)).map(c => normalize(plain(c))).filter(Boolean), ...(sidecarText.get(attr(n, "key") ?? attr(n, "id") ?? "") ?? [])])];
        for (const child of voices) {
          const [s, e] = times(child, start, end, start), line = buildLine(child, s, e, here, vocal);
          if (line) { if (translations.length) line.translation = [...new Set([...(line.translation?.split("\n") ?? []), ...translations])].join("\n"); lines.push(line); extent = Math.max(extent, line.endMs); }
        }
      } else {
        const line = buildLine(n, start, end, here, vocal, vocal === "background" || vocal === "chorus");
        if (line) { lines.push(line); extent = line.endMs; }
      }
      return Number.isFinite(end) ? end : extent;
    }
    if (["head", "metadata", "styling", "layout"].includes(n.tag)) return start;
    let cursor = start, extent = start;
    for (const c of n.children) if (typeof c !== "string") {
      const e = visit(c, start, end, attr(n, "timeContainer") === "seq" ? cursor : start, here, vocal);
      if (Number.isFinite(e)) { cursor = e; extent = Math.max(extent, e); }
    }
    return Number.isFinite(end) ? end : extent;
  }
  visit(tt, 0, Infinity, 0, [], "lead");
  if (!lines.length) throw Error("TTML 中没有可显示的歌词段落");
  return { ...finish(lines, source, "ttml", /\b(?:begin|end|dur)\s*=/.test(source.text)), agents: [...agents.values()] };
}
export function parseLyrics(source: LyricSource): LyricDocument {
  if (source.text.length > MAX_TEXT) throw Error("歌词文件超过 2 MiB");
  const text = source.text.replace(/^\ufeff/, "").replace(/\u0000/g, "");
  const clean = { ...source, text };
  if (source.format === "ttml" || /<(?:[\w-]+:)?tt(?:\s|>)/i.test(text)) return ttml(clean);
  const parsed = lrc(clean);
  if (parsed.timed) return parsed;
  return finish(text.split(/\r?\n/).filter(text => !/^\s*\[(?:ar|ti|al|by|re|ve|length|offset)\s*:/i.test(text)).map(text => ({ id: "", text: text.trim(), startMs: 0, endMs: Infinity, words: [] })).filter(l => l.text), clean, "text", false);
}
export function activeLyricIndices(document: LyricDocument, positionMs: number): number[] {
  if (!document.timed) return [];
  let low = 0, high = document.lines.length;
  while (low < high) { const mid = (low + high) >>> 1; if (document.lines[mid]!.startMs <= positionMs) low = mid + 1; else high = mid; }
  const active: number[] = [];
  for (let i = 0; i < low; i++) if (positionMs < document.lines[i]!.endMs || document.lines[i]!.background?.some(backing => positionMs >= backing.startMs && positionMs < backing.endMs)) active.push(i);
  return active;
}
export function activeLyricIndex(document: LyricDocument, positionMs: number): number {
  return activeLyricIndices(document, positionMs)[0] ?? -1;
}
/** Keep the clock's nearest cue in view during instrumental gaps, without highlighting it. */
export function lyricFollowIndex(document: LyricDocument, positionMs: number): number {
  if (!document.timed || !document.lines.length) return -1;
  const singing = activeLyricIndex(document, positionMs); if (singing >= 0) return singing;
  let low = 0, high = document.lines.length;
  while (low < high) { const mid = (low + high) >>> 1; if (document.lines[mid]!.startMs <= positionMs) low = mid + 1; else high = mid; }
  return Math.max(0, low - 1);
}
/** Display-only grouping: preserve every voice and its original timing. */
export function lyricPresentationDocument(document: LyricDocument): LyricDocument {
  if (!document.timed) return document;
  const leads = document.lines.filter(line => line.vocal !== "background" && line.vocal !== "chorus");
  const attached = new Map<LyricLine, LyricLine[]>(), owners = new Set<LyricLine>();
  let cursor = 0, sounding: LyricLine[] = [];
  for (const line of document.lines) {
    while (cursor < leads.length && leads[cursor]!.startMs <= line.startMs) sounding.push(leads[cursor++]!);
    sounding = sounding.filter(lead => lead.endMs > line.startMs);
    if (line.vocal !== "background" && line.vocal !== "chorus") continue;
    const shared = sounding.filter(lead => lead.agentIds?.some(id => line.agentIds?.includes(id)));
    // Ambiguous simultaneous leads stay separate rather than assigning the wrong singer.
    const candidates = shared.length ? shared : sounding;
    if (candidates.length !== 1) continue;
    const parent = candidates[0]!;
    attached.set(parent, [...(attached.get(parent) ?? []), line]); owners.add(line);
  }
  if (!owners.size) return document;
  return { ...document, lines: document.lines.filter(line => !owners.has(line)).map(line => {
    const extra = attached.get(line);
    return extra ? { ...line, background: [...(line.background ?? []), ...extra].sort((a, b) => a.startMs - b.startMs) } : line;
  }) };
}
/** Metadata order defines the duet sides; backing-only and group agents do not move them. */
export function lyricSpeakerAlignments(document: LyricDocument): ReadonlyMap<string, "left" | "right"> {
  const encountered = [...new Set(document.lines.filter(row => row.vocal !== "background" && row.vocal !== "chorus").flatMap(row => row.agentIds ?? []))];
  const declared = document.agents?.filter(agent => agent.type !== "group" && encountered.includes(agent.id)).map(agent => agent.id) ?? [];
  const speakers = [...declared, ...encountered.filter(id => !declared.includes(id))];
  return new Map(speakers.map((id, index) => [id, index % 2 === 1 ? "right" : "left"]));
}
export function lyricSpeakerLabel(document: LyricDocument, line: LyricLine): string {
  const speakers = line.agentIds ?? [], names = speakers.map(id => document.agents?.find(agent => agent.id === id)?.name || `声部 ${id}`);
  if (line.vocal === "chorus") return names.length ? names.join(" / ").includes("合唱") ? names.join(" / ") : `合唱 · ${names.join(" / ")}` : "合唱";
  if (line.vocal === "background") return names.length ? `伴唱 · ${names.join(" / ")}` : "伴唱";
  return (document.agents?.length ?? 0) > 1 && names.length ? names.join(" / ") : "";
}
export function lyricSources(value: unknown): LyricSource[] {
  if (typeof value === "string") return value.trim() ? [{ text: value, source: "embedded" }] : [];
  if (!Array.isArray(value)) return [];
  return value.flatMap(item => {
    if (typeof item === "string") return lyricSources(item);
    if (!item || typeof item !== "object") return [];
    const text = item.text ?? item.lyrics;
    return typeof text === "string" && text.trim() ? [{ text, format: item.format, name: item.name ?? item.filename, source: item.source ?? "embedded" }] : [];
  });
}
