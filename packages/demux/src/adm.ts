import { DOMParser, type Element } from "@xmldom/xmldom";

/** ADM targets use the renderer's forward ramp, beginning at samplePos. */
export interface AdmObjectEvent {
  id: number;
  samplePos: number;
  hasPos: boolean;
  pos: [number, number, number];
  gainDb: number;
  diffuse?: number;
  horizontalOnly?: boolean;
  size: [number, number, number];
  anchor: "room" | "screen" | "speaker";
  distanceM: number | null;
  distanceInfinite: boolean;
  screenFactor: number | null;
  depthFactor: number | null;
  rampDuration: number;
}

export interface AdmMetadata {
  labels: string[];
  rawBedLabels: string[];
  objectChannels: { id: number; channel: number }[];
  events: AdmObjectEvent[];
  title?: string;
}

interface TrackAssignment { channel: number; uid: string; trackRef: string; packRef: string }
interface ObjectContext {
  start: number;
  end: number;
  gainDb: number;
  offsets: Element[];
}

function fail(message: string): never { throw new Error(`ADM: ${message}`); }
function children(element: Element, name: string): Element[] {
  const result: Element[] = [];
  for (let node = element.firstChild; node; node = node.nextSibling) {
    if (node.nodeType === 1 && (node as Element).localName === name) result.push(node as Element);
  }
  return result;
}
function value(element: Element, name: string): string | undefined {
  const entries = children(element, name);
  if (entries.length > 1) fail(`multiple ${name} values are unsupported`);
  return entries[0]?.textContent?.trim();
}
function refs(element: Element, name: string): string[] {
  return children(element, name).map((entry) => entry.textContent?.trim() ?? "");
}
function number(text: string | null | undefined, fallback: number, description: string): number {
  if (text == null || text === "") return fallback;
  const result = Number(text);
  if (!Number.isFinite(result)) fail(`invalid ${description}: ${text}`);
  return result;
}
function flag(text: string | undefined, description: string): boolean {
  if (text == null || text === "0" || text === "false") return false;
  if (text === "1" || text === "true") return true;
  return fail(`invalid ${description}: ${text}`);
}

/** BS.2076 decimal seconds and exact sample fractions (hh:mm:ss.numeratorSdenominator). */
export function admTimeSeconds(text: string): number {
  const match = /^(\d+):(\d{2}):(\d{2})(?:\.(\d+)(?:S(\d+))?)?$/.exec(text);
  if (!match) return fail(`invalid time ${text}`);
  const hours = Number(match[1]);
  const minutes = Number(match[2]);
  const seconds = Number(match[3]);
  if (minutes > 59 || seconds > 59) fail(`invalid time ${text}`);
  const numerator = Number(match[4] ?? 0);
  const denominator = match[5] ? Number(match[5]) : 10 ** (match[4]?.length ?? 0);
  if (!(denominator > 0) || numerator >= denominator) fail(`invalid time fraction ${text}`);
  const result = hours * 3600 + minutes * 60 + seconds + numerator / denominator;
  if (!Number.isFinite(result)) fail(`invalid time ${text}`);
  return result;
}

function timeAttribute(element: Element, name: string, fallback: number): number {
  const text = element.getAttribute(name);
  return text ? admTimeSeconds(text) : fallback;
}
function gain(element: Element): number {
  const entries = children(element, "gain");
  if (entries.length > 1) fail("multiple gains are unsupported");
  const entry = entries[0];
  if (!entry) return 0;
  const amount = number(entry.textContent?.trim(), 1, "gain");
  const unit = entry.getAttribute("gainUnit") || "linear";
  if (unit === "dB") return amount;
  if (unit !== "linear" || amount < 0) fail(`unsupported gain ${amount} ${unit}`);
  return amount === 0 ? -200 : 20 * Math.log10(amount);
}

function parseChna(bytes: Uint8Array, channelCount?: number): TrackAssignment[] {
  if (bytes.length < 4) fail("CHNA header is truncated");
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const tracks = view.getUint16(0, true);
  const entries = view.getUint16(2, true);
  if (!tracks || (channelCount != null && tracks !== channelCount)) fail("CHNA track count does not match PCM channels");
  if (bytes.length < 4 + entries * 40) fail("CHNA audioID entries are truncated");
  const ascii = (offset: number, size: number) => new TextDecoder("ascii").decode(bytes.subarray(offset, offset + size)).replace(/\0.*$/, "").trim();
  const assigned = new Set<number>();
  const uids = new Set<string>();
  const result: TrackAssignment[] = [];
  for (let index = 0; index < entries; index++) {
    const offset = 4 + index * 40;
    const channel = view.getUint16(offset, true) - 1;
    const uid = ascii(offset + 2, 12);
    const trackRef = ascii(offset + 14, 14);
    const packRef = ascii(offset + 28, 11);
    if (channel < 0 || channel >= tracks || !/^ATU_[0-9a-f]{8}$/i.test(uid)) fail("invalid CHNA audioID entry");
    if (assigned.has(channel) || uids.has(uid)) fail("multiple ADM assignments to one PCM track are unsupported");
    assigned.add(channel);
    uids.add(uid);
    result.push({ channel, uid, trackRef, packRef });
  }
  if (assigned.size !== tracks) fail("CHNA must assign every PCM channel");
  return result.sort((left, right) => left.channel - right.channel);
}

const SPEAKER_LABELS: Record<string, string> = {
  "RC_L": "L", "RC_R": "R", "RC_C": "C", "RC_LFE": "LFE",
  "RC_Lss": "Ls", "RC_Rss": "Rs", "RC_Lrs": "Lb", "RC_Rrs": "Rb",
  "RC_Lts": "Tsl", "RC_Rts": "Tsr",
  "M+030": "L", "M-030": "R", "M+000": "C", "LFE": "LFE", "LFE1": "LFE", "LFE2": "LFE2",
  "M+090": "Ls", "M-090": "Rs", "M+110": "Ls", "M-110": "Rs",
  "M+135": "Lb", "M-135": "Rb", "M+150": "Lb", "M-150": "Rb", "M+180": "Cb", "M-180": "Cb",
  "M+060": "Lw", "M-060": "Rw", "U+030": "Tfl", "U-030": "Tfr", "U+045": "Tfl", "U-045": "Tfr",
  "U+090": "Tsl", "U-090": "Tsr", "U+110": "Tbl", "U-110": "Tbr", "U+135": "Tbl", "U-135": "Tbr",
  "U+000": "Tfc", "T+000": "Tc",
};

function coordinates(element: Element, name: string): Map<string, number> {
  const result = new Map<string, number>();
  for (const entry of children(element, name)) {
    const coordinate = entry.getAttribute("coordinate") ?? "";
    if (entry.hasAttribute("bound") || entry.hasAttribute("screenEdgeLock")) fail(`${name} bounds and screen edge locks are unsupported`);
    if (result.has(coordinate)) fail(`duplicate ${name} coordinate ${coordinate}`);
    result.set(coordinate, number(entry.textContent?.trim(), NaN, name));
  }
  return result;
}
function spherical(azimuth: number, elevation: number, distance: number): [number, number, number] {
  const az = azimuth * Math.PI / 180;
  const el = elevation * Math.PI / 180;
  return [-Math.sin(az) * Math.cos(el) * distance, Math.cos(az) * Math.cos(el) * distance, Math.sin(el) * distance];
}
function checkCoordinates(entries: Map<string, number>, allowed: string[]): void {
  for (const [key, value] of entries) {
    if (!allowed.includes(key) || !Number.isFinite(value)) fail(`unsupported position coordinate ${key}`);
  }
}
function objectPosition(block: Element, context: ObjectContext): { pos: [number, number, number]; cartesian: boolean } {
  const position = coordinates(block, "position");
  const cartesian = flag(value(block, "cartesian"), "cartesian");
  const allowed = cartesian ? ["X", "Y", "Z"] : ["azimuth", "elevation", "distance"];
  checkCoordinates(position, allowed);
  if (cartesian ? !position.has("X") || !position.has("Y") : !position.has("azimuth")) fail("object block has no complete position");
  for (const offset of context.offsets) {
    const coordinate = offset.getAttribute("coordinate") ?? "";
    if (!allowed.includes(coordinate)) fail("positionOffset coordinate system differs from its block");
    const defaultValue = coordinate === "distance" ? 1 : 0;
    position.set(coordinate, (position.get(coordinate) ?? defaultValue) + number(offset.textContent?.trim(), NaN, "positionOffset"));
  }
  if (cartesian) {
    const pos: [number, number, number] = [position.get("X")!, position.get("Y")!, position.get("Z") ?? 0];
    if (pos.some((coordinate) => !Number.isFinite(coordinate) || Math.abs(coordinate) > 1)) fail("Cartesian position outside the supported [-1, 1] range");
    return { pos, cartesian };
  }
  const elevation = position.get("elevation") ?? 0;
  const distance = position.get("distance") ?? 1;
  if (Math.abs(elevation) > 90 || distance < 0) fail("invalid spherical position");
  return { pos: spherical(position.get("azimuth")!, elevation, distance), cartesian };
}

function bedLabel(channel: Element, blocks: Element[]): string {
  if (blocks.length !== 1) fail("time-varying DirectSpeakers channels are unsupported");
  const block = blocks[0]!;
  if (gain(block) !== 0) fail("DirectSpeakers block gain is unsupported");
  if (timeAttribute(block, "rtime", 0) !== 0) fail("delayed DirectSpeakers blocks are unsupported");
  for (const label of refs(block, "speakerLabel")) {
    const mapped = SPEAKER_LABELS[label.split(":").at(-1)!];
    if (mapped) return mapped;
  }
  const entries = coordinates(block, "position");
  if (entries.has("azimuth") && !flag(value(block, "cartesian"), "cartesian")) {
    checkCoordinates(entries, ["azimuth", "elevation", "distance"]);
    const azimuth = entries.get("azimuth")!;
    const elevation = entries.get("elevation") ?? 0;
    const prefix = elevation === 0 ? "M" : elevation === 90 ? "T" : elevation >= 30 && elevation <= 45 ? "U" : "";
    const text = `${prefix}${azimuth < 0 ? "-" : "+"}${String(Math.abs(azimuth)).padStart(3, "0")}`;
    if (SPEAKER_LABELS[text]) return SPEAKER_LABELS[text]!;
  }
  return fail(`unsupported DirectSpeakers position in ${channel.getAttribute("audioChannelFormatID")}`);
}

function unsupportedBlock(block: Element): void {
  for (const name of ["channelLock", "screenRef", "headLocked"]) {
    const text = value(block, name);
    if (text != null && text !== "false" && number(text, 0, name) !== 0) fail(`${name} rendering is unsupported`);
  }
  for (const name of ["objectDivergence", "outputChannelFormatIDRef", "frequency", "matrix", "headphoneVirtualise"]) {
    if (children(block, name).length) fail(`${name} rendering is unsupported`);
  }
}

function horizontalExclusion(block: Element): boolean {
  const exclusions = children(block, "zoneExclusion");
  if (!exclusions.length) return false;
  const zones = exclusions.flatMap(entry => children(entry, "zone"));
  // Dolby's ZB/ZT pair excludes the lower and upper layers. Other region
  // shapes need a general ADM zone renderer and must not be silently ignored.
  const bounds = zones.map(zone => ["minX", "maxX", "minY", "maxY", "minZ", "maxZ"].map(key => number(zone.getAttribute(key), NaN, key)));
  if (bounds.length !== 2 || bounds.some(b => b[0] !== -1 || b[1] !== 1 || b[2] !== -1 || b[3] !== 1)
    || !bounds.some(b => b[4] === -1 && Math.abs(b[5]! + 0.4995) < 1e-6)
    || !bounds.some(b => Math.abs(b[4]! - 0.4995) < 1e-6 && b[5] === 1)) fail("unsupported zoneExclusion shape");
  return true;
}

function objectEvents(id: number, blocks: Element[], context: ObjectContext, sampleRate: number): AdmObjectEvent[] {
  const events: AdmObjectEvent[] = [];
  let previousEnd = 0;
  let lastEnd = Infinity;
  for (let index = 0; index < blocks.length; index++) {
    const block = blocks[index]!;
    unsupportedBlock(block);
    const start = timeAttribute(block, "rtime", 0);
    const next = blocks[index + 1];
    const fallbackDuration = next ? timeAttribute(next, "rtime", 0) - start : context.end - context.start - start;
    const duration = timeAttribute(block, "duration", fallbackDuration);
    if (start < 0 || duration <= 0 || (index > 0 && Math.abs(start - previousEnd) > 0.5 / sampleRate)) fail("object blocks overlap, have gaps, or have invalid durations");
    const { pos, cartesian } = objectPosition(block, context);
    const diffuse = number(value(block, "diffuse"), 0, "diffuse");
    if (diffuse < 0 || diffuse > 1) fail("diffuse must be in [0, 1]");
    const size: [number, number, number] = [number(value(block, "width"), 0, "width"), number(value(block, "depth"), 0, "depth"), number(value(block, "height"), 0, "height")];
    if (size.some((extent) => extent < 0 || extent > 1) || (!cartesian && size.some((extent) => extent !== 0))) fail("only point spherical objects and normalized Cartesian extents are supported");
    const jumps = children(block, "jumpPosition");
    if (jumps.length > 1) fail("multiple jumpPosition values are unsupported");
    const jump = jumps[0];
    const interpolation = flag(jump?.textContent?.trim(), "jumpPosition")
      ? number(jump?.getAttribute("interpolationLength"), 0, "interpolationLength") : duration;
    if (interpolation < 0 || interpolation > duration) fail("invalid interpolationLength");
    if (index > 0 && !Number.isFinite(interpolation)) fail("moving object block requires a duration");
    const absoluteStart = context.start + start;
    if (absoluteStart >= context.end) break;
    const event: AdmObjectEvent = {
      id, samplePos: Math.round(absoluteStart * sampleRate), hasPos: true, pos,
      gainDb: Math.max(-200, gain(block) + context.gainDb), diffuse, horizontalOnly: horizontalExclusion(block), size, anchor: "room",
      distanceM: null, distanceInfinite: false, screenFactor: null, depthFactor: null,
      rampDuration: index === 0 ? 0 : Math.max(0, Math.round(interpolation * sampleRate)),
    };
    if (index === 0 && event.samplePos > 0) events.push({ ...event, samplePos: 0, gainDb: -200, rampDuration: 1 });
    events.push(event);
    previousEnd = start + duration;
    lastEnd = Math.min(context.end, context.start + previousEnd);
  }
  if (!events.length) fail("object has no active audio blocks");
  if (Number.isFinite(lastEnd)) events.push({ ...events.at(-1)!, samplePos: Math.round(lastEnd * sampleRate), gainDb: -200, rampDuration: 1 });
  return events;
}

/** Parse the ADM `axml` and EBU Tech 3306 `chna` chunk payloads. */
export function parseAdmMetadata(axml: string | Uint8Array, chna: Uint8Array, sampleRate: number, channelCount?: number): AdmMetadata {
  if (!Number.isFinite(sampleRate) || sampleRate <= 0) fail("invalid sample rate");
  const xml = typeof axml === "string" ? axml : new TextDecoder("utf-8", { fatal: true }).decode(axml);
  const document = new DOMParser({ onError: (level, message) => { fail(`malformed XML (${level}): ${message}`); } }).parseFromString(xml.replace(/\0+$/, ""), "application/xml");
  if (document.doctype) fail("XML document types are unsupported");
  const all = Array.from(document.getElementsByTagName("*"));
  const elements = (name: string) => all.filter((element) => element.localName === name);
  if (elements("audioFormatExtended").length !== 1) fail("expected one audioFormatExtended element");
  const index = (name: string, attribute: string): Map<string, Element> => {
    const result = new Map<string, Element>();
    for (const element of elements(name)) {
      const id = element.getAttribute(attribute);
      if (!id || result.has(id)) fail(`missing or duplicate ${attribute}`);
      result.set(id, element);
    }
    return result;
  };
  const channels = index("audioChannelFormat", "audioChannelFormatID");
  const tracks = index("audioTrackFormat", "audioTrackFormatID");
  const streams = index("audioStreamFormat", "audioStreamFormatID");
  const packs = index("audioPackFormat", "audioPackFormatID");
  const uids = index("audioTrackUID", "UID");
  const objects = index("audioObject", "audioObjectID");
  const contents = index("audioContent", "audioContentID");
  const programmes = elements("audioProgramme");
  if (programmes.length > 1) fail("multiple audioProgrammes require programme selection");
  const programme = programmes[0];
  // Programme start is the production timecode; object/block times remain
  // relative to the programme and the first PCM sample in Dolby ADM exports.
  const programmeStart = programme ? timeAttribute(programme, "start", 0) : 0;
  const programmeEnd = programme ? timeAttribute(programme, "end", Infinity) - programmeStart : Infinity;
  if (programmeEnd <= 0) fail("audioProgramme has no positive duration");
  const referencedObjects = new Set(Array.from(objects.values()).flatMap((object) => refs(object, "audioObjectIDRef")));
  let rootObjects = Array.from(objects.keys()).filter((id) => !referencedObjects.has(id));
  if (programme) {
    rootObjects = refs(programme, "audioContentIDRef").flatMap((id) => {
      const content = contents.get(id);
      if (!content) return fail(`unresolved audioContent ${id}`);
      return refs(content, "audioObjectIDRef");
    });
  }
  const ownership = new Map<string, ObjectContext>();
  const visit = (id: string, parent: ObjectContext, path: Set<string>) => {
    if (path.has(id)) fail("cyclic audioObject references");
    const object = objects.get(id);
    if (!object) fail(`unresolved audioObject ${id}`);
    if (flag(object.getAttribute("interact") || undefined, "interact") || children(object, "alternativeValueSet").length || children(object, "audioComplementaryObjectIDRef").length) fail("interactive and alternative audioObjects are unsupported");
    const start = parent.start + timeAttribute(object, "start", 0);
    const context: ObjectContext = {
      start, end: Math.min(parent.end, start + timeAttribute(object, "duration", Infinity)),
      gainDb: parent.gainDb + gain(object) + (flag(value(object, "mute"), "mute") ? -200 : 0),
      offsets: [...parent.offsets, ...children(object, "positionOffset")],
    };
    if (context.end <= start) fail("audioObject has no positive duration");
    for (const uid of refs(object, "audioTrackUIDRef")) {
      if (uid === "ATU_00000000") fail("silent track UID allocation is unsupported");
      if (ownership.has(uid)) fail(`audioTrackUID ${uid} is used by multiple audioObjects`);
      ownership.set(uid, context);
    }
    const nextPath = new Set(path).add(id);
    for (const child of refs(object, "audioObjectIDRef")) visit(child, context, nextPath);
  };
  for (const id of rootObjects) visit(id, { start: 0, end: programmeEnd, gainDb: 0, offsets: [] }, new Set());
  const result: AdmMetadata = { labels: [], rawBedLabels: [], objectChannels: [], events: [], title: programme?.getAttribute("audioProgrammeName") || undefined };
  const assignments = parseChna(chna, channelCount);
  for (const assignment of assignments) {
    const uid = uids.get(assignment.uid);
    if (!uid) fail(`CHNA refers to missing audioTrackUID ${assignment.uid}`);
    const declaredRate = number(uid.getAttribute("sampleRate"), sampleRate, "audioTrackUID sampleRate");
    if (declaredRate !== sampleRate) fail("audioTrackUID sampleRate differs from PCM");
    const channelRefs = refs(uid, "audioChannelFormatIDRef");
    const trackRefs = refs(uid, "audioTrackFormatIDRef");
    if (channelRefs.length + trackRefs.length !== 1) fail(`ambiguous channel reference for ${assignment.uid}`);
    const explicitRef = channelRefs[0] ?? trackRefs[0]!;
    if (assignment.trackRef && assignment.trackRef !== explicitRef) fail(`CHNA/AXML track reference mismatch for ${assignment.uid}`);
    let channelId = channelRefs[0];
    if (!channelId) {
      const track = tracks.get(trackRefs[0]!);
      if (!track) fail(`unresolved audioTrackFormat ${trackRefs[0]}; external common definitions are unsupported`);
      const streamId = value(track, "audioStreamFormatIDRef");
      const stream = streamId ? streams.get(streamId) : undefined;
      if (!stream) fail(`unresolved audioStreamFormat for ${trackRefs[0]}`);
      channelId = value(stream, "audioChannelFormatIDRef");
    }
    const channel = channelId ? channels.get(channelId) : undefined;
    if (!channel) fail(`unresolved audioChannelFormat ${channelId}; external common definitions are unsupported`);
    const uidPack = value(uid, "audioPackFormatIDRef");
    if (assignment.packRef && uidPack && assignment.packRef !== uidPack) fail(`CHNA/AXML pack reference mismatch for ${assignment.uid}`);
    const pack = packs.get(uidPack ?? assignment.packRef);
    if (pack && refs(pack, "audioChannelFormatIDRef").length && !refs(pack, "audioChannelFormatIDRef").includes(channelId!)) fail(`channel is not in its audioPackFormat ${uidPack}`);
    const type = channel.getAttribute("typeDefinition") || ({ "0001": "DirectSpeakers", "0003": "Objects" } as Record<string, string>)[channel.getAttribute("typeLabel") || channelId!.slice(3, 7)];
    const blocks = children(channel, "audioBlockFormat");
    if (!blocks.length) fail(`audioChannelFormat ${channelId} has no blocks`);
    const context = ownership.get(assignment.uid) ?? (objects.size === 0 ? { start: 0, end: programmeEnd, gainDb: 0, offsets: [] } : fail(`PCM track ${assignment.uid} is outside the selected programme`));
    if (type === "DirectSpeakers") {
      if (context.start !== 0 || context.gainDb !== 0 || context.offsets.length) fail("timed or adjusted DirectSpeakers audioObjects are unsupported");
      const label = bedLabel(channel, blocks);
      if (result.rawBedLabels.includes(label)) fail(`duplicate DirectSpeakers output label ${label}`);
      result.labels.push(label);
      result.rawBedLabels.push(label);
    } else if (type === "Objects") {
      const id = assignment.channel;
      result.labels.push(`Obj_${id}`);
      result.objectChannels.push({ id, channel: assignment.channel });
      result.events.push(...objectEvents(id, blocks, context, sampleRate));
    } else fail(`unsupported audioChannelFormat type ${type ?? "unknown"}`);
  }
  result.events.sort((left, right) => left.samplePos - right.samplePos || left.id - right.id);
  return result;
}
