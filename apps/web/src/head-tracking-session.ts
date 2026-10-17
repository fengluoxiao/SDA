import {
  relativeQuaternion,
  HeadPoseTracker,
  type HeadPoseOptions,
  normalizeQuaternion,
  type HeadPose,
  type Quaternion,
} from "../../../packages/renderer/src/head-pose";

// The AirPods helper emits yaw(Z) * pitch(X). Extract its Z twist rather
// than projecting forward: spurious pitch beyond 90 degrees flips that
// projection by 180 degrees even though the yaw has not changed.
function trackingYaw(q: Quaternion): Quaternion {
  return normalizeQuaternion([0,0,q[2],q[3]]) ?? [0,0,0,1];
}

/** Stabilizes tracking and establishes a fresh forward reference for each song. */
export class HeadTrackingSession {
  private readonly tracker: HeadPoseTracker;
  private readonly rejectSpikes: boolean;
  private yawSamples: number[]=[];
  private sampleTime=Number.NEGATIVE_INFINITY;
  constructor(options: HeadPoseOptions = {smoothingMs:0,deadZoneDegrees:0,maxDegreesPerSecond:1e9}) {
    this.rejectSpikes=(options.smoothingMs??45)>0;
    this.tracker=new HeadPoseTracker({...options,fixedForward:true});
  }
  sample(nowMs=performance.now()): HeadPose | null {
    this.renderedPose=this.tracker.currentPose(nowMs);
    return this.renderedPose;
  }
  private centerNextPose = false;
  private startupYawSamples: number[] = [];
  private origin: Quaternion | null = null;
  private rawPose: HeadPose | null = null;
  private renderedPose: HeadPose | null = null;

  get latestPose(): HeadPose | null {
    return this.renderedPose;
  }

  update(pose: HeadPose): HeadPose {
    if (this.centerNextPose) {
      // Never anchor a new song to an idle/stale sample or a single noisy
      // packet. Stay exactly neutral until fresh provider samples establish it.
      const q = trackingYaw(pose.orientation);
      const yaw = 2 * Math.atan2(q[2], q[3]);
      const previous = this.startupYawSamples.at(-1) ?? yaw;
      this.startupYawSamples.push(previous + Math.atan2(Math.sin(yaw-previous), Math.cos(yaw-previous)));
      this.rawPose = pose;
      if (this.startupYawSamples.length < (this.rejectSpikes ? 3 : 1)) {
        this.renderedPose = {...pose, orientation:[0,0,0,1]};
        return this.renderedPose;
      }
      const values = [...this.startupYawSamples].sort((a,b)=>a-b);
      const center = values[Math.floor(values.length/2)]!;
      this.origin = [0,0,Math.sin(center/2),Math.cos(center/2)];
      this.centerNextPose = false;
      this.startupYawSamples = [];
      this.yawSamples = [0,0,0];
      this.sampleTime = pose.timestampMs ?? performance.now();
    }
    this.rawPose = pose;
    let relative=this.relativePose(pose);
    const now=pose.timestampMs??performance.now();
    if(this.rejectSpikes){
      const q=relative.orientation;
      const yaw=2*Math.atan2(q[2],q[3]);
      if(now-this.sampleTime>250 || this.yawSamples.length===0)this.yawSamples=[yaw,yaw,yaw];
      else {
        const reference=this.yawSamples[this.yawSamples.length-1]!;
        this.yawSamples.push(reference+Math.atan2(Math.sin(yaw-reference),Math.cos(yaw-reference)));
        this.yawSamples.shift();
      }
      this.sampleTime=now;
      const median=[...this.yawSamples].sort((a,b)=>a-b)[1]!;
      relative={...relative,orientation:[0,0,Math.sin(median/2),Math.cos(median/2)]};
    }
    this.tracker.set(relative,now);
    this.renderedPose=this.tracker.currentPose(now)??relative;
    return this.renderedPose;
  }

  /** Start neutral, then establish forward from fresh poses; seek/pause do not recenter. */
  beginPlayback(): HeadPose | null {
    this.tracker.clear();this.yawSamples=[];this.sampleTime=Number.NEGATIVE_INFINITY;
    this.origin=null;this.rawPose=null;this.startupYawSamples=[];
    this.renderedPose=null;this.centerNextPose=true;
    return null;
  }

  recenter(pose: HeadPose | null = this.rawPose): HeadPose | null {
    if (!pose) return null;
    this.centerNextPose = false;
    this.startupYawSamples = [];
    this.rawPose = pose;
    this.origin = pose.orientation;
    this.tracker.clear();this.yawSamples=[];this.sampleTime=Number.NEGATIVE_INFINITY;
    return this.update(pose);
  }

  clear(): void {
    this.tracker.clear();this.yawSamples=[];this.sampleTime=Number.NEGATIVE_INFINITY;
    this.origin = null;
    this.centerNextPose = false;
    this.startupYawSamples = [];
    this.rawPose = null;
    this.renderedPose = null;
  }

  private relativePose(pose: HeadPose): HeadPose {
    if (!this.origin) return {...pose,orientation:trackingYaw(pose.orientation)};
    // SDA renders AirPods in yaw-only mode. Extract yaw before composing the
    // persistent origin so pitch/roll cannot leak into the horizontal anchor.
    const orientation = relativeQuaternion(
      trackingYaw(this.origin),
      trackingYaw(pose.orientation),
    );
    return orientation ? { ...pose, orientation } : pose;
  }
}
