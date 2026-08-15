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
    source: "mock" | "external-helper";
    detail: string;
  }

  interface HeadTrackingPose {
    timestampMs: number;
    orientation: { x: number; y: number; z: number; w: number };
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
      startHeadTracking?: () => Promise<HeadTrackingStatus>;
      stopHeadTracking?: () => Promise<HeadTrackingStatus>;
      recenterHeadTracking?: () => Promise<HeadTrackingPose>;
      onHeadTrackingStatus?: (callback: (status: HeadTrackingStatus) => void) => () => void;
      onHeadTrackingPose?: (callback: (pose: HeadTrackingPose) => void) => () => void;
      onHeadTrackingRecenter?: (callback: (pose: HeadTrackingPose) => void) => () => void;
      pickFile?: () => Promise<string | null>;
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
