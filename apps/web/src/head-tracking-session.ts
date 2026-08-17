import {
  relativeQuaternion,
  yawQuaternion,
  type HeadPose,
  type Quaternion,
} from "@sda/renderer";

/** Keeps the listener's forward reference stable across player/renderer replacement. */
export class HeadTrackingSession {
  private origin: Quaternion | null = null;
  private rawPose: HeadPose | null = null;
  private renderedPose: HeadPose | null = null;

  get latestPose(): HeadPose | null {
    return this.renderedPose;
  }

  update(pose: HeadPose): HeadPose {
    this.rawPose = pose;
    this.renderedPose = this.relativePose(pose);
    return this.renderedPose;
  }

  recenter(pose: HeadPose | null = this.rawPose): HeadPose | null {
    if (!pose) return null;
    this.rawPose = pose;
    this.origin = pose.orientation;
    this.renderedPose = this.relativePose(pose);
    return this.renderedPose;
  }

  clear(): void {
    this.origin = null;
    this.rawPose = null;
    this.renderedPose = null;
  }

  private relativePose(pose: HeadPose): HeadPose {
    if (!this.origin) return pose;
    // SDA renders AirPods in yaw-only mode. Extract yaw before composing the
    // persistent origin so pitch/roll cannot leak into the horizontal anchor.
    const orientation = relativeQuaternion(
      yawQuaternion(this.origin),
      yawQuaternion(pose.orientation),
    );
    return orientation ? { ...pose, orientation } : pose;
  }
}
