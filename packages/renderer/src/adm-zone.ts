import type { AdmZone } from "../../core/src/adm-zone.js";
import type { VirtualSpeaker } from "./layouts.js";
import { sphericalToAdm } from "./coords.js";

const EPS = 1e-6;
const layer = (elevation: number) => elevation < -10 ? 0 : elevation < 10 ? 1 : elevation < 75 ? 2 : 3;
const layers = [[0, 1, 2, 3], [3, 0, 1, 2], [3, 2, 0, 1], [3, 2, 1, 0]];
const sign = (value: number) => value > EPS ? 1 : value < -EPS ? -1 : 0;

export function speakerInZone(speaker: Pick<VirtualSpeaker, "azimuth" | "elevation">, zone: AdmZone): boolean {
  if (zone.type === "cartesian") {
    const pos = sphericalToAdm({ ...speaker, distance: 1 });
    return pos.every((value, i) => value >= zone.min[i]! - EPS && value <= zone.max[i]! + EPS);
  }
  const { azimuth, elevation } = speaker;
  const span = zone.max[0] - zone.min[0];
  const offset = ((azimuth - zone.min[0]) % 360 + 360) % 360;
  const inAzimuth = Math.abs(elevation) >= 90 - EPS || span >= 360 - EPS
    || offset <= ((span % 360 + 360) % 360) + EPS || offset >= 360 - EPS;
  return inAzimuth && elevation >= zone.min[1] - EPS && elevation <= zone.max[1] + EPS;
}

/** Energy downmix follows EBU EAR's layer/front-back/distance priority rules. */
export function applyZoneExclusion(gains: Float32Array, layout: readonly VirtualSpeaker[], zones: readonly AdmZone[]): void {
  if (!zones.length) return;
  const candidates = layout.flatMap((speaker, i) => !speaker.isLfe ? [i] : []);
  const excluded = layout.map(speaker => !speaker.isLfe && zones.some(zone => speakerInZone(speaker, zone)));
  const allowed = candidates.filter(i => !excluded[i]);
  // The ADM reference downmix is identity when all speakers are excluded.
  if (!allowed.length || allowed.length === candidates.length) return;
  const positions = layout.map(speaker => sphericalToAdm({ ...speaker, distance: 1 }));
  const energy = Float64Array.from(gains, gain => gain * gain);
  for (const from of candidates) {
    if (!excluded[from]) continue;
    const a = positions[from]!;
    let best: number[] | undefined;
    let targets: number[] = [];
    for (const to of allowed) {
      const b = positions[to]!;
      const key = [layers[layer(layout[from]!.elevation)]![layer(layout[to]!.elevation)]!,
        Math.abs(sign(a[1]) - sign(b[1])), Math.hypot(...a.map((value, i) => value - b[i]!)), Math.abs(a[1] - b[1])];
      const difference = best ? key.findIndex((value, i) => Math.abs(value - best![i]!) >= EPS) : -1;
      if (!best || (difference >= 0 && key[difference]! < best[difference]!)) { best = key; targets = [to]; }
      else if (difference < 0) targets.push(to);
    }
    const share = gains[from]! ** 2 / targets.length;
    energy[from] = 0;
    for (const target of targets) energy[target] = energy[target]! + share;
  }
  for (let i = 0; i < gains.length; i++) gains[i] = Math.sqrt(energy[i]!);
}
