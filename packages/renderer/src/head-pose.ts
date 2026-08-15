/**
 * Device-neutral head-pose handling in SDA's ADM coordinate system.
 *
 * A quaternion is `[x, y, z, w]` and describes a rotation from head-local ADM
 * axes to world ADM axes. Providers must calibrate their device axes and
 * handedness before calling this API. World-locked source directions are made
 * head-relative with the inverse of this quaternion.
 */

import { sphericalToAdm, admToSpherical, type Spherical } from "./coords.js";

export type Quaternion = readonly [number, number, number, number];
export type HeadPoseYawMode = "full" | "yaw";

export interface HeadPose {
  /** Canonical ADM head-to-world orientation quaternion `[x, y, z, w]`. */
  orientation: Quaternion;
  /** Monotonic provider timestamp in the same time domain as `performance.now()`.
   * Omit to use the renderer receipt time. */
  timestampMs?: number;
}

export interface HeadPoseOptions {
  /** `yaw` ignores pitch and roll; `full` preserves all orientation axes. */
  yawMode?: HeadPoseYawMode;
  /** Exponential smoothing time constant in milliseconds. Zero disables it. */
  smoothingMs?: number;
  /** Maximum accepted turn speed in degrees per second. */
  maxDegreesPerSecond?: number;
  /** A pose older than this is treated as unavailable. */
  staleAfterMs?: number;
  /** Maximum pose-driven gain update rate. */
  updateHz?: number;
}

export const DEFAULT_HEAD_POSE_OPTIONS: Required<HeadPoseOptions> = {
  yawMode: "yaw",
  smoothingMs: 45,
  maxDegreesPerSecond: 360,
  staleAfterMs: 750,
  updateHz: 60,
};

type Vec3 = readonly [number, number, number];

function finite(value: number): boolean {
  return Number.isFinite(value);
}

export function normalizeQuaternion(q: Quaternion): [number, number, number, number] | null {
  if (!q.every(finite)) return null;
  const length = Math.hypot(q[0], q[1], q[2], q[3]);
  if (length < 1e-8) return null;
  return [q[0] / length, q[1] / length, q[2] / length, q[3] / length];
}

export function invertQuaternion(q: Quaternion): [number, number, number, number] {
  return [-q[0], -q[1], -q[2], q[3]];
}

function multiplyQuaternion(a: Quaternion, b: Quaternion): [number, number, number, number] {
  return [
    a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
    a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
    a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
    a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
  ];
}

export function rotateAdmVector(q: Quaternion, vector: Vec3): [number, number, number] {
  const rotated = multiplyQuaternion(multiplyQuaternion(q, [vector[0], vector[1], vector[2], 0]), invertQuaternion(q));
  return [rotated[0], rotated[1], rotated[2]];
}

function slerp(a: Quaternion, b: Quaternion, t: number): [number, number, number, number] {
  let bx = b[0]; let by = b[1]; let bz = b[2]; let bw = b[3];
  let dot = a[0] * bx + a[1] * by + a[2] * bz + a[3] * bw;
  if (dot < 0) { dot = -dot; bx = -bx; by = -by; bz = -bz; bw = -bw; }
  if (dot > 0.9995) {
    return normalizeQuaternion([a[0] + t * (bx - a[0]), a[1] + t * (by - a[1]), a[2] + t * (bz - a[2]), a[3] + t * (bw - a[3])])!;
  }
  const theta = Math.acos(Math.min(1, dot));
  const sinTheta = Math.sin(theta);
  const left = Math.sin((1 - t) * theta) / sinTheta;
  const right = Math.sin(t * theta) / sinTheta;
  return [left * a[0] + right * bx, left * a[1] + right * by, left * a[2] + right * bz, left * a[3] + right * bw];
}

/** Extract an ADM-up yaw quaternion. Positive yaw turns the head toward ADM left. */
export function yawQuaternion(q: Quaternion): [number, number, number, number] {
  const forward = rotateAdmVector(q, [0, 1, 0]);
  const length = Math.hypot(forward[0], forward[1]);
  if (length < 1e-8) return [0, 0, 0, 1];
  const yaw = Math.atan2(-forward[0], forward[1]);
  const z = Math.sin(yaw / 2);
  return [0, 0, z === 0 ? 0 : z, Math.cos(yaw / 2)];
}

/** Pure pose state machine, intentionally usable in deterministic tests. */
export class HeadPoseTracker {
  readonly options: Required<HeadPoseOptions>;
  private orientation: [number, number, number, number] = [0, 0, 0, 1];
  private recenterOrientation: [number, number, number, number] = [0, 0, 0, 1];
  private receivedAtMs = Number.NEGATIVE_INFINITY;
  private lastUpdateMs = Number.NEGATIVE_INFINITY;
  private active = false;

  constructor(options: HeadPoseOptions = {}) {
    this.options = {
      yawMode: options.yawMode ?? DEFAULT_HEAD_POSE_OPTIONS.yawMode,
      smoothingMs: Math.max(0, Number.isFinite(options.smoothingMs) ? options.smoothingMs! : DEFAULT_HEAD_POSE_OPTIONS.smoothingMs),
      maxDegreesPerSecond: Math.max(0, Number.isFinite(options.maxDegreesPerSecond) ? options.maxDegreesPerSecond! : DEFAULT_HEAD_POSE_OPTIONS.maxDegreesPerSecond),
      staleAfterMs: Math.max(1, Number.isFinite(options.staleAfterMs) ? options.staleAfterMs! : DEFAULT_HEAD_POSE_OPTIONS.staleAfterMs),
      updateHz: Math.max(1, Number.isFinite(options.updateHz) ? options.updateHz! : DEFAULT_HEAD_POSE_OPTIONS.updateHz),
    };
  }

  set(pose: HeadPose, nowMs: number): boolean {
    const normalized = normalizeQuaternion(pose.orientation);
    const timestamp = pose.timestampMs ?? nowMs;
    if (!normalized || !finite(timestamp) || timestamp > nowMs + 1000 || nowMs - timestamp > this.options.staleAfterMs) return false;
    const target = this.options.yawMode === "yaw" ? yawQuaternion(normalized) : normalized;
    if (!this.active) {
      this.orientation = target;
      this.active = true;
    } else {
      const elapsedMs = Math.max(0, nowMs - this.lastUpdateMs);
      const dot = Math.min(1, Math.abs(this.orientation[0] * target[0] + this.orientation[1] * target[1] + this.orientation[2] * target[2] + this.orientation[3] * target[3]));
      const angle = 2 * Math.acos(dot);
      const rateLimit = this.options.maxDegreesPerSecond * Math.PI / 180 * elapsedMs / 1000;
      const rateT = angle < 1e-8 ? 1 : Math.min(1, rateLimit / angle);
      const smoothT = this.options.smoothingMs <= 0 ? 1 : 1 - Math.exp(-elapsedMs / this.options.smoothingMs);
      this.orientation = slerp(this.orientation, target, Math.min(rateT, smoothT));
    }
    this.receivedAtMs = nowMs;
    this.lastUpdateMs = nowMs;
    return true;
  }

  clear(): boolean {
    const changed = this.active;
    this.active = false;
    this.orientation = [0, 0, 0, 1];
    this.recenterOrientation = [0, 0, 0, 1];
    return changed;
  }

  recenter(): boolean {
    if (!this.active) return false;
    this.recenterOrientation = invertQuaternion(this.orientation);
    return true;
  }

  isActive(nowMs: number): boolean {
    if (this.active && nowMs - this.receivedAtMs > this.options.staleAfterMs) this.clear();
    return this.active;
  }

  /** Canonical world spherical position transformed into the current head frame. */
  headRelative(world: Spherical, nowMs: number): Spherical {
    if (!this.isActive(nowMs)) return world;
    const headToWorld = multiplyQuaternion(this.orientation, this.recenterOrientation);
    const [x, y, z] = rotateAdmVector(invertQuaternion(headToWorld), sphericalToAdm({ ...world, distance: 1 }));
    const direction = admToSpherical([x, y, z]);
    return { ...direction, distance: world.distance };
  }
}
