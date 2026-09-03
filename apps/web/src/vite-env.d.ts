/// <reference types="vite/client" />

interface LocalHeadphoneProfileAsset {
  fileName: string;
  tapCount: number;
  sha256: string;
}

interface LocalHeadphoneProfileManifest {
  id: string;
  name: string;
  source: string;
  target: string;
  leftMeasurement: string;
  rightMeasurement: string;
  balanceEvidence: string;
  sampleRate: number;
  preampDb: number;
  leftFirUrl: string;
  rightFirUrl: string;
  schemaVersion: 1;
  measurementMode: "independent-lr" | "average-dual-mono";
  channelClaim: string;
  averageMeasurement?: string;
  derivation?: string;
  createdAt: string;
  deviceRevision: string;
  playbackState: string;
  earTips: string;
  firmware: string;
  measurementRig: string;
  referenceBand: string;
  leftFir: LocalHeadphoneProfileAsset;
  rightFir: LocalHeadphoneProfileAsset;
}

declare global {
  interface HeadTrackingStatus {
    running: boolean;
    source: "mock" | "bundled-helper" | "external-helper";
    detail: string;
  }

  interface HeadTrackingHelperConfiguration {
    configured: boolean;
    fileName: string | null;
    bundledAvailable: boolean;
    usingBundled: boolean;
    externalSelected: boolean;
    mockAvailable: boolean;
  }

  interface HeadTrackingPose {
    timestampMs: number;
    orientation: { x: number; y: number; z: number; w: number };
  }

  interface NativeRendererStatus {
    running: boolean;
    referenceMix: boolean;
    detail: string;
    samplePos?: number;
    outputActive?: boolean;
    hrtfReady?: boolean;
  }

  interface Window {
    sdaDesktop?: {
      electron3D: boolean;
      rendererMode: string;
      getOutputLatencySeconds?: () => 0.1 | 0.2 | 0.3;
      setOutputLatencySeconds?: (seconds: 0.1 | 0.2 | 0.3) => boolean;
      getVolumeBalanceEnabled?: () => boolean;
      setVolumeBalanceEnabled?: (enabled: boolean) => boolean;
      getHeadTrackingStatus?: () => Promise<HeadTrackingStatus>;
      getHeadTrackingHelper?: () => Promise<HeadTrackingHelperConfiguration>;
      selectHeadTrackingHelper?: () => Promise<HeadTrackingHelperConfiguration>;
      useBundledHeadTrackingHelper?: () => Promise<HeadTrackingHelperConfiguration>;
      startHeadTracking?: () => Promise<HeadTrackingStatus>;
      stopHeadTracking?: () => Promise<HeadTrackingStatus>;
      recenterHeadTracking?: () => Promise<HeadTrackingPose | null>;
      getNativeRendererStatus?: () => Promise<NativeRendererStatus>;
      startNativeRenderer?: () => Promise<NativeRendererStatus>;
      stopNativeRenderer?: () => Promise<NativeRendererStatus>;
      getNativeRendererHealth?: () => Promise<NativeRendererStatus>;
      nativeRendererSource?: (id: string, atSample: number) => Promise<boolean>;
      nativeRendererRemoveSource?: (id: string, atSample: number) => Promise<boolean>;
      nativeRendererEvents?: (events: readonly import("@sda/core").ObjectEvent[]) => Promise<boolean>;
      nativeRendererReset?: (origin: number) => Promise<boolean>;
      nativeRendererPose?: (orientation: readonly [number, number, number, number]) => Promise<boolean>;
      nativeRendererClearPose?: () => Promise<boolean>;
      nativeRendererHrtf?: (set: string, wetWeight: number) => Promise<boolean>;
      nativeRendererOutputActive?: (active: boolean) => Promise<boolean>;
      nativeRendererStartAt?: (origin: number) => Promise<boolean>;
      nativeRendererPause?: (paused: boolean) => Promise<boolean>;
      nativeRendererFrame?: (samplePos: number, entries: readonly { id: string; samples: Float32Array }[]) => Promise<{ accepted: boolean; samples: number; reason?: string }>;
      onNativeRendererStatus?: (callback: (status: NativeRendererStatus) => void) => () => void;
      onHeadTrackingStatus?: (callback: (status: HeadTrackingStatus) => void) => () => void;
      onHeadTrackingPose?: (callback: (pose: HeadTrackingPose) => void) => () => void;
      onHeadTrackingRecenter?: (callback: (pose: HeadTrackingPose) => void) => () => void;
      pickFile?: () => Promise<string | null>;
      pickFolder?: () => Promise<{ canceled: boolean; paths: string[] }>;
      openPath?: (filePath: string) => Promise<{ id: number; size: number; name: string }>;
      readSlice?: (id: number, offset: number, length: number) => Promise<Uint8Array>;
      close?: (id: number) => Promise<void>;
      readBundledHeadphoneFir?: (assetPath: string) => Promise<Uint8Array>;
      readBundledHrtf?: (assetPath: string) => Promise<Uint8Array>;
      importHeadphoneProfile?: () => Promise<{ profile: LocalHeadphoneProfileManifest; leftFir: Uint8Array; rightFir: Uint8Array } | null>;
      listHeadphoneProfiles?: () => Promise<LocalHeadphoneProfileManifest[]>;
      readHeadphoneProfile?: (id: string) => Promise<{ profile: LocalHeadphoneProfileManifest; leftFir: Uint8Array; rightFir: Uint8Array }>;
      deleteHeadphoneProfile?: (id: string) => Promise<void>;
      onOpenFile?: (callback: (filePath: string) => void) => () => void;
    };
  }
}

export {};
