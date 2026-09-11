/// <reference types="vite/client" />
export interface MediaBrowserSaved { recent:string[]; favorites:string[]; }
export interface MediaBrowserPlaces extends MediaBrowserSaved { places:{name:string;path:string}[];initial:string; }
export interface MediaBrowserDirectory { path:string;parent:string;entries:{name:string;path:string;directory:boolean}[]; }
export interface CinemaSpeakerCalibration { gainDb: number; delayMs: number; lowDb: number; highDb: number; }
export interface MonitorOutput { trimDb:number; delayMs:number; invert:boolean; muted:boolean; }
export interface MonitorSettings {
  hardware?: {enabled:boolean;inputDb:number;dacBits:number;lineRms:number;gainDb:number;railV:number;currentA:number;loadOhms:number;outputOhms:number;bandwidthHz:number};
  enabled:boolean; levelDb:number; dim:boolean; dimDb:number; muted:boolean;
  bassEnabled:boolean; crossoverHz:number; bassDb:number; outputs:Record<string,MonitorOutput>;
}
export interface CinemaSettings {
  monitor?: MonitorSettings;
  reflectionMode?: "direct"|"early"|"full";
  enabled: boolean; directDb: number; earlyDb: number; lateDb: number; earlyMs: number;
  bassEnabled: boolean; crossoverHz: number; bassDb: number; speakers: Record<string, CinemaSpeakerCalibration>;
}
export interface CinemaRoomSummary {
  builtin?: boolean;
  id: string; name: string; source: string; license: string; measurement: string; layout: string; sampleRate: number;
  limited: boolean; suggested: Record<string, CinemaSpeakerCalibration>;
  rows: { name: string; arrivalMs: number; itdMs: number; directEnergyDb: number; peak: number }[];
  simulation?: RoomSimulation;
}
export interface RoomSimulationConfig { layout:string; length:number; width:number; height:number; earHeight:number; placement:number; listeningDistance?:number; material:"studio"|"treated"|"living"|"reflective"|"rockwool_50mm_80kgm3"|"plasterboard"|"hard_surface"; order:number; }
export interface RoomSimulation {
  surfaces?:Record<string,{materialId:string;coverage:number;remainder:string;coeffs:number[]}>;
  studioDesign?:{nominalTargetSeconds:number;eyringSeconds:number[];nearFieldDistanceMetres:number;source:string;firstOrderEarlyReflections:{speaker:string;wall:string;delayMs:number;worstDb:number}[]};
  reference?:{kind:string;propagationReferenceMetres:number;absoluteSplCalibrated:boolean;makeupGainDb:number;};
  material?:{id:string;description:string;coeffs:number[];centerFreqs:number[];source:string;reference:string;coverage:string;};
  sourceModel?:string;
  revision?:number;
  engine:string; config:RoomSimulationConfig; listener:[number,number,number]; size:[number,number,number];
  positions:Record<string,[number,number,number]>;
  paths:Record<string,{wall:string;order:number;distance:number;arrivalMs:number;points:[number,number,number][]}[]>;
  comparison:{metric:string;energyDb:Record<"raw"|"calibrated"|"room",number>&Partial<Record<"direct"|"early",number>>;gainDb:Record<"raw"|"calibrated"|"room",number>&Partial<Record<"direct"|"early",number>>;limited:boolean};
}

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
    remoteSynchronized?: boolean;
    remoteSyncWaiting?: boolean;
    running: boolean;
    referenceMix: boolean;
    detail: string;
    samplePos?: number;
    outputActive?: boolean;
    hrtfReady?: boolean;
  }

  interface NativeRendererObjectActivity {
    ids: readonly number[];
  }

  interface Window {
    sdaDesktop?: {
      getRemotePairingKey?:()=>Promise<string>;
      getRemoteStatus?:()=>Promise<import("./remote-session").RemoteStatus>;
      remoteSession?:(action:"host"|"join"|"stop"|"localMute"|"deviceApprove"|"deviceReject"|"deviceRevoke"|"devicePermission"|"deviceDisconnect",value?:unknown)=>Promise<import("./remote-session").RemoteStatus>;
      nativeRendererEnd?:(sample:number)=>Promise<boolean>;
      remoteCommand?:(command:import("./remote-session").RemoteCommand)=>Promise<string>;
      publishRemoteScene?:(scene:import("./remote-session").RemoteScene|undefined)=>void;
      publishRemoteState?:(state:import("./remote-session").RemotePlayback)=>void;
      completeRemoteControl?:(id:string,error:string|null)=>void;
      onRemoteStatus?:(callback:(status:import("./remote-session").RemoteStatus)=>void)=>()=>void;
      onRemoteControl?:(callback:(command:import("./remote-session").RemoteCommand)=>void)=>()=>void;
      onRemoteSuspend?:(callback:(value?:{replaceOutput?:boolean}|null)=>void)=>()=>void;
      onRemoteResult?:(callback:(result:{id:string;error:string|null})=>void)=>()=>void;
      electron3D: boolean;
      browsePersonalHrtf?: NonNullable<Window["sdaDesktop"]>["browseMedia"];
      listPersonalHrtf?: () => Promise<{id:string;name:string;directions:number;method:string}[]>;
      renamePersonalHrtf?: (id:string,name:string)=>Promise<{id:string;name:string;directions:number;method:string}>;
      personalHrtfArchive?: (action:"copy"|"export",id:string,value:string)=>Promise<{id?:string;path?:string}>;
      generatePersonalHrtf?: (parameters:import("../../desktop/parametric-hrtf.mjs").PhrtfParameters,assessment?:unknown) => Promise<{id:string;name:string;method:string}>;
      importPersonalHrtf?: (sourcePath:string) => Promise<{id:string;name:string;directions:number;method:string}>;
      browseMedia?: {
        (action:"places"):Promise<MediaBrowserPlaces>;
        (action:"list",value:string):Promise<MediaBrowserDirectory>;
        (action:"favorite"|"unfavorite"|"forget",value:string):Promise<MediaBrowserSaved>;
        (action:"files",value:string[]):Promise<string[]>;
        (action:"folder",value:string):Promise<string[]>;
      };
      windowControl?: (action: "minimize" | "maximize" | "close") => Promise<void>;
      getWindowMaximized?: () => Promise<boolean>;
      onWindowMaximized?: (callback: (maximized: boolean) => void) => () => void;
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
      getOutputDevices?: () => Promise<import("./components/OutputPanel").OutputDevices>;
      setOutputDevice?: (settings: import("./components/OutputPanel").OutputSettings) => Promise<import("./components/OutputPanel").OutputDevices & {accepted:boolean}>;
      onOutputDevices?: (callback:(value:import("./components/OutputPanel").OutputDevices)=>void) => () => void;
      startNativeRenderer?: () => Promise<NativeRendererStatus>;
      stopNativeRenderer?: () => Promise<NativeRendererStatus>;
      getNativeRendererHealth?: () => Promise<NativeRendererStatus>;
      nativeRendererSource?: (source: { id: string; atSample: number; bedLabel?: string }) => Promise<boolean>;
      nativeRendererRemoveSource?: (id: string, atSample: number) => Promise<boolean>;
      nativeRendererEvents?: (events: readonly import("@sda/core").ObjectEvent[]) => Promise<boolean>;
      nativeRendererReset?: (origin: number) => Promise<boolean>;
      nativeRendererMuted?: (id: string, muted: boolean, atSample?: number) => Promise<boolean>;
      nativeRendererLfeMuted?: (muted: boolean) => Promise<boolean>;
      nativeRendererSpeakerMutes?: (names: string[], focus?: string[]) => Promise<boolean>;
      nativeRendererVolume?: (volume: number) => Promise<boolean>;
      nativeRendererProgramEnabled?: (enabled: boolean) => Promise<boolean>;
      nativeRendererProgramGain?: (gain: number, atSample?: number) => Promise<boolean>;
      nativeRendererBinauralEq?: (bands: { low: number; mid: number; high: number }, lowCut: boolean) => Promise<boolean>;
      nativeRendererHeadphoneProfile?: (id: string | null, source?: string) => Promise<boolean>;
      nativeRendererPose?: (orientation: readonly [number, number, number, number]) => Promise<boolean>;
      nativeRendererClearPose?: () => Promise<boolean>;
      nativeRendererHrtf?: (set: string, wetWeight: number) => Promise<boolean>;
      nativeRendererStereoMode?: (mode: "original" | "dry" | "room") => Promise<boolean>;
      getCinemaSettings?: () => Promise<{settings: CinemaSettings; profileId: string | null; error?: string}>;
      listCinemaRooms?: () => Promise<CinemaRoomSummary[]>;
      importCinemaRoom?: () => Promise<CinemaRoomSummary | null>;
      deleteCinemaRoom?: (id: string) => Promise<boolean>;
      exportCinemaReport?: (id: string) => Promise<boolean>;
      nativeRendererCinema?: (settings: CinemaSettings, profileId: string | null) => Promise<boolean>;
      roomLabStatus?: () => Promise<{available:boolean;running:boolean;current:number;total:number;error:string|null}>;
      roomLabGenerate?: (config:RoomSimulationConfig) => Promise<CinemaRoomSummary>;
      roomLabCancel?: () => Promise<boolean>;
      nativeRendererComparisonGain?: (gainDb:number) => Promise<boolean>;
      nativeRendererObjectHrtf?: (enabled: boolean) => Promise<boolean>;
      nativeRendererDirectionalHrtf?: (enabled:boolean) => Promise<boolean>;
      nativeRendererNearField?: (settings: {enabled:boolean;metresPerUnit:number}) => Promise<boolean>;
      nativeRendererSourceExtent?: (settings: {enabled:boolean;width:number;diffusion:number}) => Promise<boolean>;
      nativeRendererLayout?: (layout: import("@sda/renderer").LayoutId) => Promise<boolean>;
      nativeRendererOutputActive?: (active: boolean) => Promise<boolean>;
      nativeRendererStartAt?: (origin: number) => Promise<boolean>;
      nativeRendererPause?: (paused: boolean) => Promise<boolean>;
      nativeRendererFrame?: (samplePos: number, entries: readonly { id: string; samples: Float32Array }[], events?: readonly import("@sda/core").ObjectEvent[]) => Promise<{ accepted: boolean; samples: number; reason?: string }>;
      onNativeRendererStatus?: (callback: (status: NativeRendererStatus) => void) => () => void;
      onNativeRendererObjectActivity?: (callback: (activity: NativeRendererObjectActivity) => void) => () => void;
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
