export type Quaternion = readonly [number, number, number, number];

export interface AngularVelocity {
  x: number;
  y: number;
  z: number;
}

export interface EulerAngles {
  yaw: number;
  pitch: number;
  roll: number;
}

export interface HeadTrackingTelemetrySample extends AngularVelocity, EulerAngles {
  timestampMs: number;
}

const RADIANS_TO_DEGREES = 180 / Math.PI;

export function normalizeQuaternion(value: Quaternion): Quaternion | null {
  const magnitude = Math.hypot(value[0], value[1], value[2], value[3]);
  if (!Number.isFinite(magnitude) || magnitude < 1e-8) return null;
  return [value[0] / magnitude, value[1] / magnitude, value[2] / magnitude, value[3] / magnitude];
}

function conjugate(value: Quaternion): Quaternion {
  return [-value[0], -value[1], -value[2], value[3]];
}

function multiply(left: Quaternion, right: Quaternion): Quaternion {
  const [lx, ly, lz, lw] = left;
  const [rx, ry, rz, rw] = right;
  return [
    lw * rx + lx * rw + ly * rz - lz * ry,
    lw * ry - lx * rz + ly * rw + lz * rx,
    lw * rz + lx * ry - ly * rx + lz * rw,
    lw * rw - lx * rx - ly * ry - lz * rz,
  ];
}

/** Local-axis angular velocity derived from two head-to-world orientations. */
export function quaternionAngularVelocity(
  previous: Quaternion,
  current: Quaternion,
  deltaMs: number,
): AngularVelocity {
  if (!Number.isFinite(deltaMs) || deltaMs <= 0) return { x: 0, y: 0, z: 0 };
  const from = normalizeQuaternion(previous);
  const to = normalizeQuaternion(current);
  if (!from || !to) return { x: 0, y: 0, z: 0 };

  let delta = multiply(conjugate(from), to);
  if (delta[3] < 0) delta = [-delta[0], -delta[1], -delta[2], -delta[3]];
  const vectorLength = Math.hypot(delta[0], delta[1], delta[2]);
  if (vectorLength < 1e-8) return { x: 0, y: 0, z: 0 };
  const angle = 2 * Math.atan2(vectorLength, Math.min(1, Math.max(-1, delta[3])));
  const degreesPerSecond = angle * RADIANS_TO_DEGREES * (1000 / deltaMs) / vectorLength;
  return {
    x: delta[0] * degreesPerSecond,
    y: delta[1] * degreesPerSecond,
    z: delta[2] * degreesPerSecond,
  };
}

/** ZYX Euler angles for display only; rendering continues to use quaternions. */
export function quaternionEulerAngles(value: Quaternion): EulerAngles {
  const orientation = normalizeQuaternion(value) ?? [0, 0, 0, 1];
  const [x, y, z, w] = orientation;
  return {
    yaw: Math.atan2(2 * (w * z + x * y), 1 - 2 * (y * y + z * z)) * RADIANS_TO_DEGREES,
    pitch: Math.asin(Math.min(1, Math.max(-1, 2 * (w * y - z * x)))) * RADIANS_TO_DEGREES,
    roll: Math.atan2(2 * (w * x + y * z), 1 - 2 * (x * x + y * y)) * RADIANS_TO_DEGREES,
  };
}
