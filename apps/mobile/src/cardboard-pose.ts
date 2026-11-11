import { Euler, Quaternion, Vector3 } from "three";
const Z = new Vector3(0, 0, 1);
const SCREEN_TO_CAMERA = new Quaternion(-Math.sqrt(.5), 0, 0, Math.sqrt(.5));
/** Convert the actual Expo native Euler conventions into Three Y-up space. */
export class CardboardPose {
  readonly value = new Quaternion();
  private raw = new Quaternion();
  private center = new Quaternion();
  private euler = new Euler(0, 0, 0, "YXZ");
  private screen = new Quaternion();
  private forward = new Vector3();
  private centered = false;
  private orientation = NaN;
  private lastSample = 0;
  ready = false;
  update(rotation: { alpha: number; beta: number; gamma: number }, screenAngle: number, now: number, platform: "ios" | "android" = "android") {
    const { alpha, beta, gamma } = rotation;
    if (![alpha, beta, gamma, screenAngle, now].every(Number.isFinite)) return false;
    // CoreMotion yaw/pitch/roll is ZYX; Android getOrientation is ZXY.
    // Expo iOS reports UIDevice landscape directions; Android reports display
    // rotation. Their landscape signs are opposite, including a 180-degree flip.
    const displayRotation = platform === "ios" ? screenAngle : -screenAngle;
    this.raw.setFromEuler(platform === "ios" ? this.euler.set(gamma, beta, alpha, "ZYX") : this.euler.set(beta, gamma, alpha, "ZXY"))
      .premultiply(SCREEN_TO_CAMERA).multiply(this.screen.setFromAxisAngle(Z, displayRotation)).normalize();
    if (!this.centered || this.orientation !== screenAngle) {
      this.orientation = screenAngle; this.recenter(); this.value.copy(this.center).multiply(this.raw);
    } else {
      const dt = Math.max(0, Math.min(.1, (now - this.lastSample) / 1000));
      this.screen.copy(this.center).multiply(this.raw);
      this.value.slerp(this.screen, 1 - Math.exp(-dt / .012));
    }
    this.lastSample = now; this.ready = true; return true;
  }
  recenter() {
    // Reset heading only. Keep gravity/horizon and the user's physical pitch.
    this.forward.set(0, 0, -1).applyQuaternion(this.raw);
    const yaw = Math.atan2(-this.forward.x, -this.forward.z);
    this.center.setFromAxisAngle(new Vector3(0, 1, 0), -yaw);
    this.centered = true; this.value.copy(this.center).multiply(this.raw);
  }
  reset() { this.centered = false; this.ready = false; this.lastSample = 0; this.value.identity(); }
}

/** UIInterfaceOrientation is opposite UIDeviceOrientation in landscape. Keep
 * using the displayed interface when looking up/down makes UIDevice faceUp. */
export function cardboardIOSScreenAngle(interfaceOrientation: number): number | undefined {
  if (interfaceOrientation === 3) return Math.PI / 2;
  if (interfaceOrientation === 4) return -Math.PI / 2;
  return undefined;
}
