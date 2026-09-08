import assert from 'node:assert/strict';
import { applyZoneExclusion, speakerInZone } from '../src/adm-zone.ts';
import { LAYOUTS } from '../src/layouts.ts';
import { sameObjectTarget } from '../../player/src/control.ts';
const left = { type: 'cartesian', min: [-1, -1, -1], max: [-0.1, 1, 1] };
const rear = { type: 'polar', min: [150, -90], max: [-150, 90] };
assert.ok(speakerInZone({ azimuth: 180, elevation: 0 }, rear));
assert.ok(speakerInZone({ azimuth: -160, elevation: 0 }, rear));
assert.ok(speakerInZone({ azimuth: 0, elevation: 90 }, rear));
assert.ok(!speakerInZone({ azimuth: 0, elevation: 0 }, rear));
assert.ok(speakerInZone({ azimuth: 90, elevation: 0 }, left));
assert.ok(!speakerInZone({ azimuth: -90, elevation: 0 }, left));
for (const layout of Object.values(LAYOUTS)) {
  const gains = Float32Array.from(layout, speaker => speaker.isLfe ? 0 : 1 / Math.sqrt(layout.filter(s => !s.isLfe).length));
  const initial = [...gains];
  applyZoneExclusion(gains, layout, [{ type: 'cartesian', min: [-1, -1, -1], max: [1, 1, 1] }]);
  assert.deepEqual([...gains], initial, 'all excluded retains reference identity downmix');
  applyZoneExclusion(gains, layout, [left]);
  assert.ok(Math.abs(gains.reduce((sum, gain) => sum + gain * gain, 0) - 1) < 1e-6);
  layout.forEach((speaker, i) => { if (speakerInZone(speaker, left)) assert.equal(gains[i], 0); });
}
const front = [30, -30, 0].map(azimuth => ({ azimuth, elevation: 0, isLfe: false }));
const gains = new Float32Array([1, 0, 0]);
applyZoneExclusion(gains, front, [left]);
assert.deepEqual([...gains], [0, 0, 1], 'excluded left energy moves to nearest eligible front speaker');
const event = { hasPos: true, pos: [0, 1, 0], size: [0, 0, 0], gainDb: 0 };
assert.equal(sameObjectTarget(event, { ...event, zoneExclusion: [left] }), false);
assert.equal(sameObjectTarget({ ...event, zoneExclusion: [left] }, { ...event, zoneExclusion: [rear] }), false);
console.log('ADM zones: coordinates, wrapped angles, poles, all-excluded fallback, energy, layouts and event changes passed');
