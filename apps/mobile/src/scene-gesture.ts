// Match desktop OrbitControls: rotateSpeed=0.9, normalized to viewport height.
export function rotateScene(rotation: { x: number; y: number }, dx: number, dy: number, height: number) {
  const scale = 2 * Math.PI * 0.9 / Math.max(1, height);
  const basePolar = Math.PI / 2.9;
  return {
    x: rotation.x - dx * scale,
    y: Math.max(0.01 - basePolar, Math.min(Math.PI - 0.01 - basePolar, rotation.y - dy * scale)),
  };
}
