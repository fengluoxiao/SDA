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
  /** Multiplier applied to rotation around the centered listener after recentering. */
  sensitivity?: number;
  /** Exponential smoothing time constant in milliseconds. Zero disables it. */
  smoothingMs?: number;
  /** Ignore measured orientation changes smaller than this many degrees. */
  deadZoneDegrees?: number;
  /** Maximum accepted turn speed in degrees per second. */
  maxDegreesPerSecond?: number;
  /** A pose older than this is treated as unavailable. */
  staleAfterMs?: number;
  /** Maximum pose-driven gain update rate. */
  updateHz?: number;
}

export const DEFAULT_HEAD_POSE_OPTIONS: Required<HeadPoseOptions> = {
  yawMode: "yaw",
  sensitivity: 1,
  smoothingMs: 45,
  deadZoneDegrees: 0.35,
  maxDegreesPerSecond: 360,
  staleAfterMs: 750,
  updateHz: 60,
};

/** Drift-anchor easing. Providers without a second inertial reference wander
 *  with the sensor's integrated bias (observed up to ±2°/s during playback),
 *  so once no real motion has been confirmed for a while the attitude eases
 *  back toward the most recently recentered front. The rate dominates every
 *  drift seen in the field while staying far below deliberate head turns. */
const ANCHOR_EASE_DEGREES_PER_SECOND = 2.5;
/** Real turns keep the anchor paused for this long before the ease resumes. */
const REAL_MOTION_HOLD_MS = 8000;
/** Angular speed above which a pose change counts as a deliberate turn. */
const REAL_MOTION_SPEED_DEG_PER_S = 30;
/** The neck cannot yaw past ~±90°; any attitude beyond this from the
 *  recentered front is a device/fit artifact, not a head pose. Beyond it the
 *  target is clamped and real-motion marking stops so the anchor ease can
 *  pull the image home instead of holding an impossible turn. */
const ANATOMICAL_LIMIT_DEGREES = 120;
/** The front "safe zone": within this of the recentered anchor and with no
 *  recent deliberate turn, the target snaps to the exact anchor — small
 *  wiggles, jaw motion and sensor tremor do not move the image at all, the
 *  same guarded feel Apple's fused implementation has. */
const ANCHOR_ZONE_DEGREES = 15;

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

/** Orientation of `current` relative to a persistent head-to-world origin. */
export function relativeQuaternion(
  origin: Quaternion,
  current: Quaternion,
): [number, number, number, number] | null {
  const normalizedOrigin = normalizeQuaternion(origin);
  const normalizedCurrent = normalizeQuaternion(current);
  if (!normalizedOrigin || !normalizedCurrent) return null;
  return normalizeQuaternion(multiplyQuaternion(invertQuaternion(normalizedOrigin), normalizedCurrent));
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

function scaleQuaternionAngle(q: Quaternion, scale: number): [number, number, number, number] {
  let [x, y, z, w] = q;
  // q and -q encode the same rotation. Use the shortest representation before
  // scaling so a small physical turn can never become an almost-full revolution.
  if (w < 0) { x = -x; y = -y; z = -z; w = -w; }
  const halfAngle = Math.acos(Math.min(1, Math.max(-1, w)));
  const axisLength = Math.hypot(x, y, z);
  if (axisLength < 1e-8 || halfAngle < 1e-8) return [0, 0, 0, 1];
  const scaledHalfAngle = halfAngle * scale;
  const axisScale = Math.sin(scaledHalfAngle) / axisLength;
  return [x * axisScale, y * axisScale, z * axisScale, Math.cos(scaledHalfAngle)];
}

/**
 * Scalar constant-velocity Kalman filter for the yaw channel (yaw angle +
 * yaw rate, with a full covariance update). It replaces a fixed exponential
 * smoother: provider samples get weighted by their estimated noise while the
 * process model carries fast turns through the rate state, so tracking is
 * both quicker on real turns and steadier at rest. Outlier samples inflate
 * the measurement variance instead of kicking the estimate.
 */
class YawKalmanFilter {
  private theta = 0;
  private omega = 0;
  private p00 = 1;
  private p01 = 0;
  private p11 = 1;
  /** Rate random-walk intensity. With R below, the steady-state bandwidth is
   *  ~(qRate/R)^(1/3) ≈ 1 Hz: provider sample noise averages out, while the
   *  initial covariance spike still snaps onto real step turns. */
  private readonly qRate = 0.5;
  /** One provider sample's angular noise (~2.5°). */
  private readonly rMeas = (2.5 * Math.PI / 180) ** 2;

  reset(yawRadians: number): void {
    this.theta = yawRadians;
    this.omega = 0;
    this.p00 = 1;
    this.p01 = 0;
    this.p11 = 1;
  }

  /** Filtered yaw in the continuous (unwrapped) domain. */
  update(measurement: number, dtSeconds: number): number {
    const dt = Math.min(0.1, Math.max(0.001, dtSeconds));
    this.theta += this.omega * dt;
    this.p00 += 2 * dt * this.p01 + dt * dt * this.p11 + (dt ** 3 / 3) * this.qRate;
    this.p01 += dt * this.p11 + (dt * dt / 2) * this.qRate;
    this.p11 += dt * this.qRate;

    let innovation = measurement - this.theta;
    innovation = Math.atan2(Math.sin(innovation), Math.cos(innovation));
    // Outlier gating: a >4 sigma innovation is a burst, not a turn — measure
    // it at 16x variance so it barely moves the estimate.
    const variance = this.p00 + this.rMeas;
    const r = innovation * innovation > 16 * variance ? this.rMeas * 16 : this.rMeas;
    const s = this.p00 + r;
    const k0 = this.p00 / s;
    const k1 = this.p01 / s;
    this.theta += k0 * innovation;
    this.omega += k1 * innovation;
    const p00 = this.p00;
    const p01 = this.p01;
    this.p00 = p00 - k0 * p00;
    this.p01 = p01 - k0 * p01;
    this.p11 = this.p11 - k1 * p01;
    return this.theta;
  }
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

/** Yaw angle of a yaw-only quaternion `[0, 0, z, w]`, in radians. */
function quaternionYaw(q: Quaternion): number {
  return 2 * Math.atan2(q[2], q[3]);
}

/** Pure pose state machine, intentionally usable in deterministic tests. */
export class HeadPoseTracker {
  readonly options: Required<HeadPoseOptions>;
  private orientation: [number, number, number, number] = [0, 0, 0, 1];
  private targetOrientation: [number, number, number, number] = [0, 0, 0, 1];
  private recenterOrientation: [number, number, number, number] = [0, 0, 0, 1];
  /** The attitude the ease-back anchor pulls toward: the most recent
   *  recentered front, or the first pose before any recenter happened. */
  private anchorOrientation: [number, number, number, number] = [0, 0, 0, 1];
  private lastRealMotionMs = Number.NEGATIVE_INFINITY;
  /** Previous raw provider attitude: deliberate-turn detection must compare
   *  device samples to each other, never to the eased target — the ease's own
   *  pull would otherwise register as real motion and pause itself forever. */
  private providerOrientation: [number, number, number, number] = [0, 0, 0, 1];
  private acceptedProviderOrientation: [number, number, number, number] = [0, 0, 0, 1];
  private providerMs = Number.NEGATIVE_INFINITY;
  private readonly kalman = new YawKalmanFilter();
  private receivedAtMs = Number.NEGATIVE_INFINITY;
  private lastUpdateMs = Number.NEGATIVE_INFINITY;
  private active = false;

  constructor(options: HeadPoseOptions = {}) {
    this.options = {
      yawMode: options.yawMode ?? DEFAULT_HEAD_POSE_OPTIONS.yawMode,
      sensitivity: Math.min(4, Math.max(0.1, Number.isFinite(options.sensitivity) ? options.sensitivity! : DEFAULT_HEAD_POSE_OPTIONS.sensitivity)),
      smoothingMs: Math.max(0, Number.isFinite(options.smoothingMs) ? options.smoothingMs! : DEFAULT_HEAD_POSE_OPTIONS.smoothingMs),
      deadZoneDegrees: Math.max(0, Number.isFinite(options.deadZoneDegrees) ? options.deadZoneDegrees! : DEFAULT_HEAD_POSE_OPTIONS.deadZoneDegrees),
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
      this.targetOrientation = target;
      this.anchorOrientation = target;
      this.providerOrientation = target;
      this.acceptedProviderOrientation = target;
      this.providerMs = nowMs;
      this.kalman.reset(quaternionYaw(target));
      this.active = true;
      this.lastUpdateMs = nowMs;
      this.lastRealMotionMs = nowMs;
    } else {
      // Integrate the provider-relative step into the target instead of
      // overwriting it: the anchor ease then compounds across samples rather
      // than being wiped by every absolute provider attitude. Sub-dead-zone
      // motion stays pending against the last accepted attitude, so slow
      // deliberate turns still accumulate past the dead zone.
      const stepQuat = multiplyQuaternion(invertQuaternion(this.acceptedProviderOrientation), target);
      // The step's angle lives in the quaternion's real part: |q| is always 1.
      const stepAngle = 2 * Math.acos(Math.min(1, Math.abs(stepQuat[3])));
      // Deliberate-turn speed is measured between consecutive provider
      // samples. The accepted reference goes stale while the ease pulls the
      // target away, and the gap then reads as phantom fast motion.
      const speedQuat = multiplyQuaternion(invertQuaternion(this.providerOrientation), target);
      const speedAngle = 2 * Math.acos(Math.min(1, Math.abs(speedQuat[3])));
      const providerS = Math.max(1, nowMs - this.providerMs) / 1000;
      // Real turns mark motion only while the attitude stays within the
      // anatomical limit (with hysteresis so a clamped target cannot flip-flop
      // the hold); an artifact pinned at the limit must not keep refreshing
      // the hold, or the anchor ease could never resume.
      if (speedAngle / providerS * 180 / Math.PI > REAL_MOTION_SPEED_DEG_PER_S
        && this.anchorAngleOf(this.targetOrientation) < (ANATOMICAL_LIMIT_DEGREES - 2) * Math.PI / 180) {
        this.lastRealMotionMs = nowMs;
      }
      const candidateQuat = multiplyQuaternion(invertQuaternion(this.targetOrientation), target);
      const candidateAngle = 2 * Math.acos(Math.min(1, Math.abs(candidateQuat[3])));
      if (candidateAngle >= this.options.deadZoneDegrees * Math.PI / 180) {
        const integrated = normalizeQuaternion(multiplyQuaternion(this.targetOrientation, stepQuat));
        if (integrated) this.targetOrientation = integrated;
        this.acceptedProviderOrientation = target;
      }
      this.providerOrientation = target;
      this.providerMs = nowMs;
      this.advance(nowMs);
    }
    this.receivedAtMs = nowMs;
    return true;
  }

  private advance(nowMs: number): void {
    const elapsedMs = Math.max(0, nowMs - this.lastUpdateMs);
    this.lastUpdateMs = nowMs;
    if (elapsedMs <= 0) return;
    if (this.options.yawMode === "yaw" && this.options.smoothingMs > 0) {
      // Kalman-filtered yaw chase: the filter carries a rate state through
      // real turns and down-weights noisy samples; the rate limit still caps
      // the applied step per advance.
      const currentYaw = quaternionYaw(this.orientation);
      const filteredYaw = this.kalman.update(quaternionYaw(this.targetOrientation), elapsedMs / 1000);
      const maxStep = this.options.maxDegreesPerSecond * Math.PI / 180 * elapsedMs / 1000;
      let step = filteredYaw - currentYaw;
      step = Math.atan2(Math.sin(step), Math.cos(step));
      const clamped = Math.abs(step) > maxStep ? Math.sign(step) * maxStep : step;
      const nextYaw = currentYaw + clamped;
      this.orientation = [0, 0, Math.sin(nextYaw / 2), Math.cos(nextYaw / 2)];
    } else {
      const target = this.targetOrientation;
      const dot = Math.min(1, Math.abs(this.orientation[0] * target[0] + this.orientation[1] * target[1] + this.orientation[2] * target[2] + this.orientation[3] * target[3]));
      const angle = 2 * Math.acos(dot);
      const rateLimit = this.options.maxDegreesPerSecond * Math.PI / 180 * elapsedMs / 1000;
      const rateT = angle < 1e-8 ? 1 : Math.min(1, rateLimit / angle);
      const smoothT = this.options.smoothingMs <= 0 ? 1 : 1 - Math.exp(-elapsedMs / this.options.smoothingMs);
      this.orientation = slerp(this.orientation, target, Math.min(rateT, smoothT));
    }
    // Ease the target back toward the anchored front once no deliberate turn
    // has been seen for a while, so provider drift cannot walk the image away.
    if (nowMs - this.lastRealMotionMs > REAL_MOTION_HOLD_MS && this.targetOrientation !== this.anchorOrientation) {
      const anchorDot = Math.min(1, Math.abs(
        this.targetOrientation[0] * this.anchorOrientation[0]
        + this.targetOrientation[1] * this.anchorOrientation[1]
        + this.targetOrientation[2] * this.anchorOrientation[2]
        + this.targetOrientation[3] * this.anchorOrientation[3],
      ));
      const anchorAngle = 2 * Math.acos(anchorDot);
      // Inside the front safe zone, hold the exact anchor: small wiggles and
      // jaw-driven bud rotation never move the image at all.
      if (anchorAngle < ANCHOR_ZONE_DEGREES * Math.PI / 180) {
        this.targetOrientation = this.anchorOrientation;
      } else if (anchorAngle > 1e-8) {
        const easeT = Math.min(1, ANCHOR_EASE_DEGREES_PER_SECOND * Math.PI / 180 * elapsedMs / 1000 / anchorAngle);
        this.targetOrientation = slerp(this.targetOrientation, this.anchorOrientation, easeT);
      }
    }
    // Anatomical cap: the neck cannot yaw past ~±90°, so any attitude beyond
    // the limit from the recentered front is a device/fit artifact, not a
    // head pose. Clamp the target to the boundary.
    const limitRad = ANATOMICAL_LIMIT_DEGREES * Math.PI / 180;
    const limitDot = Math.min(1, Math.abs(
      this.targetOrientation[0] * this.anchorOrientation[0]
      + this.targetOrientation[1] * this.anchorOrientation[1]
      + this.targetOrientation[2] * this.anchorOrientation[2]
      + this.targetOrientation[3] * this.anchorOrientation[3],
    ));
    const limitAngle = 2 * Math.acos(limitDot);
    if (limitAngle > limitRad) {
      this.targetOrientation = slerp(this.anchorOrientation, this.targetOrientation, limitRad / limitAngle);
    }
  }

  /** Angle of a quaternion from the anchor, in radians. */
  private anchorAngleOf(q: Quaternion): number {
    const dot = Math.min(1, Math.abs(
      q[0] * this.anchorOrientation[0]
      + q[1] * this.anchorOrientation[1]
      + q[2] * this.anchorOrientation[2]
      + q[3] * this.anchorOrientation[3],
    ));
    return 2 * Math.acos(dot);
  }

  clear(): boolean {
    const changed = this.active;
    this.active = false;
    this.orientation = [0, 0, 0, 1];
    this.targetOrientation = [0, 0, 0, 1];
    this.recenterOrientation = [0, 0, 0, 1];
    this.anchorOrientation = [0, 0, 0, 1];
    this.providerOrientation = [0, 0, 0, 1];
    this.acceptedProviderOrientation = [0, 0, 0, 1];
    this.providerMs = Number.NEGATIVE_INFINITY;
    this.kalman.reset(0);
    this.lastUpdateMs = Number.NEGATIVE_INFINITY;
    return changed;
  }

  recenter(): boolean {
    if (!this.active) return false;
    // Recenter against the newest measured attitude rather than a deliberately
    // lagging render orientation, then snap the render state to exact neutral.
    this.orientation = this.targetOrientation;
    this.recenterOrientation = invertQuaternion(this.targetOrientation);
    // The anchor follows: the eased-back "front" is whatever the user most
    // recently confirmed with an explicit recenter. Hold from the last sample
    // so the ease cannot eat idle time the listener spent facing forward.
    this.anchorOrientation = this.targetOrientation;
    this.lastRealMotionMs = this.receivedAtMs;
    this.kalman.reset(quaternionYaw(this.targetOrientation));
    return true;
  }

  isActive(nowMs: number): boolean {
    if (this.active && nowMs - this.receivedAtMs > this.options.staleAfterMs) this.clear();
    return this.active;
  }

  /** Canonical world spherical position transformed into the current head frame. */
  headRelative(world: Spherical, nowMs: number): Spherical {
    if (!this.isActive(nowMs)) return world;
    // Core Motion supplies timestamped attitude samples, while rendering runs on
    // its own steady clock. Continue toward the latest sample on every render
    // tick so sparse or discontinuous provider updates never become HRTF jumps.
    this.advance(nowMs);
    // The listener/KU100 remains at the room origin. R0^-1 * R is only an
    // orientation relative to the recentered forward direction; applying its
    // inverse makes every world-locked source head-relative without translating
    // either the listener or the source.
    const neutralToHead = multiplyQuaternion(this.recenterOrientation, this.orientation);
    const sensitiveRotation = scaleQuaternionAngle(neutralToHead, this.options.sensitivity);
    const [x, y, z] = rotateAdmVector(invertQuaternion(sensitiveRotation), sphericalToAdm({ ...world, distance: 1 }));
    const direction = admToSpherical([x, y, z]);
    return { ...direction, distance: world.distance };
  }
}
