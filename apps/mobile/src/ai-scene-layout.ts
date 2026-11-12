import type { AiSceneDescription } from "./ai-scene";
/** Bounded deterministic positions, regenerated only when the composition changes. */
export function aiSceneParticlePositions(scene: AiSceneDescription): Float32Array {
  let seed = scene.seed || 1;
  const random = () => { seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5; return (seed >>> 0) / 4294967296; };
  const positions = new Float32Array(scene.particles.count * 3);
  for (let i = 0; i < scene.particles.count; i++) {
    let x = (random() * 2 - 1) * scene.particles.spread[0], y = (random() * 2 - 1) * scene.particles.spread[1], z = (random() * 2 - 1) * scene.particles.spread[2];
    const distance = Math.hypot(x, y, z);
    if (distance < 4) { const factor = 4 / Math.max(.001, distance); x *= factor; y *= factor; z *= factor; }
    positions.set([x, y, z], i * 3);
  }
  return positions;
}
