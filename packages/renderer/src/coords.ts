/**
 * Coordinate conversions.
 *
 * Events from @sda/core are ADM cartesian: x+ = RIGHT, y+ = front, z+ = up,
 * each nominally in [-1, 1] (unit cube mapped onto the room). 这是
 * ITU-R BS.2076 / EBU EAR 的标准约定（EAR: cart(az=+90°) = [-1,0,0]，即
 * 极坐标 +az=左对应 cartesian X=-1），harletty-bridge 与 Omniphony
 * 也用它（"X: left(-) -> right(+)"）。
 *
 * Internally the renderer works in spherical: azimuth degrees
 * (0 = front, + = left, matching ITU/ADM polar), elevation degrees
 * (+ = up), distance (1 = reference speaker distance).
 */

export interface Spherical {
  /** Degrees: 0 = front, +90 = left, -90 = right, ±180 = behind. */
  azimuth: number;
  /** Degrees: +90 = zenith, -90 = nadir. */
  elevation: number;
  /** Reference distance 1.0 ≈ speaker ring radius. */
  distance: number;
}

/** ADM cartesian [x, y, z] → spherical. */
export function admToSpherical(pos: [number, number, number]): Spherical {
  const [x, y, z] = pos;
  // 不钳制到 1：房间角落的 ADM 距离可达 √3，供环外 inverse 距离增益使用
  // （VBAP 只用方向，不受距离影响）。上限 4 防异常值。
  const radius = Math.hypot(x, y, z);
  const distance = Math.min(4, radius);
  if (distance < 1e-6) return { azimuth: 0, elevation: 0, distance: 0 };
  return {
    // +az = 左 = cartesian X 负侧（EBU EAR: azimuth = -atan2(x, y)）
    azimuth: (-Math.atan2(x, y) * 180) / Math.PI,
    // Keep bearing independent of the capped display/range value, matching
    // the native renderer even for coordinates outside the reference cube.
    elevation: (Math.asin(Math.min(1, Math.max(-1, z / radius))) * 180) / Math.PI,
    distance,
  };
}

/** Spherical → ADM cartesian [x, y, z]. */
export function sphericalToAdm(s: Spherical): [number, number, number] {
  const az = (s.azimuth * Math.PI) / 180;
  const el = (s.elevation * Math.PI) / 180;
  const r = s.distance;
  // az=+90°（左）→ x=-1（EBU EAR: cart 用 sin(-az)）
  return [-r * Math.cos(el) * Math.sin(az), r * Math.cos(el) * Math.cos(az), r * Math.sin(el)];
}

/**
 * Spherical → Web Audio / three.js right-handed coordinates:
 * x+ = right, y+ = up, z+ = toward listener (behind the viewer is +z... in
 * Web Audio, the listener faces -z, so "front" is -z).
 */
export function sphericalToWebAudio(s: Spherical): [number, number, number] {
  const [x, y, z] = sphericalToAdm(s);
  // ADM x+ = right → WebAudio x+；ADM y+ = front → WebAudio z-；z+ = up stays y+.
  return [x, z, -y];
}
