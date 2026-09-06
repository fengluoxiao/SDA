import type { VirtualSpeaker } from "@sda/renderer";
import type { CinemaSpeakerCalibration } from "./vite-env";

// Ray/box intersection in a centered 6 x 4 x 2.8 m room, ear height 1.2 m.
// Preserve each layout's directions; this is a hypothetical calibration table,
// not a measured BRIR or a change to the renderer's propagation model.
export function simulatedRoomCalibration(speakers: readonly VirtualSpeaker[]): Record<string, CinemaSpeakerCalibration> {
  const distances = speakers.map(speaker => {
    const az = speaker.azimuth * Math.PI / 180;
    const el = speaker.elevation * Math.PI / 180;
    const ray: [number,number,number] = [Math.sin(az) * Math.cos(el), Math.cos(az) * Math.cos(el), Math.sin(el)];
    const bounds = [2, 3, ray[2] < 0 ? 1.2 : 1.6];
    return Math.min(...ray.map((v,i)=>Math.abs(v)<1e-9?Infinity:bounds[i]!/Math.abs(v)));
  });
  const farthest = Math.max(...distances);
  const round = (value:number) => Math.round(value * 100) / 100;
  return Object.fromEntries(speakers.map((speaker,i)=>[speaker.name, {
    gainDb: round(20 * Math.log10(distances[i]! / farthest)),
    delayMs: round((farthest - distances[i]!) / 343 * 1000),
    lowDb: 0,
    highDb: 0,
  }]));
}
