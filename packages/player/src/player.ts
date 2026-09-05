/**
 * SdaPlayer — glues everything together:
 *
 *   file/stream ──push──▶ decoder worker (demux + wasm decode)
 *                         │  DecodedFrameData (PCM + events)
 *                         ▼
 *                    SpatialRenderer (AudioWorklet, VBAP → buses → out)
 *                         │
 *                         ▼ onVisualState (throttled) → 3D visualization
 *
 * Timing model: decoded PCM is pushed into per-source ring buffers ahead of
 * the playhead; `samplePos` on frames is the authoritative stream clock.
 * The player paces pushes so the renderer stays ~TARGET_AHEAD_SECONDS ahead.
 */

import {
  SpatialRenderer,
  LAYOUTS,
  getBinauralIrSet,
  headphoneProfileById,
  registerLocalHeadphoneCompensation,
  unregisterLocalHeadphoneCompensation,
  type LocalHeadphoneCompensationData,
  type BinauralMode,
  type BinauralIrSet,
  type BinauralEqBands,
  type BinauralLowFrequencyDiagnostic,
  type BinauralHealthTelemetry,
  type HeadPose,
  type HeadPoseOptions,
  type OutputMode,
  type LayoutId,
  type VirtualSpeaker,
} from "@sda/renderer";
import type { DecodedFrameData, FrameLoudness, ObjectChannelDecl, ObjectEvent, ProgramLoudnessMetadata } from "@sda/core";
import type { BinauralRenderMetadata } from "@sda/demux";
import { placeholderVisualObject, sameObjectTarget, visualObjectFromEvent, withoutPendingObjectEvents } from "./control.js";

export interface VisualObject {
  id: number;
  pos: [number, number, number]; // ADM cartesian
  hasPos: boolean;
  size: [number, number, number];
  gainDb: number;
  anchor: "room" | "screen" | "speaker";
  distanceM: number | null;
  distanceInfinite: boolean;
}

export interface PlayerHealthSnapshot {
  /** AudioContext output FIFO requested by the active playback session. */
  requestedOutputLatencySeconds: number;
  /** Recommended output FIFO retained for future playback sessions. A sustained
   * callback-gap pattern also upgrades the active context through safe replay. */
  nextRecommendedOutputLatencySeconds: number;
  /** Chromium/device-selected latency values for the active AudioContext. */
  baseLatencySeconds: number;
  outputLatencySeconds: number | null;
  audioContextSampleRate: number;
  /** True when Chromium selected substantially less base latency than requested. */
  outputLatencyHintLimited: boolean;
  /** Cumulative within the active playback session. */
  callbackGaps: number;
  underrunSamples: number;
  /** Rolling 2.5-second burst evidence eligible for active output-FIFO escalation. */
  callbackGapWindowEvents: number;
  callbackGapWindowTicks: number;
  /** Rolling five-second distributed-stall evidence. */
  callbackGapDistributedEvents: number;
  callbackGapDistributedTicks: number;
  /** Most recent escalation decision for operator-facing Electron diagnostics. */
  callbackGapEscalation: "none" | "distributed" | "burst" | "deferred-low-buffer";
  /** Current worklet reporting window (about 1/8 second). */
  tick: {
    callbackGaps: number;
    /** Callback gaps strictly above 25ms, eligible for adaptive FIFO evidence. */
    callbackGapsOver25Ms: number;
    callbackGapMaxMs: number;
    underrunSamples: number;
    rejectedBatches: number;
    rejectedSources: number;
    processMeanMs: number;
    processMaxMs: number;
  };
  /** Decoded audio seconds divided by wall-clock seconds over a sliding window. */
  decodeRealtimeMultiplier: number;
  /** PCM already accepted by the worklet, ahead of its consumption cursor. */
  fedBufferedSeconds: number;
  /** Decoded PCM waiting to be submitted to the worklet. */
  queuedSeconds: number;
  /** Current binaural bank graph; direct `off` paths are not convolutions. */
  binaural: BinauralHealthTelemetry;
}

export type OutputLatencySeconds = 0.1 | 0.2 | 0.3;

export interface PlayerCallbacks {
  /** Measured-loudness balance converged (or applied from cache) for the
   *  current track; the UI persists it so replays balance from sample 0. */
  onMeasuredLoudness?: (integratedLufs: number) => void;
  onTrack?: (info: { codec: string; sampleRate: number; channels: number; container: string; durationSec?: number; title?: string; coverArt?: { bytes: Uint8Array; mimeType: "image/jpeg" | "image/png" } }) => void;
  /** Program-level DBMD metadata. It never follows the sample event timeline. */
  onBinauralMetadata?: (metadata: BinauralRenderMetadata) => void;
  /** Decoded frame topology. Container channel_count can describe only an EC-3 core. */
  onDecodedFormat?: (info: { rawBedLabels: string[]; bedLabels: string[]; objectChannels: number }) => void;
  /** Throttled (~per frame batch) object-state snapshot for the 3D view. */
  onVisualState?: (objects: VisualObject[], streamTimeSec: number, soundingIds: ReadonlySet<number>) => void;
  onError?: (message: string) => void;
  /** Fired when the input ended and the renderer drained. */
  onEnded?: () => void;
  /** Throttled worklet and decoder health snapshot for the active playback session. */
  onHealth?: (health: PlayerHealthSnapshot) => void;
  /** A sustained callback-gap pattern upgrades the active AudioContext and
   * persists this latency for the next playback session. */
  onOutputLatencyRecommendation?: (seconds: OutputLatencySeconds) => void;
}

export interface NativeRendererSourceDeclaration {
  id: string;
  atSample: number;
  /** Present only for fixed bed channels. The native renderer uses this label
   * to select a room-locked virtual-speaker route or the dedicated LFE path. */
  bedLabel?: string;
}

/** Optional mirror transport to the native Rust sidecar. The player continues
 * feeding Web Audio until a later native-output mode explicitly takes ownership. */
export interface NativeRendererSink {
  /** Resolves only after the sidecar has created or rebound the source route. */
  addSource(source: NativeRendererSourceDeclaration): void | Promise<void>;
  removeSource(id: string, atSample: number): void | Promise<void>;
  setMuted(id: string, muted: boolean, atSample?: number): void | Promise<void>;
  setLfeMuted(muted: boolean): void | Promise<void>;
  setSpeakerMutes?(names: string[], focus?: string[]): void | Promise<void>;
  setVolume(volume: number): void | Promise<void>;
  setProgramEnabled(enabled: boolean): void | Promise<void>;
  setProgramGainDb(gainDb: number | null, atSample?: number): void | Promise<void>;
  setBinauralEq(bands: BinauralEqBands, lowCut: boolean): void | Promise<void>;
  setHeadphoneProfile(id: string | null): void | Promise<void>;
  events(events: readonly ObjectEvent[]): void | Promise<void>;
  /** Resolves only once the sidecar accepted or rejected the entire codec batch. */
  frame(samplePos: number, entries: readonly { id: string; samples: Float32Array }[]): void | Promise<{ accepted: boolean; samples: number; reason?: string }>;
  reset(origin: number): void | Promise<void>;
  setHeadPose(pose: HeadPose): void | Promise<void>;
  clearHeadPose(): void | Promise<void>;
  startAt(origin: number): void | Promise<boolean>;
  pause(paused: boolean): void | Promise<boolean>;
  /** Selects the master-defined virtual physical speaker layout. */
  setLayout(layout: LayoutId): void | Promise<void>;
  /** Optional native DAC consumption cursor on the codec sample clock. */
  getConsumedSamples?(): number;
  /** Subscribe to native consumption cursor updates; returns an optional unsubscribe. */
  onConsumedSamples?(callback: (sample: number) => void): void | (() => void);
  /** DAC-aligned post-source-gain/post-mute object activity from the native worker. */
  onObjectActivity?(callback: (ids: readonly number[]) => void): void | (() => void);
}

export type OutputBackend = "web-audio" | "native-sidecar";

export interface SdaPlayerOptions {
  /** Validated output FIFO setting to use when this player creates its first
   * AudioContext. Invalid values safely fall back to 100ms. */
  initialOutputLatencySeconds?: number;
  /** Device-neutral head-pose filtering policy passed to the renderer. */
  headPose?: HeadPoseOptions;
  /** 逐对象精确方向双耳渲染（实验性）：对象 VBAP 到密集球面而非床层环。 */
  denseBinauralObjects?: boolean;
  /** 密集球面 IR 集地址（hrtf-dense）；开启 denseBinauralObjects 时必填。 */
  denseBinauralBaseUrl?: string;
  /** Selects the audible PCM owner. `web-audio` remains the default. */
  outputBackend?: OutputBackend;
  /** Native PCM transport. It is a best-effort mirror in Web Audio mode and the
   * authoritative batch ACK/startup clock in native-sidecar mode. */
  nativeRendererSink?: NativeRendererSink;
}

/** 按码流内容推断渲染布局（自动布局模式）。返回 null = 保持当前布局。 */
export type LayoutResolver = (
  bedLabels: readonly string[],
  hasDynamics: boolean,
) => readonly VirtualSpeaker[] | null;

/** 解码前瞻：环形缓冲约 5.3s，前瞻 4s 可吞掉弹窗/后台切换造成的秒级供给抖动。 */
const TARGET_AHEAD_SECONDS = 4;
const STARTUP_AHEAD_SECONDS = 0.5;
/** 输出端 FIFO 深度（AudioContext latencyHint，秒）。Windows 系统级抖动
 * （任务管理器/DWM 窗口合成/驱动打嗝、对象移动时的渲染线程抢占）动辄 20–60ms，
 * "playback" 默认只给 ~20ms 输出缓冲，吸收不了就是可闻卡顿——即便 worklet 环形
 * 缓冲里有 4s PCM 也无济于事，因为缺口发生在输出级。给到 100ms；
 * UI/可视化播放头按 baseLatency 回拨补偿，音画同步不受影响。 */
const INITIAL_OUTPUT_LATENCY_SECONDS = 0.1;
const OUTPUT_LATENCY_STEPS_SECONDS = [0.1, 0.2, 0.3] as const;

/** Dolby's music delivery loudness target (Dolby Atmos Music: −18 LKFS
 *  integrated per BS.1770-4). Content without codec loudness metadata —
 *  ALAC, PCM, AAC-LC stereo — is balanced toward it, attenuation-only just
 *  like dialnorm. */const MEASURED_LOUDNESS_TARGET_LUFS = -18;
/** Balance applies once ≥6 s (150 × 400 ms blocks) of gated audio has been
 *  observed; replays use the persisted measurement instead and balance from
 *  sample 0. */
const MEASURED_LOUDNESS_MIN_BLOCKS = 150;
/** The settle is a gentle staircase: ≤0.75 dB per 250 ms scheduled step. */
const MEASURED_LOUDNESS_STEP_DB = 0.75;
const MEASURED_LOUDNESS_STEP_SECONDS = 0.25;

function validatedOutputLatencySeconds(value: number | undefined): OutputLatencySeconds {
  return OUTPUT_LATENCY_STEPS_SECONDS.includes(value as OutputLatencySeconds)
    ? value as OutputLatencySeconds
    : INITIAL_OUTPUT_LATENCY_SECONDS;
}

/** Broad gap telemetry begins at 12ms; only repeated >25ms gaps justify
 * rebuilding the active output FIFO. The rolling window accepts non-contiguous
 * scheduling stalls while ignoring isolated/low-amplitude jitter. */
const CALLBACK_GAP_BURST_WINDOW_MS = 2_500;
const CALLBACK_GAP_BURST_EVENT_THRESHOLD = 4;
const CALLBACK_GAP_BURST_TICK_THRESHOLD = 2;
const CALLBACK_GAP_BURST_MAX_SUM_MS = 100;
const CALLBACK_GAP_DISTRIBUTED_WINDOW_MS = 5_000;
const CALLBACK_GAP_DISTRIBUTED_EVENT_THRESHOLD = 4;
const CALLBACK_GAP_DISTRIBUTED_TICK_THRESHOLD = 3;
const ACTIVE_RECREATE_MIN_AHEAD_SECONDS = 0.5;
const MAX_IN_FLIGHT_BATCHES = 32;
/** Native sidecar transport needs enough lead to survive IPC/control scheduling;
 * its source rings retain at most ten seconds per source. */
const MAX_IN_FLIGHT_SECONDS = 1;
const CHUNK_SIZE = 1 << 20; // 1 MiB reads

function layoutIdFor(layout: readonly VirtualSpeaker[]): LayoutId {
  for (const [id, candidate] of Object.entries(LAYOUTS) as [LayoutId, readonly VirtualSpeaker[]][]) {
    if (
      candidate.length === layout.length &&
      candidate.every((speaker, index) => {
        const current = layout[index];
        return current !== undefined
          && current.name === speaker.name
          && current.azimuth === speaker.azimuth
          && current.elevation === speaker.elevation
          && current.isLfe === speaker.isLfe;
      })
    ) return id;
  }
  throw new Error("native renderer only supports SDA preset speaker layouts");
}

export class SdaPlayer {
  /** 当前活跃实例。防止 HMR / 异常路径泄漏的旧 AudioContext 继续发声：
   *  新实例 init 时强制 dispose 上一个。 */
  private static active: SdaPlayer | null = null;
  private static nextId = 1;
  /** 实例序号，用于诊断"界面控制的实例"和"实际发声的实例"是否一致。 */
  readonly id = SdaPlayer.nextId++;

  private worker: Worker;
  private renderer: SpatialRenderer | null = null;
  private readonly headPoseOptions: HeadPoseOptions | undefined;
  private readonly outputBackend: OutputBackend;
  /** Non-audible stage-one mirror in Web Audio mode; authoritative PCM owner in native-sidecar mode. */
  private readonly nativeRendererSink: NativeRendererSink | undefined;
  private nativeConsumedSamples = 0;
  private nativeConsumedUnsubscribe: (() => void) | undefined;
  private nativeObjectActivityUnsubscribe: (() => void) | undefined;
  /** 逐对象精确方向双耳渲染开关与密集 IR 集地址；renderer 重建时恢复。 */
  private denseBinauralObjects: boolean;
  private denseBinauralBaseUrl: string | undefined;
  private latestHeadPose: HeadPose | null = null;
  private cb: PlayerCallbacks;
  private readyResolve!: () => void;
  private ready: Promise<void>;
  private objectChannels = new Map<number, number>(); // object id → PCM channel
  private decodedFormatKey = "";
  private trackReported = false;
  private knownBedLabels: string[] = [];
  private mutedBedLabels = new Set<string>();
  private soloBedLabels = new Set<string>();
  private mutedSpeakers = new Set<string>();
  private focusedSpeakers = new Set<string>();
  private acceptedEndSample = 0;
  private startupOrigin: number | null = null;
  private startupAcceptedEnd = 0;
  private playbackStarted = false;
  private nativeStartPending = false;
  private nextBatchSequence = 1;
  /** Increments before every renderer replacement so old worklets cannot mutate
   * the active queue through delayed acks or consumed ticks. */
  private rendererGeneration = 0;
  private inFlight = new Map<number, { sequence: number; frame: DecodedFrameData; samples: number }>();
  private submittedFrames = new Set<DecodedFrameData>();
  private batchResults = new Map<DecodedFrameData, { accepted: boolean; samples: number; reason?: string }>();
  /** Frames acknowledged by the active worklet but not yet consumed. They are
   * retained so replacing its AudioContext never discards prebuffered PCM. */
  private acceptedFrames: DecodedFrameData[] = [];
  /** 已解码但尚未喂入 worklet 的帧队列（背压：环形缓冲只有 ~5.5s，
   *  直接灌会被静默丢弃，必须按播放头消耗速度泵入）。 */
  private pcmQueue: DecodedFrameData[] = [];
  private queuedSamples = 0;
  /** 容器头部元数据给出的真实总时长（裸流没有，回退到已解码时长）。 */
  private containerDurationSec: number | null = null;
  private sampleRate = 48000;
  private objects = new Map<number, VisualObject>();
  /** DBMD is static program metadata and is intentionally never sample-scheduled. */
  private binauralMetadata: BinauralRenderMetadata | null = null;
  /** Visual metadata waits for the same codec sample clock as audio gains. */
  private pendingVisualEvents: ObjectEvent[] = [];
  private pendingVisualCursor = 0;
  private pendingVisualTargets = new Map<number, ObjectEvent>();
  private visualObjectsSnapshot: VisualObject[] = [];
  /** Latest worklet-confirmed object sources with post-gain/post-mute signal. */
  private soundingObjectIds = new Set<number>();
  private soundingObjectIdsSnapshot: ReadonlySet<number> = new Set();
  private soundingObjectIdsDirty = false;
  private visualSnapshotDirty = true;
  private visualTimer: ReturnType<typeof setInterval> | null = null;
  private ended = false;
  /** init 参数快照，重建 AudioContext（采样率对齐）时用。 */
  private initArgs: {
    mode: OutputMode;
    workletUrl: string | URL;
    layout?: readonly VirtualSpeaker[];
    binauralBaseUrl: string;
    layoutResolver?: LayoutResolver;
  } | null = null;
  /** 是否已按码流内容做过布局自动检测（每次播放只检测一次）。 */
  private layoutChecked = false;
  /** 用户选择的最终双耳三段 EQ；renderer 重建后恢复。 */
  private binauralEqBands: BinauralEqBands = { low: 0, mid: 0, high: 0 };
  /** Reversible final-output low-frequency A/B; renderer rebuilds retain its selection. */
  private binauralLowFrequencyDiagnosticMode: BinauralLowFrequencyDiagnostic = "reference";
  /** 上次布局检测时是否已有动态对象（对象迟到的码流允许再检测一次）。 */
  private layoutHadDynamics = false;
  /** renderer 重建串行链：采样率对齐与布局自动检测可能在同一帧同时
   *  触发，并发跑 recreateRenderer 会泄漏 AudioContext —— 必须排队。 */
  private recreateChain: Promise<void> = Promise.resolve();
  private lastVolume = 1;
  private volumeBalanceEnabled = false;
  private programLoudness: ProgramLoudnessMetadata | null = null;
  private programLoudnessGainDb: number | null = null;
  private scheduledProgramLoudnessGainDb: number | null | undefined;
  /** Live BS.1770-4 measurement from the decoder worker (metadata-less content). */
  private measuredLoudness: FrameLoudness | null = null;
  private measuredLoudnessBlocks = 0;
  /** Measurement balance for this track has been scheduled (or found unnecessary). */
  private measuredLoudnessSettled = false;
  /** Persisted measurement for the upcoming track, set by the UI per track. */
  private cachedMeasuredLufs: number | null = null;
  /** 杜比 Binaural Settings（近/中/远），重建 renderer 后需恢复。
   *  UI 固定"近"，mid/far 暂不从界面暴露。 */
  private binauralMode: BinauralMode = "near";
  /** 被静音的对象事件 id（Omniphony 式 mute/solo）；重建 renderer 后恢复。 */
  private mutedObjects = new Set<number>();
  /** 独立 LFE 床声道的静音状态，renderer 重建后恢复。 */
  private lfeMuted = false;
  /** 自动布局在用户手动选择后暂停，切回 Auto 时恢复。 */
  private autoLayoutEnabled = true;
  /** 仅真实测量曲线可选；当前 registry 为空，null = 最终输出 literal bypass。 */
  private headphoneProfileId: string | null = null;
  /** 是否已按码流采样率校准过 AudioContext（每次播放只校准一次）。 */
  private rateChecked = false;
  /** Blocks PCM submission and startAt until the initial stream-rate renderer is
   * fully ready. This prevents a default-rate context from audibly starting
   * before a 44.1 → 48 kHz alignment rebuild completes. */
  private initialRendererReady = false;
  private initialRendererRate: number | null = null;
  private lastUnderrunReport = 0;
  /** Requested output FIFO of the active AudioContext. */
  private requestedOutputLatencySeconds: OutputLatencySeconds = INITIAL_OUTPUT_LATENCY_SECONDS;
  /** Latest callback-gap-derived latency, retained for future player creation. */
  private pendingOutputLatencySeconds: OutputLatencySeconds = INITIAL_OUTPUT_LATENCY_SECONDS;
  /** Recent worklet ticks with one or more callback gaps strictly above 25ms. */
  private callbackGapEvidence: { at: number; events: number; maxMs: number }[] = [];
  private health: PlayerHealthSnapshot = this.createHealthSnapshot();
  private decodeSamples: { at: number; seconds: number }[] = [];
  private nextWorkerPushSequence = 1;
  private pendingWorkerPushes = new Map<number, { resolve: () => void; reject: (error: Error) => void }>();
  private disposed = false;

  constructor(cb: PlayerCallbacks = {}, options: SdaPlayerOptions = {}) {
    this.cb = cb;
    this.headPoseOptions = options.headPose;
    this.outputBackend = options.outputBackend ?? "web-audio";
    this.nativeRendererSink = options.nativeRendererSink;
    if (this.outputBackend === "native-sidecar" && !this.nativeRendererSink) {
      throw new Error("native-sidecar outputBackend requires nativeRendererSink");
    }
    this.denseBinauralObjects = options.denseBinauralObjects === true;
    this.denseBinauralBaseUrl = options.denseBinauralBaseUrl;
    this.pendingOutputLatencySeconds = validatedOutputLatencySeconds(options.initialOutputLatencySeconds);
    this.requestedOutputLatencySeconds = this.pendingOutputLatencySeconds;
    this.health = this.createHealthSnapshot();
    this.worker = new Worker(new URL("./decoder.worker.ts", import.meta.url), { type: "module" });
    this.ready = new Promise<void>((res) => (this.readyResolve = res));
    this.worker.onmessage = (e) => this.onWorkerMessage(e.data);
    this.worker.onerror = (e) => this.handleWorkerFailure(`解码 worker 异常：${e.message || "未知错误"}`);
    this.worker.onmessageerror = () => this.handleWorkerFailure("解码 worker 消息传输失败");
  }

  async init(
    mode: OutputMode,
    workletUrl: string | URL,
    layout?: readonly VirtualSpeaker[],
    binauralBaseUrl = "/hrtf",
    layoutResolver?: LayoutResolver,
    initialAutoLayout = true,
  ): Promise<void> {
    console.log(`[SDA] player#${this.id} init (active=#${SdaPlayer.active?.id ?? "-"})`);
    // The UI publishes a fully initialized player atomically. Do not dispose a
    // different instance here: overlapping play requests may still be preparing
    // one, and the older request must never tear down the latest audible player.
    SdaPlayer.active = this;
    this.initArgs = { mode, workletUrl, layout, binauralBaseUrl, layoutResolver };
    this.autoLayoutEnabled = initialAutoLayout;
    this.requestedOutputLatencySeconds = this.pendingOutputLatencySeconds;
    this.health.requestedOutputLatencySeconds = this.requestedOutputLatencySeconds;
    this.health.nextRecommendedOutputLatencySeconds = this.pendingOutputLatencySeconds;
    this.initialRendererReady = false;
    this.initialRendererRate = null;
    try {
      if (this.outputBackend === "native-sidecar") {
        this.installNativeConsumedClock();
        this.installNativeObjectActivity();
        await this.nativeRendererSink?.setLayout(layoutIdFor(layout ?? LAYOUTS["7.1.4"]));
        // A replacement native session starts with its LFE group unmuted. Replay
        // the player's retained state before decoded source declarations arrive.
        this.setLfeMuted(this.lfeMuted);
        this.syncSpeakerMutes(this.mutedSpeakers, this.focusedSpeakers);
        this.setVolume(this.lastVolume);
        this.setVolumeBalance(this.volumeBalanceEnabled);
        this.setNativeProgramGainDb(this.programLoudnessGainDb);
        this.setHeadphoneCompensation(this.headphoneProfileId);
        this.initialRendererReady = true;
        this.worker.postMessage({ type: "init" });
        await this.ready;
        return;
      }
      const ctx = new AudioContext({ latencyHint: this.requestedOutputLatencySeconds });
      const generation = this.rendererGeneration;
      this.renderer = new SpatialRenderer(ctx, {
        mode,
        layout,
        denseBinauralObjects: this.denseBinauralObjects,
        onConsumedTick: (stats) => this.handleConsumedTick(generation, stats),
        onObjectActivity: (ids) => this.handleObjectActivity(generation, ids),
        onBatchResult: (result) => this.handleBatchResult(generation, result),
        headPose: this.headPoseOptions,
      });
      await this.renderer.init(workletUrl);
      if (this.latestHeadPose) this.renderer.setHeadPose(this.latestHeadPose);
      this.observeWorkletHealth(this.renderer, generation);
      this.renderer.setHeadphoneCompensation(this.headphoneProfileId);
      this.renderer.setBinauralEqBands(this.binauralEqBands);
      this.renderer.setBinauralLowFrequencyDiagnostic(this.binauralLowFrequencyDiagnosticMode);
      await this.attachBinauralIrs(this.renderer);
      this.worker.postMessage({ type: "init" });
      await this.ready;
    } catch (error) {
      await this.dispose().catch(() => {});
      throw error;
    }
  }

  /** 加载双耳 IR 集并注入渲染器。播放和采样率重建都等待同一份资产完成，
   * 避免启动在浏览器 HRTF、随后异步切到卷积图。 */
  private async attachBinauralIrs(r: SpatialRenderer): Promise<void> {
    const baseUrl = this.initArgs?.binauralBaseUrl;
    if (!baseUrl) throw new Error("双耳 IR 资产地址缺失");
    const wantDense = this.denseBinauralObjects && this.denseBinauralBaseUrl;
    const [set, dense] = await Promise.all([
      getBinauralIrSet(baseUrl),
      wantDense ? getBinauralIrSet(this.denseBinauralBaseUrl!) : Promise.resolve(null),
    ]);
    if (this.disposed || this.renderer !== r) return;
    this.assertCompleteBinauralHeadSet(baseUrl, set);
    // Inject the dense set first so the graph build below mounts dense fill IRs
    // directly instead of building the snapped fallback convolvers first.
    if (dense) r.setDenseBinauralIrSet(dense);
    r.setBinauralData(set);
    if (!r.hasBinauralData) throw new Error("双耳 IR 图未就绪");
    r.setBinauralMode(this.binauralMode);
    console.log(`[SDA] player#${this.id} 双耳 IR 已加载（${set.positions.length} 方向 @${set.sampleRate}Hz）`);
  }

  /** 播放中仅替换逻辑扬声器布局；不重建 AudioContext/worklet，不清 PCM 缓冲。 */
  setLayout(layout: readonly VirtualSpeaker[], manual = true): void {
    if (!this.initArgs) return;
    if (manual) this.autoLayoutEnabled = false;
    this.initArgs.layout = layout;
    this.renderer?.setLayout(layout);
    if (this.outputBackend === "native-sidecar") {
      try {
        const result = this.nativeRendererSink?.setLayout(layoutIdFor(layout));
        if (result instanceof Promise) void result.catch((error) => {
          console.warn(`[SDA] player#${this.id} native layout update failed:`, error);
        });
      } catch (error) {
        console.warn(`[SDA] player#${this.id} native layout update failed:`, error);
      }
    }
    this.emitHealth();
  }

  /** 恢复按当前码流信息自动选择布局。 */
  setAutoLayout(): void {
    const resolver = this.initArgs?.layoutResolver;
    if (!resolver) return;
    this.autoLayoutEnabled = true;
    const hasDyn = this.objectChannels.size > 0;
    const next = resolver(this.knownBedLabels, hasDyn);
    if (next) this.setLayout(next, false);
    this.layoutChecked = true;
    this.layoutHadDynamics = hasDyn;
  }

  /** 播放中实时交叉淡化最终输出模式，保留 decoder/worklet/PCM 与所有 source 状态。 */
  setOutputMode(mode: OutputMode): void {
    if (!this.initArgs) return;
    this.initArgs.mode = mode;
    this.renderer?.setOutputMode(mode);
    this.emitHealth();
  }

  get outputMode(): OutputMode | null {
    return this.renderer?.outputMode ?? this.initArgs?.mode ?? null;
  }

  /** Forward a calibrated canonical ADM head-to-world orientation. This is a
   * real-time control, not codec metadata; it never recreates playback state. */
  setHeadPose(pose: HeadPose): boolean {
    this.latestHeadPose = pose;
    try { this.nativeRendererSink?.setHeadPose(pose); } catch (error) {
      console.warn(`[SDA] player#${this.id} native head pose mirror failed:`, error);
    }
    return this.renderer?.setHeadPose(pose) ?? true;
  }

  clearHeadPose(): void {
    this.latestHeadPose = null;
    try { this.nativeRendererSink?.clearHeadPose(); } catch (error) {
      console.warn(`[SDA] player#${this.id} native clear head pose mirror failed:`, error);
    }
    this.renderer?.clearHeadPose();
  }

  recenterHeadPose(): boolean {
    return this.renderer?.recenterHeadPose() ?? false;
  }

  /** 切换杜比近/中/远（播放中实时生效）。 */
  setBinauralMode(mode: BinauralMode): void {
    this.binauralMode = mode;
    this.renderer?.setBinauralMode(mode);
    this.emitHealth();
  }

  private assertCompleteBinauralHeadSet(baseUrl: string, set: BinauralIrSet): void {
    const setDirectory = new URL(baseUrl, "http://sda.local").pathname.split("/").filter(Boolean).at(-1);
    if (!setDirectory || setDirectory.startsWith("hrtf-ku100-")) {
      throw new Error("拒绝 KU100 与其他 subject 的 hybrid HRTF 资产");
    }
    const requestedSubject = setDirectory.match(/^hrtf-(d2|h(?:[3-9]|1[0-9]|20))$/)?.[1] ?? null;
    if (requestedSubject && (!set.calibrated || !set.completeSubject || set.subjectId !== requestedSubject)) {
      throw new Error(`拒绝不完整或未校准的 ${requestedSubject.toUpperCase()} HRTF 测量集`);
    }
  }

  /** 切换完整人头/subject HRTF（播放中实时生效，不重建解码器/worklet/缓冲）。 */
  async setBinauralHead(baseUrl: string): Promise<void> {
    if (this.initArgs) this.initArgs.binauralBaseUrl = baseUrl;
    const r = this.renderer;
    if (!r) return;
    const set = await getBinauralIrSet(baseUrl);
    this.assertCompleteBinauralHeadSet(baseUrl, set);
    if (this.disposed || this.renderer !== r) return;
    r.setBinauralData(set);
    r.setBinauralMode(this.binauralMode);
    this.emitHealth();
  }

  /** 逐对象精确方向双耳渲染开关（播放中实时生效）。
   *  首次开启时按需加载密集球面 IR 集；renderer 重建后由 attachBinauralIrs 恢复。 */
  async setDenseBinauralObjects(enabled: boolean, denseBaseUrl?: string): Promise<void> {
    if (denseBaseUrl) this.denseBinauralBaseUrl = denseBaseUrl;
    this.denseBinauralObjects = enabled;
    const r = this.renderer;
    if (!r) return;
    if (enabled && !r.hasDenseBinauralData) {
      if (!this.denseBinauralBaseUrl) throw new Error("密集双耳 IR 资产地址缺失");
      const dense = await getBinauralIrSet(this.denseBinauralBaseUrl);
      if (this.disposed || this.renderer !== r) return;
      r.setDenseBinauralIrSet(dense);
    }
    r.setDenseBinauralObjects(enabled);
    this.emitHealth();
  }

  /** 注册主进程已校验的本地左右 FIR。选中该 profile 时只切最终双耳 EQ，
   * 不重建 decoder/worklet/PCM。 */
  registerLocalHeadphoneCompensation(data: LocalHeadphoneCompensationData): void {
    registerLocalHeadphoneCompensation(data);
  }

  /** 移除本地档案。若它正在使用，先回到 literal bypass。 */
  unregisterLocalHeadphoneCompensation(profileId: string): boolean {
    if (this.headphoneProfileId === profileId) this.setHeadphoneCompensation(null);
    return unregisterLocalHeadphoneCompensation(profileId);
  }

  /** 设置最终双耳耳机补偿。profile 必须来自 renderer 注册的真实测量曲线。 */
  setHeadphoneCompensation(profileId: string | null): void {
    if (profileId !== null && !headphoneProfileById(profileId)) {
      throw new Error(`未知或未注册的耳机补偿 profile: ${profileId}`);
    }
    this.headphoneProfileId = profileId;
    this.renderer?.setHeadphoneCompensation(profileId);
    try {
      const result = this.nativeRendererSink?.setHeadphoneProfile(profileId);
      if (result instanceof Promise) void result.catch((error) => {
        console.warn(`[SDA] player#${this.id} native headphone profile update failed:`, error);
      });
    } catch (error) {
      console.warn(`[SDA] player#${this.id} native headphone profile update failed:`, error);
    }
  }

  get headphoneCompensationProfileId(): string | null {
    return this.headphoneProfileId;
  }

  /** 设置最终双耳的低、中、高三段 EQ，不改变空间化或耳机补偿 FIR。 */
  setBinauralEqBands(bands: BinauralEqBands): void {
    this.binauralEqBands = bands;
    this.renderer?.setBinauralEqBands(bands);
    try {
      const result = this.nativeRendererSink?.setBinauralEq(
        bands,
        this.binauralLowFrequencyDiagnosticMode === "low-cut",
      );
      if (result instanceof Promise) void result.catch((error) => {
        console.warn(`[SDA] player#${this.id} native binaural EQ update failed:`, error);
      });
    } catch (error) {
      console.warn(`[SDA] player#${this.id} native binaural EQ update failed:`, error);
    }
  }

  get binauralEq(): Readonly<BinauralEqBands> {
    return this.binauralEqBands;
  }

  /** 切换最终双耳低频诊断，不改变 HRTF、LFE 或物理多声道输出。 */
  setBinauralLowFrequencyDiagnostic(mode: BinauralLowFrequencyDiagnostic): void {
    this.binauralLowFrequencyDiagnosticMode = mode;
    this.renderer?.setBinauralLowFrequencyDiagnostic(mode);
    try {
      const result = this.nativeRendererSink?.setBinauralEq(
        this.binauralEqBands,
        mode === "low-cut",
      );
      if (result instanceof Promise) void result.catch((error) => {
        console.warn(`[SDA] player#${this.id} native low-frequency diagnostic update failed:`, error);
      });
    } catch (error) {
      console.warn(`[SDA] player#${this.id} native low-frequency diagnostic update failed:`, error);
    }
  }

  get binauralLowFrequencyDiagnostic(): BinauralLowFrequencyDiagnostic {
    return this.binauralLowFrequencyDiagnosticMode;
  }

  /** 静音/取消静音一个对象（Omniphony 式 per-object mute 原语；
   * solo 由 UI 层用“mute 其他全部”组合实现）。对象尚未声明时只记录状态，
   * 声源声明到达/renderer 重建时自动应用。 */
  setObjectMuted(objectId: number, muted: boolean): void {
    if (muted) this.mutedObjects.add(objectId);
    else this.mutedObjects.delete(objectId);
    if (!this.objectChannels.has(objectId)) return;
    const sourceId = `obj:${objectId}`;
    if (this.renderer && !this.renderer.setSourceMuted(sourceId, muted)) {
      this.cb.onError?.(`静音未命中：${sourceId} 已声明但渲染器无此声源`);
    }
    try {
      const result = this.nativeRendererSink?.setMuted(sourceId, muted);
      if (result instanceof Promise) {
        void result.catch((error) => console.warn(`[SDA] player#${this.id} native object mute failed:`, error));
      }
    } catch (error) {
      console.warn(`[SDA] player#${this.id} native object mute failed:`, error);
    }
  }

  /** 原子同步整组对象静音状态，避免 React state、首帧声明和 renderer
   * 初始化之间的时序竞争。未声明对象只保存在 mutedObjects，等声明到达时应用。 */
  syncObjectMutes(mutedIds: ReadonlySet<number>): void {
    this.mutedObjects = new Set(mutedIds);
    for (const id of this.objectChannels.keys()) {
      const sourceId = `obj:${id}`;
      this.renderer?.setSourceMuted(sourceId, mutedIds.has(id));
      try {
        const result = this.nativeRendererSink?.setMuted(sourceId, mutedIds.has(id));
        if (result instanceof Promise) void result.catch((error) => {
          console.warn(`[SDA] player#${this.id} native object mute sync failed:`, error);
        });
      } catch (error) {
        console.warn(`[SDA] player#${this.id} native object mute sync failed:`, error);
      }
    }
  }

  syncSpeakerMutes(names: ReadonlySet<string>, focus: ReadonlySet<string> = new Set()): void {
    this.mutedSpeakers = focus.size > 0 ? new Set() : new Set(names);
    this.focusedSpeakers = new Set(focus);
    if (!this.nativeRendererSink?.setSpeakerMutes) {
      if (names.size > 0 || focus.size > 0) this.cb.onError?.("当前输出后端不支持音箱监听控制");
      return;
    }
    try {
      const result = this.nativeRendererSink?.setSpeakerMutes?.([...this.mutedSpeakers], [...this.focusedSpeakers]);
      if (result instanceof Promise) void result.catch(error => this.cb.onError?.(`音箱静音同步失败：${error}`));
    } catch (error) { this.cb.onError?.(`音箱静音同步失败：${error}`); }
  }

  syncBedMix(muted: ReadonlySet<string>, solo: ReadonlySet<string>): void {
    this.mutedBedLabels = new Set(muted);
    this.soloBedLabels = new Set(solo);
    this.knownBedLabels.forEach((label, channel) => {
      if (!label.startsWith("Obj_")) this.applyBedMute(`bed:${channel}`, label);
    });
  }

  private applyBedMute(id: string, label: string, atSample?: number): void {
    const muted = this.mutedBedLabels.has(label)
      || (this.soloBedLabels.size > 0 && !this.soloBedLabels.has(label));
    this.renderer?.setSourceMuted(id, muted);
    try {
      const result = this.nativeRendererSink?.setMuted(id, muted, atSample);
      if (result instanceof Promise) void result.catch((error) => {
        console.warn(`[SDA] native bed mute failed:`, error);
      });
    } catch (error) {
      console.warn(`[SDA] native bed mute failed:`, error);
    }
  }

  /** 静音/恢复独立 LFE 床声道；状态会跨 renderer 重建保留。 */
  setLfeMuted(muted: boolean): void {
    this.lfeMuted = muted;
    this.renderer?.setLfeMuted(muted);
    try {
      const result = this.nativeRendererSink?.setLfeMuted(muted);
      if (result instanceof Promise) {
        void result.catch((error) => console.warn(`[SDA] player#${this.id} native LFE mute failed:`, error));
      }
    } catch (error) {
      console.warn(`[SDA] player#${this.id} native LFE mute failed:`, error);
    }
  }

  /** 码流采样率与 AudioContext 不一致时（如 48k 码流 vs 44.1k 声卡）
   *  重建 AudioContext —— 否则按错误速率播放 = 变慢/降调。
   *  只在音轨发现/首帧时调用一次，此时环形缓冲还没喂数据，切换无损。 */
  private ensureStreamRate(rate: number): void {
    if (this.rateChecked || !this.initArgs) return;
    if (this.outputBackend === "native-sidecar") {
      this.rateChecked = true;
      this.initialRendererReady = true;
      this.pumpPcm();
      return;
    }
    if (!this.renderer) return;
    this.rateChecked = true;
    if (!Number.isFinite(rate) || rate <= 0) {
      this.initialRendererReady = true;
      this.pumpPcm();
      return;
    }
    this.initialRendererRate = rate;
    if (Math.abs(this.renderer.ctx.sampleRate - rate) < 1) {
      this.initialRendererReady = true;
      this.pumpPcm();
      return;
    }
    console.log(`[SDA] player#${this.id} 采样率不匹配：ctx=${this.renderer.ctx.sampleRate} 码流=${rate}，首次发声前重建 AudioContext`);
    this.scheduleRecreate(rate);
  }

  /** 排队重建 renderer（采样率对齐 / 布局自动检测可能在同一帧同时触发，
   *  并发跑 recreateRenderer 会泄漏 AudioContext —— 必须串行）。
   *  recreatePending 期间 pumpPcm 停止喂入：喂给旧 worklet 的帧随旧
   *  AudioContext 关闭整段丢失，攒在队列里才能无损切换。 */
  private recreatePending = 0;

  private scheduleRecreate(sampleRate: number, layout?: readonly VirtualSpeaker[]): void {
    if (layout && this.initArgs) this.initArgs.layout = layout;
    this.recreatePending++;
    this.recreateChain = this.recreateChain
      .then(() => this.recreateRenderer(sampleRate))
      .catch((error) => {
        const message = `renderer 重建失败: ${error instanceof Error ? error.message : String(error)}`;
        console.warn(`[SDA] player#${this.id} ${message}`, error);
        this.handleWorkerFailure(message);
      });
  }

  private async recreateRenderer(sampleRate: number): Promise<void> {
    try {
      const { mode, workletUrl, layout } = this.initArgs!;
      const outputLatencySeconds = this.requestedOutputLatencySeconds;
      const old = this.renderer;
      // Retain every acknowledged but unconsumed frame before the old context is
      // closed. Delayed callbacks from it are ignored by the new generation.
      this.rendererGeneration++;
      this.replayUnconsumedFrames(old?.consumedSamples ?? 0);
      this.inFlight.clear();
      this.submittedFrames.clear();
      this.batchResults.clear();
      this.renderer = null; // pump/feed 暂停，帧在队列里堆积
      await old?.close();
      const ctx = new AudioContext({ latencyHint: outputLatencySeconds, sampleRate });
      // A browser/device may silently choose its hardware rate despite the request.
      // Do not publish or feed this renderer: playing codec-clock PCM through it
      // would make the first audible samples run at the wrong speed and pitch.
      if (Math.abs(ctx.sampleRate - sampleRate) >= 1) {
        await ctx.close();
        throw new Error(`AudioContext 未能匹配码流采样率（请求 ${sampleRate}Hz，实际 ${ctx.sampleRate}Hz）`);
      }
      const generation = this.rendererGeneration;
      const r = new SpatialRenderer(ctx, {
        mode,
        layout,
        denseBinauralObjects: this.denseBinauralObjects,
        onConsumedTick: (stats) => this.handleConsumedTick(generation, stats),
        onObjectActivity: (ids) => this.handleObjectActivity(generation, ids),
        onBatchResult: (result) => this.handleBatchResult(generation, result),
        headPose: this.headPoseOptions,
      });
      await r.init(workletUrl);
      if (this.latestHeadPose) r.setHeadPose(this.latestHeadPose);
      this.observeWorkletHealth(r, generation);
      if (this.disposed) {
        await r.close();
        return;
      }
      r.setVolume(this.lastVolume);
      r.setProgramLoudnessGainDb(this.programLoudnessGainDb);
      this.scheduledProgramLoudnessGainDb = undefined;
      r.setVolumeBalance(this.volumeBalanceEnabled);
      r.setHeadphoneCompensation(this.headphoneProfileId);
      r.setBinauralEqBands(this.binauralEqBands);
      r.setBinauralLowFrequencyDiagnostic(this.binauralLowFrequencyDiagnosticMode);
      r.setLfeMuted(this.lfeMuted);
      this.renderer = r;
      this.requestedOutputLatencySeconds = outputLatencySeconds;
      this.health.requestedOutputLatencySeconds = outputLatencySeconds;
      try {
        await this.attachBinauralIrs(r);
      } catch (error) {
        this.renderer = null;
        await r.close();
        throw error;
      }
      // 恢复暂停意图：重建的 worklet 默认不暂停、新 AudioContext 默认 running，
      // 不恢复的话暂停中重建会让音频自己继续响（UI 仍显示暂停，按钮看似失效）
      if (this.pausedState) {
        r.setPaused(true);
        void r.ctx.suspend().catch(() => {});
      } else {
        await r.ctx.resume();
      }
      // 双耳 IR 已在发布 renderer 前同步注入；此处不再异步切换输出图。
      // 床层/对象源在新 worklet 里重新声明
      this.knownBedLabels = [];
      for (const id of this.objectChannels.keys()) {
        r.addSource(`obj:${id}`);
      }
      // 新 renderer 中恢复对象静音状态。
      for (const id of this.mutedObjects) r.setSourceMuted(`obj:${id}`, true);
    } finally {
      this.recreatePending--;
      if (this.recreatePending === 0 && this.renderer) {
        // 新 worklet 的 consumed 从 0 起计，fedSamples 同步归零 —— 否则
        // fedBufferedSeconds 虚高 TARGET 秒，pump 停摆数秒（表现为开播卡死）。
        // 此刻才喂入的帧全部来自队列，播放内容无损。
        this.acceptedEndSample = 0;
        this.inFlight.clear();
        this.submittedFrames.clear();
        this.batchResults.clear();
        this.resetStartupGate();
        this.resetOutputLatencyProtection();
        this.initialRendererReady = this.initialRendererRate === null || Math.abs(this.renderer.ctx.sampleRate - this.initialRendererRate) < 1;
        this.pumpPcm();
      }
    }
  }

  get audioContext(): AudioContext | null {
    return this.renderer?.ctx ?? null;
  }

  /** Play a File/Blob (browser) end-to-end. */
  async playFile(file: Blob, codec: "auto" | "truehd" | "eac3" | "dts" = "auto"): Promise<void> {
    if (this.outputBackend === "web-audio" && !this.renderer) throw new Error("call init() first");
    await this.renderer?.ctx.resume();
    if (this.disposed) return;
    console.log(`[SDA] player#${this.id} playFile`);
    this.resetOutputLatencyProtection(true);
    this.resetHealth();
    this.worker.postMessage({ type: "open", codec });

    // Audio/object events stay sample-accurate; diagnostics redraw at 10 Hz so
    // the React/Three scene cannot contend with object-heavy Atmos playback.
    this.visualTimer = setInterval(() => this.emitVisual(), 100);

    const stream = file.stream();
    const reader = stream.getReader();
    for (;;) {
      const { done, value } = await reader.read();
      if (done || this.disposed) break;
      await this.pushWorkerChunk(value.buffer);
      await this.pace();
    }
    if (!this.disposed) this.worker.postMessage({ type: "flush" });
  }

  /** Push raw bytes manually (Electron fs stream / network fetch). */
  open(codec: "auto" | "truehd" | "eac3" | "dts" = "auto"): void {
    this.resetOutputLatencyProtection(true);
    this.resetHealth();
    this.worker.postMessage({ type: "open", codec });
    this.visualTimer ??= setInterval(() => this.emitVisual(), 100);
  }

  private pushWorkerChunk(chunk: ArrayBuffer): Promise<void> {
    if (this.disposed) return Promise.resolve();
    const sequence = this.nextWorkerPushSequence++;
    return new Promise<void>((resolve, reject) => {
      this.pendingWorkerPushes.set(sequence, { resolve, reject });
      this.worker.postMessage({ type: "push", chunk, sequence }, [chunk]);
    });
  }

  async push(chunk: Uint8Array): Promise<void> {
    const copy = Uint8Array.from(chunk).buffer;
    await this.pushWorkerChunk(copy);
    await this.pace();
  }

  /** Signal end of a manually pushed stream and drain remaining demuxed PCM. */
  end(): void {
    this.worker.postMessage({ type: "flush" });
  }

  stop(): void {
    this.resetOutputLatencyProtection();
    if (this.visualTimer) clearInterval(this.visualTimer);
    this.visualTimer = null;
    this.renderer?.resetBuffers();
    // The sidecar is a process-wide shared output. A player instance never
    // clears it during stop/dispose: delayed cleanup from an old player can
    // otherwise erase a still-playing or newly-prebuffered session. The next
    // createPlayer() owns the sole reset boundary before declaring its sources.
    this.knownBedLabels.forEach((label, channel) => {
      if (!label.startsWith("Obj_")) this.renderer?.removeSource(`bed:${channel}`);
    });
    for (const id of this.objectChannels.keys()) {
      this.renderer?.removeSource(`obj:${id}`);
    }
    this.knownBedLabels = [];
    this.soundingObjectIds.clear();
    this.soundingObjectIdsDirty = true;
    // 若暂停中停止，同时解除 worklet 静音和时钟挂起，避免卡死
    this.pausedState = false;
    this.renderer?.setPaused(false);
    // Do not resume the shared native sidecar from an outgoing instance; the
    // owning replacement session establishes its own start/pause state.
    if (this.outputBackend !== "native-sidecar") {
      try { void this.nativeRendererSink?.pause(false); } catch {}
    }
    void this.renderer?.ctx.resume();
    this.objects.clear();
    this.visualSnapshotDirty = true;
    this.binauralMetadata = null;
    this.pendingVisualEvents = [];
    this.pendingVisualCursor = 0;
    this.pendingVisualTargets.clear();
    this.objectChannels.clear();
    this.decodedFormatKey = "";
    this.emitVisual();
    this.acceptedEndSample = 0;
    this.nativeConsumedSamples = 0;
    this.inFlight.clear();
    this.submittedFrames.clear();
    this.batchResults.clear();
    this.acceptedFrames = [];
    this.resetStartupGate();
    this.pcmQueue = [];
    this.queuedSamples = 0;
    this.containerDurationSec = null;
    this.trackReported = false;
    this.programLoudness = null;
    this.programLoudnessGainDb = null;
    this.scheduledProgramLoudnessGainDb = undefined;
    this.measuredLoudness = null;
    this.measuredLoudnessBlocks = 0;
    this.measuredLoudnessSettled = false;
    this.cachedMeasuredLufs = null;
    this.renderer?.setProgramLoudnessGainDb(null);
    this.ended = false;
    this.rateChecked = false;
    this.initialRendererReady = false;
    this.initialRendererRate = null;
    this.layoutChecked = false;
    this.layoutHadDynamics = false;
    this.autoLayoutEnabled = true;
    this.resetHealth();
  }

  /** Pause: silence the worklet (buffer-preserving) AND suspend the clock.
   *  The worklet mute alone is sufficient — its consumed counter freezes,
   *  so the playhead stops with it. suspend() is a best-effort backup.
   *  暂停意图记录在 pausedState：renderer 重建（采样率对齐/布局自动检测）
   *  或重建进行中（renderer 暂为 null）时暂停不丢失，recreateRenderer 恢复。 */
  private pausedState = false;

  async pause(): Promise<void> {
    this.pausedState = true;
    this.soundingObjectIds.clear();
    this.soundingObjectIdsDirty = true;
    if (this.outputBackend === "native-sidecar") {
      try { await this.nativeRendererSink?.pause(true); } catch (error) {
        console.warn(`[SDA] player#${this.id} native pause failed:`, error);
      }
      return;
    }
    if (!this.renderer) return;
    console.log(`[SDA] player#${this.id} pause @${this.renderer.consumedSeconds().toFixed(2)}s`);
    this.renderer.setPaused(true);
    try { await this.nativeRendererSink?.pause(true); } catch (error) {
      console.warn(`[SDA] player#${this.id} native pause failed:`, error);
    }
    try {
      await this.renderer.ctx.suspend();
    } catch {
      /* suspend 在某些环境不可靠；worklet 硬暂停已足够 */
    }
  }

  async resume(): Promise<void> {
    this.pausedState = false;
    if (this.outputBackend === "native-sidecar") {
      if (!this.playbackStarted) {
        this.startPlaybackIfReady();
        return;
      }
      try { await this.nativeRendererSink?.pause(false); } catch (error) {
        console.warn(`[SDA] player#${this.id} native resume failed:`, error);
      }
      return;
    }
    if (!this.renderer) return;
    console.log(`[SDA] player#${this.id} resume`);
    this.renderer.setPaused(false);
    try { await this.nativeRendererSink?.pause(false); } catch (error) {
      console.warn(`[SDA] player#${this.id} native resume failed:`, error);
    }
    try {
      await this.renderer.ctx.resume();
    } catch {
      /* ignore */
    }
  }

  setVolumeBalance(enabled: boolean): void {
    this.volumeBalanceEnabled = enabled;
    this.renderer?.setVolumeBalance(enabled);
    try {
      const result = this.nativeRendererSink?.setProgramEnabled(enabled);
      if (result instanceof Promise) void result.catch((error) => {
        console.warn(`[SDA] player#${this.id} native program-balance toggle failed:`, error);
      });
    } catch (error) {
      console.warn(`[SDA] player#${this.id} native program-balance toggle failed:`, error);
    }
  }

  /** 码流携带的节目响度元数据（Dolby dialnorm），无元数据时为 null。 */
  programLoudnessInfo(): ProgramLoudnessMetadata | null {
    return this.programLoudness;
  }

  /** Persisted BS.1770-4 measurement for the upcoming track (UI cache hit).
   *  The balance then applies from sample 0 instead of after convergence. */
  setMeasuredLoudness(integratedLufs: number | null): void {
    this.cachedMeasuredLufs = typeof integratedLufs === "number" && Number.isFinite(integratedLufs)
      ? integratedLufs
      : null;
    this.measuredLoudnessSettled = false;
  }

  /** Schedule the measured balance as a gentle staircase so the settle is
   *  inaudible; attenuation-only, matching the dialnorm contract. */
  private setNativeProgramGainDb(gainDb: number | null, atSample?: number): void {
    try {
      const result = this.nativeRendererSink?.setProgramGainDb(gainDb, atSample);
      if (result instanceof Promise) void result.catch((error) => {
        console.warn(`[SDA] player#${this.id} native program gain failed:`, error);
      });
    } catch (error) {
      console.warn(`[SDA] player#${this.id} native program gain failed:`, error);
    }
  }

  private applyMeasuredLoudnessBalance(integratedLufs: number, atSample: number): void {
    this.cb.onMeasuredLoudness?.(integratedLufs);
    const gainDb = Math.min(0, MEASURED_LOUDNESS_TARGET_LUFS - integratedLufs);
    if (gainDb > -0.05) return;
    const steps = Math.ceil(-gainDb / MEASURED_LOUDNESS_STEP_DB);
    const stepSamples = Math.round(MEASURED_LOUDNESS_STEP_SECONDS * this.sampleRate);
    for (let i = 1; i <= steps; i++) {
      const target = (gainDb * i) / steps;
      this.renderer?.setProgramLoudnessGainDb(target, atSample + i * stepSamples);
      this.setNativeProgramGainDb(target, atSample + i * stepSamples);
    }
  }

  setVolume(v: number): void {
    this.lastVolume = v;
    this.renderer?.setVolume(v);
    try {
      const result = this.nativeRendererSink?.setVolume(v);
      if (result instanceof Promise) void result.catch((error) => {
        console.warn(`[SDA] player#${this.id} native volume update failed:`, error);
      });
    } catch (error) {
      console.warn(`[SDA] player#${this.id} native volume update failed:`, error);
    }
  }

  async dispose(): Promise<void> {
    console.log(`[SDA] player#${this.id} dispose`);
    this.disposed = true;
    this.rejectPendingWorkerPushes("player disposed");
    this.stop();
    this.worker.terminate();
    this.nativeConsumedUnsubscribe?.();
    this.nativeConsumedUnsubscribe = undefined;
    this.nativeObjectActivityUnsubscribe?.();
    this.nativeObjectActivityUnsubscribe = undefined;
    await this.renderer?.close();
    if (SdaPlayer.active === this) SdaPlayer.active = null;
  }

  /** Playhead in seconds: frames the worklet actually rendered.
   *  Immune to AudioContext clock drift / suspend weirdness; freezes on pause.
   *  Clamped to the fed duration — after the stream ends the worklet keeps
   *  rendering silence blocks and its counter would otherwise run past the
   *  end of the song. */
  positionSeconds(): number {
    if (this.outputBackend === "native-sidecar") {
      const origin = this.startupOrigin ?? 0;
      return Math.min(Math.max(0, this.consumedSamples() - origin) / this.sampleRate, this.durationSeconds());
    }
    if (!this.renderer) return 0;
    // 听觉位置补偿：worklet 已渲染的样本还要经过 peak guard lookahead（5ms）
    // 和输出级 FIFO（baseLatency，100–300ms 自适应请求）才到达 DAC。
    // 进度条与 3D 对象可视化应对齐用户实际听到的位置，而不是渲染时钟。
    const outputLatency = (this.renderer.ctx.baseLatency || 0) + 0.005;
    const audible = Math.max(0, this.renderer.consumedSeconds() - outputLatency);
    return Math.min(audible, this.durationSeconds());
  }

  durationSeconds(): number {
    return this.containerDurationSec ?? Math.max(0, this.acceptedEndSample - (this.startupOrigin ?? 0)) / this.sampleRate;
  }

  // ---- internals ----

  private createHealthSnapshot(): PlayerHealthSnapshot {
    return {
      requestedOutputLatencySeconds: this.requestedOutputLatencySeconds,
      nextRecommendedOutputLatencySeconds: this.pendingOutputLatencySeconds,
      baseLatencySeconds: this.renderer?.ctx.baseLatency ?? 0,
      outputLatencySeconds: typeof this.renderer?.ctx.outputLatency === "number" ? this.renderer.ctx.outputLatency : null,
      audioContextSampleRate: this.renderer?.ctx.sampleRate ?? 0,
      outputLatencyHintLimited: false,
      callbackGaps: 0,
      underrunSamples: 0,
      callbackGapWindowEvents: 0,
      callbackGapWindowTicks: 0,
      callbackGapDistributedEvents: 0,
      callbackGapDistributedTicks: 0,
      callbackGapEscalation: "none",
      tick: {
        callbackGaps: 0,
        callbackGapsOver25Ms: 0,
        callbackGapMaxMs: 0,
        underrunSamples: 0,
        rejectedBatches: 0,
        rejectedSources: 0,
        processMeanMs: 0,
        processMaxMs: 0,
      },
      decodeRealtimeMultiplier: 0,
      fedBufferedSeconds: 0,
      queuedSeconds: 0,
      binaural: {
        activeBankCount: 0,
        banks: [],
        totalSpatialConvolutions: 0,
        totalDirectPaths: 0,
      },
    };
  }

  private refreshAudioContextHealth(): void {
    const ctx = this.renderer?.ctx;
    if (!ctx) return;
    this.health.baseLatencySeconds = Number(ctx.baseLatency) || 0;
    this.health.outputLatencySeconds = typeof ctx.outputLatency === "number" && Number.isFinite(ctx.outputLatency)
      ? ctx.outputLatency
      : null;
    this.health.audioContextSampleRate = ctx.sampleRate;
    // baseLatency is not guaranteed to equal latencyHint, but a value below half
    // the request proves Chromium/device did not provide the expected FIFO depth.
    this.health.outputLatencyHintLimited = this.health.baseLatencySeconds < this.requestedOutputLatencySeconds * 0.5;
  }

  private emitHealth(): void {
    this.refreshAudioContextHealth();
    this.health.fedBufferedSeconds = this.fedBufferedSeconds();
    this.health.queuedSeconds = this.queuedSamples / this.sampleRate;
    this.health.binaural = this.renderer?.binauralHealth ?? {
      activeBankCount: 0,
      banks: [],
      totalSpatialConvolutions: 0,
      totalDirectPaths: 0,
    };
    this.cb.onHealth?.({
      ...this.health,
      tick: { ...this.health.tick },
      binaural: { ...this.health.binaural, banks: [...this.health.binaural.banks] },
    });
  }

  private resetHealth(): void {
    this.health = this.createHealthSnapshot();
    this.decodeSamples = [];
    this.emitHealth();
  }

  /** Clear only the active session's callback-gap evidence. The upgraded
   * latency remains selected for subsequently created players. */
  private resetOutputLatencyProtection(_forNewPlayback = false): void {
    this.callbackGapEvidence = [];
    this.health.callbackGapWindowEvents = 0;
    this.health.callbackGapWindowTicks = 0;
    this.health.callbackGapDistributedEvents = 0;
    this.health.callbackGapDistributedTicks = 0;
  }

  private observeCallbackGaps(
    callbackGapsOver25Ms: number,
    callbackGapMaxMs: number,
    now = performance.now(),
  ): void {
    if (this.recreatePending > 0) {
      this.resetOutputLatencyProtection();
      return;
    }
    this.callbackGapEvidence = this.callbackGapEvidence.filter(
      (entry) => entry.at >= now - CALLBACK_GAP_DISTRIBUTED_WINDOW_MS,
    );
    if (callbackGapsOver25Ms > 0) {
      this.callbackGapEvidence.push({ at: now, events: callbackGapsOver25Ms, maxMs: callbackGapMaxMs });
    }
    const summarize = (windowMs: number) => {
      const entries = this.callbackGapEvidence.filter((entry) => entry.at >= now - windowMs);
      return {
        entries,
        events: entries.reduce((total, entry) => total + entry.events, 0),
        maxSumMs: entries.reduce((total, entry) => total + entry.maxMs, 0),
      };
    };
    const burst = summarize(CALLBACK_GAP_BURST_WINDOW_MS);
    const distributed = summarize(CALLBACK_GAP_DISTRIBUTED_WINDOW_MS);
    this.health.callbackGapWindowEvents = burst.events;
    this.health.callbackGapWindowTicks = burst.entries.length;
    this.health.callbackGapDistributedEvents = distributed.events;
    this.health.callbackGapDistributedTicks = distributed.entries.length;
    const trigger = (
      distributed.events >= CALLBACK_GAP_DISTRIBUTED_EVENT_THRESHOLD &&
      distributed.entries.length >= CALLBACK_GAP_DISTRIBUTED_TICK_THRESHOLD
    )
      ? "distributed"
      : (
        burst.events >= CALLBACK_GAP_BURST_EVENT_THRESHOLD &&
        burst.entries.length >= CALLBACK_GAP_BURST_TICK_THRESHOLD &&
        burst.maxSumMs >= CALLBACK_GAP_BURST_MAX_SUM_MS
      )
        ? "burst"
        : null;
    if (!trigger) return;

    const currentStep = OUTPUT_LATENCY_STEPS_SECONDS.indexOf(
      this.requestedOutputLatencySeconds as (typeof OUTPUT_LATENCY_STEPS_SECONDS)[number],
    );
    const nextLatency = OUTPUT_LATENCY_STEPS_SECONDS[currentStep + 1];
    if (
      nextLatency === undefined ||
      !this.renderer ||
      !this.initArgs ||
      !this.initialRendererReady ||
      !this.playbackStarted ||
      !Number.isFinite(this.sampleRate) ||
      this.sampleRate <= 0
    ) return;

    this.pendingOutputLatencySeconds = nextLatency;
    this.health.nextRecommendedOutputLatencySeconds = nextLatency;
    const activeRecreateSafe = this.aheadSeconds() >= ACTIVE_RECREATE_MIN_AHEAD_SECONDS;
    this.health.callbackGapEscalation = activeRecreateSafe ? trigger : "deferred-low-buffer";
    try {
      this.cb.onOutputLatencyRecommendation?.(nextLatency);
    } catch (error) {
      console.warn(`[SDA] player#${this.id} failed to persist output latency recommendation`, error);
    }
    if (!activeRecreateSafe) {
      console.warn(`[SDA] player#${this.id} ${trigger} callback-gap evidence; deferring ${Math.round(nextLatency * 1000)}ms output latency to next playback because ahead PCM is below ${ACTIVE_RECREATE_MIN_AHEAD_SECONDS}s`);
      this.resetOutputLatencyProtection();
      this.emitHealth();
      return;
    }

    this.requestedOutputLatencySeconds = nextLatency;
    this.health.requestedOutputLatencySeconds = nextLatency;
    console.warn(
      `[SDA] player#${this.id} ${trigger} callback-gap evidence; recreating active output ` +
      `at ${Math.round(nextLatency * 1000)}ms requested latency`,
    );
    this.resetOutputLatencyProtection();
    this.scheduleRecreate(this.sampleRate);
    this.emitHealth();
  }

  private recordDecode(samples: number, sampleRate: number): void {
    if (samples <= 0 || sampleRate <= 0) return;
    const now = performance.now();
    this.decodeSamples.push({ at: now, seconds: samples / sampleRate });
    this.refreshDecodeRealtimeMultiplier(now);
  }

  private refreshDecodeRealtimeMultiplier(now: number): void {
    const windowStart = now - 5000;
    while (this.decodeSamples.length > 0 && this.decodeSamples[0]!.at < windowStart) this.decodeSamples.shift();
    const oldest = this.decodeSamples[0];
    const elapsedSeconds = oldest ? Math.max(0.001, (now - oldest.at) / 1000) : 0;
    this.health.decodeRealtimeMultiplier = elapsedSeconds > 0
      ? this.decodeSamples.reduce((total, entry) => total + entry.seconds, 0) / elapsedSeconds
      : 0;
  }

  private resetStartupGate(): void {
    this.startupOrigin = null;
    this.startupAcceptedEnd = 0;
    this.playbackStarted = false;
    this.nativeStartPending = false;
  }

  private startPlaybackIfReady(force = false): void {
    if (
      !this.initialRendererReady
      || this.playbackStarted
      || this.nativeStartPending
      || this.startupOrigin === null
      || this.pausedState
    ) return;
    const required = Math.min(STARTUP_AHEAD_SECONDS, this.renderer?.maxBufferedSeconds() ?? STARTUP_AHEAD_SECONDS) * this.sampleRate;
    if (!force && this.startupAcceptedEnd - this.startupOrigin < required) return;
    if (this.outputBackend === "native-sidecar") {
      const origin = this.startupOrigin;
      this.nativeStartPending = true;
      const begin = performance.now();
      const attemptStart = (): void => {
        Promise.resolve(this.nativeRendererSink!.startAt(origin)).then((accepted) => {
          if (this.disposed || this.startupOrigin !== origin) return;
          if (accepted === true) {
            this.nativeStartPending = false;
            this.playbackStarted = true;
            this.updateNativeConsumedCursor(this.nativeRendererSink!.getConsumedSamples?.() ?? origin);
            this.pumpPcm();
            return;
          }
          if (performance.now() - begin < 5_000) {
            setTimeout(attemptStart, 50);
            return;
          }
          this.nativeStartPending = false;
          this.cb.onError?.("native sidecar rejected playback start");
        }).catch((error) => {
          this.nativeStartPending = false;
          this.cb.onError?.(`native sidecar start failed: ${error instanceof Error ? error.message : String(error)}`);
        });
      };
      attemptStart();
      return;
    }
    this.renderer?.startAt(this.startupOrigin);
    try {
      const nativeStart = this.nativeRendererSink?.startAt(this.startupOrigin);
      if (nativeStart instanceof Promise) {
        void nativeStart.then((accepted) => {
          if (!accepted) console.warn(`[SDA] player#${this.id} native startAt was rejected; Web Audio remains owner`);
        }).catch((error) => console.warn(`[SDA] player#${this.id} native startAt failed:`, error));
      }
    } catch (error) {
      console.warn(`[SDA] player#${this.id} native startAt failed:`, error);
    }
    this.playbackStarted = true;
  }

  /** Keep only PCM after an old worklet's consumption cursor. Object events at
   * or before that cursor have already taken effect and must not be replayed. */
  private frameAfter(frame: DecodedFrameData, sample: number): DecodedFrameData | null {
    const samples = frame.channels[0]?.length ?? 0;
    const end = frame.samplePos + samples;
    if (end <= sample) return null;
    if (frame.samplePos >= sample) return frame;
    const offset = Math.max(0, sample - frame.samplePos);
    return {
      ...frame,
      samplePos: frame.samplePos + offset,
      channels: frame.channels.map((channel) => channel.subarray(offset)),
      // Retain prior target events too: the fresh renderer needs the last
      // object state established before this partial frame resumes.
      events: frame.events,
    };
  }

  /** Rebuild the submission queue from PCM which the retiring renderer had
   * accepted but not consumed, followed by frames it had not acknowledged. */
  private replayUnconsumedFrames(consumedSamples: number): void {
    const replay = [
      ...this.acceptedFrames.map((frame) => this.frameAfter(frame, consumedSamples)),
      ...this.pcmQueue,
    ].filter((frame): frame is DecodedFrameData => frame !== null);
    replay.sort((a, b) => a.samplePos - b.samplePos);
    this.pcmQueue = replay;
    this.queuedSamples = replay.reduce((total, frame) => total + (frame.channels[0]?.length ?? 0), 0);
    this.acceptedFrames = [];
    this.acceptedEndSample = 0;
    this.resetStartupGate();
  }

  private installNativeConsumedClock(): void {
    this.nativeConsumedUnsubscribe?.();
    this.nativeConsumedUnsubscribe = undefined;
    const sink = this.nativeRendererSink;
    if (!sink) return;
    this.nativeConsumedSamples = sink.getConsumedSamples?.() ?? 0;
    const unsubscribe = sink.onConsumedSamples?.((sample) => this.updateNativeConsumedCursor(sample));
    if (typeof unsubscribe === "function") this.nativeConsumedUnsubscribe = unsubscribe;
  }

  private installNativeObjectActivity(): void {
    this.nativeObjectActivityUnsubscribe?.();
    this.nativeObjectActivityUnsubscribe = undefined;
    const sink = this.nativeRendererSink;
    if (!sink) return;
    const generation = this.rendererGeneration;
    const unsubscribe = sink.onObjectActivity?.((ids) => this.handleObjectActivity(generation, ids));
    if (typeof unsubscribe === "function") this.nativeObjectActivityUnsubscribe = unsubscribe;
  }

  /** Native-sidecar cursor updates drive queue reclamation, pacing, end detection,
   * and the sample-clock visual timeline without creating an AudioContext. */
  private updateNativeConsumedCursor(sample: number): void {
    if (this.outputBackend !== "native-sidecar" || !Number.isFinite(sample)) return;
    this.nativeConsumedSamples = Math.max(this.nativeConsumedSamples, Math.trunc(sample));
    this.consumeAcceptedFrames();
    this.pumpPcm();
    this.emitHealth();
  }

  private consumedSamples(): number {
    if (this.outputBackend === "native-sidecar") {
      const reported = this.nativeRendererSink?.getConsumedSamples?.();
      if (typeof reported === "number" && Number.isFinite(reported)) {
        this.nativeConsumedSamples = Math.max(this.nativeConsumedSamples, Math.trunc(reported));
      }
      return this.nativeConsumedSamples;
    }
    return this.renderer?.consumedSamples ?? 0;
  }

  /** Drop acknowledged PCM only after the active output backend has rendered it. */
  private consumeAcceptedFrames(): void {
    if (!this.playbackStarted) return;
    const consumed = this.consumedSamples();
    while (this.acceptedFrames.length > 0) {
      const frame = this.acceptedFrames[0]!;
      if (frame.samplePos + (frame.channels[0]?.length ?? 0) > consumed) break;
      this.acceptedFrames.shift();
    }
    this.checkEnded();
  }

  private commitBatchResults(): void {
    while (this.pcmQueue.length > 0) {
      const frame = this.pcmQueue[0]!;
      const result = this.batchResults.get(frame);
      if (!result) break;
      this.batchResults.delete(frame);
      this.submittedFrames.delete(frame);
      this.pcmQueue.shift();
      const samples = frame.channels[0]?.length ?? 0;
      this.queuedSamples -= samples;
      if (!result.accepted) {
        const backend = this.outputBackend === "native-sidecar" ? "native sidecar" : "worklet";
        this.cb.onError?.(`PCM frame 被 ${backend} 跳过：${result.reason ?? "unknown"}`);
        continue;
      }
      const accepted = result.samples === samples
        ? frame
        : this.frameAfter(frame, frame.samplePos + Math.max(0, samples - result.samples));
      if (!accepted) continue;
      const end = accepted.samplePos + (accepted.channels[0]?.length ?? 0);
      this.acceptedFrames.push(accepted);
      this.acceptedEndSample = Math.max(this.acceptedEndSample, end);
      if (!this.playbackStarted) {
        if (this.startupOrigin === null) {
          this.startupOrigin = accepted.samplePos;
          this.startupAcceptedEnd = end;
        } else if (accepted.samplePos === this.startupAcceptedEnd) {
          this.startupAcceptedEnd = end;
        } else {
          // Do not begin through a gap or an out-of-order acknowledged range.
          break;
        }
      }
    }
    this.startPlaybackIfReady();
  }

  private handleConsumedTick(generation: number, stats: { underrunSamples: number; rejectedBatches: number; rejectedSources: number; callbackGaps?: number; callbackGapMaxMs?: number }): void {
    if (generation !== this.rendererGeneration) return;
    this.reportRendererHealth(stats);
    this.consumeAcceptedFrames();
    this.pumpPcm();
  }

  private handleObjectActivity(generation: number, ids: readonly number[]): void {
    if (generation !== this.rendererGeneration) return;
    const next = new Set(ids.filter((id) => this.objectChannels.has(id) && !this.mutedObjects.has(id)));
    if (
      next.size === this.soundingObjectIds.size &&
      [...next].every((id) => this.soundingObjectIds.has(id))
    ) return;
    this.soundingObjectIds = next;
    this.soundingObjectIdsDirty = true;
  }

  private handleBatchResult(generation: number, result: { sequence: number; accepted: boolean; samples: number; reason?: string }): void {
    if (generation !== this.rendererGeneration) return;
    const pending = this.inFlight.get(result.sequence);
    if (!pending) return;
    this.inFlight.delete(result.sequence);
    if (!result.accepted && result.reason === "ring-full") {
      this.submittedFrames.delete(pending.frame);
    } else if (
      !result.accepted &&
      this.outputBackend === "native-sidecar" &&
      /unknown source|source ring capacity/i.test(result.reason ?? "") &&
      pending.frame.samplePos + pending.samples <= this.consumedSamples()
    ) {
      // The sidecar already consumed an earlier copy of this frame and the ACK
      // was lost; the retry raced the codec clock. The audio is committed, so
      // treat the replay as accepted instead of dropping the frame audibly.
      this.batchResults.set(pending.frame, { sequence: result.sequence, accepted: true, samples: pending.samples });
    } else {
      this.batchResults.set(pending.frame, result);
    }
    this.commitBatchResults();
    this.pumpPcm();
  }

  private targetAheadSeconds(): number {
    return Math.min(TARGET_AHEAD_SECONDS, this.renderer?.maxBufferedSeconds() ?? TARGET_AHEAD_SECONDS);
  }

  private observeWorkletHealth(renderer: SpatialRenderer, generation: number): void {
    const node = (renderer as unknown as { node: AudioWorkletNode | null }).node;
    node?.port.addEventListener("message", (event: MessageEvent) => {
      const tick = event.data;
      if (generation !== this.rendererGeneration || this.renderer !== renderer || tick?.type !== "tick") return;
      const callbackGaps = Number(tick.callbackGaps) || 0;
      const callbackGapsOver25Ms = Number(tick.callbackGapsOver25Ms) || 0;
      const underrunSamples = Number(tick.underrunSamples) || 0;
      this.health.callbackGaps += callbackGaps;
      this.health.underrunSamples += underrunSamples;
      this.health.tick = {
        callbackGaps,
        callbackGapsOver25Ms,
        callbackGapMaxMs: Number(tick.callbackGapMaxMs) || 0,
        underrunSamples,
        rejectedBatches: Number(tick.rejectedBatches) || 0,
        rejectedSources: Number(tick.rejectedSources) || 0,
        processMeanMs: Number(tick.processMeanMs) || 0,
        processMaxMs: Number(tick.processMaxMs) || 0,
      };
      this.observeCallbackGaps(
        callbackGapsOver25Ms,
        Number(tick.callbackGapMaxMs) || 0,
      );
      this.refreshDecodeRealtimeMultiplier(performance.now());
      this.emitHealth();
    });
    node?.port.start();
  }

  private reportRendererHealth(stats: { underrunSamples: number; rejectedBatches: number; rejectedSources: number; callbackGaps?: number; callbackGapMaxMs?: number }): void {
    const gaps = stats.callbackGaps ?? 0;
    if (stats.underrunSamples === 0 && stats.rejectedBatches === 0 && stats.rejectedSources === 0 && gaps === 0) return;
    const now = performance.now();
    if (now - this.lastUnderrunReport < 1000) return;
    this.lastUnderrunReport = now;
    const details = [
      stats.underrunSamples ? `断供 ${stats.underrunSamples} samples` : "",
      stats.rejectedBatches ? `拒绝 ${stats.rejectedBatches} PCM frame` : "",
      stats.rejectedSources ? `拒绝 ${stats.rejectedSources} source` : "",
      gaps ? `回调间隙 ${gaps} 次（最大 ${(stats.callbackGapMaxMs ?? 0).toFixed(1)}ms）` : "",
      `缓冲 ${Math.max(0, this.fedBufferedSeconds()).toFixed(2)}s`,
    ].filter(Boolean).join("，");
    this.cb.onError?.(`音频实时供给不足：${details}`);
  }

  private async pace(): Promise<void> {
    // renderer 为 null（重建中）也要继续节流：queuedSamples 仍在累计，
    // 否则整个文件会在重建窗口内灌进 worker 解码（帧随即因 renderer 缺席堆积，
    // 缓冲爆炸）。disposed 时退出避免死等。
    while (!this.disposed && this.aheadSeconds() > this.targetAheadSeconds()) {
      await new Promise((r) => setTimeout(r, 50));
    }
  }

  private aheadSeconds(): number {
    // 读取节流看的是"已解码但未播出"总量 = 队列里的 + 环形缓冲里的。
    return this.queuedSamples / this.sampleRate + this.fedBufferedSeconds();
  }

  private rejectPendingWorkerPushes(message: string): void {
    const error = new Error(message);
    for (const pending of this.pendingWorkerPushes.values()) pending.reject(error);
    this.pendingWorkerPushes.clear();
  }

  private handleWorkerFailure(message: string): void {
    this.rejectPendingWorkerPushes(message);
    if (this.disposed) return;
    this.cb.onError?.(message);
    this.ended = true;
    this.pcmQueue = [];
    this.queuedSamples = 0;
    this.checkEnded();
  }

  private onWorkerMessage(msg: { type: string; [k: string]: unknown }): void {
    switch (msg.type) {
      case "ready":
        this.readyResolve();
        break;
      case "track": {
        this.trackReported = true;
        const track = msg.track as { codec: string; sampleRate: number; channels: number; container: string; durationSec?: number; title?: string; coverArt?: { bytes: Uint8Array; mimeType: "image/jpeg" | "image/png" } };
        if (track.durationSec && Number.isFinite(track.durationSec)) {
          this.containerDurationSec = track.durationSec;
        }
        this.ensureStreamRate(track.sampleRate);
        console.log(
          `[SDA] player#${this.id} 轨道 ${track.container}/${track.codec} ${track.sampleRate}Hz ${track.channels}ch` +
          (track.durationSec ? ` ${track.durationSec.toFixed(3)}s` : ""),
        );
        this.cb.onTrack?.(track);
        break;
      }
      case "binaural-metadata": {
        this.binauralMetadata = msg.metadata as BinauralRenderMetadata;
        this.cb.onBinauralMetadata?.(this.binauralMetadata);
        break;
      }
      case "frame":
        this.handleFrame(msg.frame as DecodedFrameData);
        break;
      case "push-ack": {
        const sequence = Number(msg.sequence);
        const pending = this.pendingWorkerPushes.get(sequence);
        if (!pending) break;
        this.pendingWorkerPushes.delete(sequence);
        if (msg.error) pending.reject(new Error(String(msg.error)));
        else pending.resolve();
        break;
      }
      case "flushed":
        this.ended = true;
        this.startPlaybackIfReady(true);
        this.checkEnded();
        break;
      case "error":
        this.cb.onError?.(String(msg.message));
        break;
    }
  }

  private handleFrame(frame: DecodedFrameData): void {
    // 注意：renderer 为 null（重建窗口中）也不能 return —— 帧必须照样排队，
    // 否则窗口内解码的帧被静默丢弃（采样率对齐重建 + pace 同时失灵时，
    // 整个文件会在窗口内解完扔光 → 提前 onEnded，卡在第几秒）。
    // pumpPcm 自己有 null 守卫，队列在重建完成后继续泵。
    this.sampleRate = frame.sampleRate;
    this.recordDecode(frame.channels[0]?.length ?? 0, frame.sampleRate);
    if (frame.programLoudness) {
      this.programLoudness = frame.programLoudness;
      this.programLoudnessGainDb = Math.min(0, frame.programLoudness.gainDb);
    }
    if (frame.loudness) {
      this.measuredLoudness = frame.loudness;
      this.measuredLoudnessBlocks = frame.loudness.blocks;
    }

    // Raw elementary streams never fire the demuxer's onTrack — derive the
    // panel info from the first decoded frame instead.
    if (!this.trackReported) {
      this.trackReported = true;
      this.ensureStreamRate(frame.sampleRate);
      this.cb.onTrack?.({
        codec: frame.codec,
        sampleRate: frame.sampleRate,
        channels: frame.channels.length,
        container: "raw",
      });
    }

    // 解码帧先排队，由 pumpPcm 按播放头消耗速度喂入 worklet —
    // 直接灌会撑爆 ~5.5s 的环形缓冲，超出的音频被静默丢弃。
    this.pcmQueue.push(frame);
    this.queuedSamples += frame.channels[0]?.length ?? 0;
    this.pumpPcm();
    this.checkEnded();
  }

  private submittedEndSample(): number {
    let end = this.acceptedEndSample;
    for (const pending of this.inFlight.values()) end = Math.max(end, pending.frame.samplePos + pending.samples);
    return end;
  }

  private submittedBufferedSeconds(): number {
    const origin = this.startupOrigin ?? this.pcmQueue[0]?.samplePos ?? 0;
    const cursor = this.playbackStarted ? this.consumedSamples() : origin;
    return Math.max(0, this.submittedEndSample() - cursor) / this.sampleRate;
  }

  /** 把队列里的帧泵入 worklet 环形缓冲，保持喂入量领先播放头 ~TARGET 秒。 */
  private pumpPcm(): void {
    if ((this.outputBackend === "web-audio" && !this.renderer) || !this.initialRendererReady || this.recreatePending > 0) return;
    let outstandingSamples = [...this.inFlight.values()].reduce((sum, pending) => sum + pending.samples, 0);
    while (
      this.inFlight.size < MAX_IN_FLIGHT_BATCHES &&
      outstandingSamples < MAX_IN_FLIGHT_SECONDS * this.sampleRate &&
      this.submittedBufferedSeconds() <= this.targetAheadSeconds()
    ) {
      const frame = this.pcmQueue.find((candidate) => !this.submittedFrames.has(candidate));
      if (!frame) break;
      const frameSamples = frame.channels[0]?.length ?? 0;
      const renderer = this.renderer;
      if (frame.programLoudness) {
        const gainDb = Math.min(0, frame.programLoudness.gainDb);
        if (gainDb !== this.scheduledProgramLoudnessGainDb) {
          this.scheduledProgramLoudnessGainDb = gainDb;
          renderer?.setProgramLoudnessGainDb(gainDb, frame.samplePos);
          this.setNativeProgramGainDb(gainDb, frame.samplePos);
        }
      } else if (!this.measuredLoudnessSettled) {
        // Metadata-less content (ALAC/PCM/AAC stereo): balance from a persisted
        // measurement immediately, or from the live BS.1770-4 estimate once it
        // has enough gated audio to be trustworthy.
        const integrated = this.cachedMeasuredLufs
          ?? (this.measuredLoudnessBlocks >= MEASURED_LOUDNESS_MIN_BLOCKS ? this.measuredLoudness?.integratedLufs ?? null : null);
        if (integrated != null) {
          this.measuredLoudnessSettled = true;
          this.applyMeasuredLoudnessBalance(integrated, frame.samplePos);
        }
      }

      // (Re)declare bed sources when labels change. Retire channels removed by
      // a topology contraction on the same codec sample boundary.
      if (frame.labels.join() !== this.knownBedLabels.join()) {
        const previousLabels = this.knownBedLabels;
        previousLabels.forEach((label, ch) => {
          const next = frame.labels[ch];
          if (!label.startsWith("Obj_") && (!next || next.startsWith("Obj_"))) {
            renderer?.retireSourceAt(`bed:${ch}`, frame.samplePos);
            try { this.nativeRendererSink?.removeSource(`bed:${ch}`, frame.samplePos); } catch (error) {
              console.warn(`[SDA] player#${this.id} native renderer source removal mirror failed:`, error);
            }
          }
        });
        this.knownBedLabels = frame.labels;
        frame.labels.forEach((label, ch) => {
          if (!label.startsWith("Obj_")) {
            const id = `bed:${ch}`;
            renderer?.rebindBedSource(id, label, frame.samplePos);
            try {
              this.nativeRendererSink?.addSource({ id, atSample: frame.samplePos, bedLabel: label });
            } catch (error) {
              console.warn(`[SDA] player#${this.id} native renderer bed rebind mirror failed:`, error);
            }
            this.applyBedMute(id, label, frame.samplePos);
          }
        });
      }

      // Sparse object↔channel declaration. An all-fixed frame (no Obj_ labels)
      // is an explicit presentation transition, not an unchanged sparse frame:
      // drop old object routes so a later bed PCM channel cannot inherit a stale
      // moving-object binding after an invalid/missing JOC↔OAMD mapping.
      const hasObjectLabels = frame.labels.some((label) => label.startsWith("Obj_"));
      const declaredObjects = frame.objectChannels as ObjectChannelDecl[];
      // JOC declarations are intentionally sparse. A decoder can omit the
      // unchanged declaration while still carrying the same Obj_* PCM channels;
      // recover that durable mapping from labels so those channels can never be
      // rebound as `bed:*` during a sparse metadata update.
      const labelObjects: ObjectChannelDecl[] = frame.labels.flatMap((label, channel) => {
        const id = /^Obj_(\d+)$/.exec(label)?.[1];
        return id === undefined ? [] : [{ id: Number(id), channel }];
      });
      const declarations = declaredObjects.length > 0 ? declaredObjects : labelObjects;
      const bedLabels = frame.labels.filter((label) => !label.startsWith("Obj_"));
      // Object declarations are sparse after their first frame. Labels remain on
      // every PCM frame, so they are the durable decoded-format signal for UI.
      const objectChannelCount = frame.labels.filter((label) => label.startsWith("Obj_")).length;
      const decodedFormatKey = `${frame.rawBedLabels.join(",")}|${bedLabels.join(",")}|${objectChannelCount}`;
      if (decodedFormatKey !== this.decodedFormatKey) {
        this.decodedFormatKey = decodedFormatKey;
        this.cb.onDecodedFormat?.({ rawBedLabels: frame.rawBedLabels, bedLabels, objectChannels: objectChannelCount });
      }
      let visualChanged = false;
      if (declarations.length > 0) {
        // A non-empty sparse declaration is the complete replacement mapping,
        // not a patch. Retire removed sources on the codec sample boundary.
        const nextIds = new Set(declarations.map((declaration) => declaration.id));
        for (const id of this.objectChannels.keys()) {
          if (!nextIds.has(id)) {
            const sourceId = `obj:${id}`;
            renderer?.retireSourceAt(sourceId, frame.samplePos);
            try { this.nativeRendererSink?.removeSource(sourceId, frame.samplePos); } catch (error) {
              console.warn(`[SDA] player#${this.id} native renderer object retirement mirror failed:`, error);
            }
            this.discardPendingVisualEvents(id);
          }
        }
        this.objectChannels.clear();
        const declaredIds = new Set<number>();
        for (const decl of declarations) {
          declaredIds.add(decl.id);
          this.objectChannels.set(decl.id, decl.channel);
          renderer?.addSource(`obj:${decl.id}`, { atSample: frame.samplePos });
          // 声明可能是整组重放；addSource 对已有 id 幂等，此处只同步独立
          // mute 包络，不触碰该源已经排队/生效的位置、增益等元数据。
          const objectSourceId = `obj:${decl.id}`;
          renderer?.setSourceMuted(objectSourceId, this.mutedObjects.has(decl.id));
          if (!this.objects.has(decl.id)) {
            // OAMD events may arrive in a later frame. Expose the object now so
            // the first opened file does not appear to have no objects.
            this.objects.set(decl.id, placeholderVisualObject(decl.id));
            this.visualSnapshotDirty = true;
            visualChanged = true;
          }
        }
        for (const id of this.objects.keys()) {
          if (!declaredIds.has(id)) {
            this.objects.delete(id);
            this.visualSnapshotDirty = true;
            visualChanged = true;
          }
        }
      } else if (!hasObjectLabels) {
        // No object labels on this frame means the decoded programme really is a
        // pure bed now; do not retire objects merely because the frame carried a
        // sparse declaration with no changes.
        for (const id of this.objectChannels.keys()) {
          const sourceId = `obj:${id}`;
          renderer?.retireSourceAt(sourceId, frame.samplePos);
          try { this.nativeRendererSink?.removeSource(sourceId, frame.samplePos); } catch (error) {
            console.warn(`[SDA] player#${this.id} native renderer source removal mirror failed:`, error);
          }
          this.discardPendingVisualEvents(id);
        }
        this.objectChannels.clear();
        if (this.objects.size > 0) {
          this.objects.clear();
          this.visualSnapshotDirty = true;
          visualChanged = true;
        }
      }
      const channelToObject = new Map<number, number>();
      for (const [id, ch] of this.objectChannels) channelToObject.set(ch, id);

      // 自动布局只替换逻辑增益映射；worklet/PCM 缓冲和播放头保持连续。
      // 对象声明迟到的码流同样不会再触发 AudioContext 重建。
      const resolver = this.initArgs?.layoutResolver;
      const hasDyn = this.objectChannels.size > 0;
      if (this.autoLayoutEnabled && resolver && (!this.layoutChecked || (!this.layoutHadDynamics && hasDyn))) {
        this.layoutChecked = true;
        this.layoutHadDynamics = hasDyn;
        const next = resolver(frame.labels, hasDyn);
        const cur = this.initArgs?.layout;
        const same =
          next && cur && next.length === cur.length && next.every((s, i) => s.name === cur[i]!.name);
        if (next && !same) {
          console.log(
            `[SDA] player#${this.id} 布局自动检测 → ${next.length} 音箱（${hasDyn ? "含动态对象" : "纯床层"}），保持播放切换`,
          );
          this.setLayout(next, false);
        }
      }

      // Schedule metadata before exposing this frame to the worklet. Port
      // messages are FIFO, so the first sample can never render with a future
      // or stale object position merely because the player prebuffers ~2 s.
      const events = frame.events as ObjectEvent[];
      renderer?.applyEvents(events);
      this.queueVisualEvents(events);

      // Enqueue every channel of the decoded frame atomically on the codec's
      // absolute sample clock. Per-source feed messages allowed the worklet to
      // consume a partial frame and permanently desynchronise late objects.
      const sourceDeclarations = frame.channels.map((_, ch) => {
        const objectId = channelToObject.get(ch);
        const id = objectId !== undefined ? `obj:${objectId}` : `bed:${ch}`;
        const bedLabel = objectId === undefined ? frame.labels[ch] ?? `Bed_${ch}` : undefined;
        if (bedLabel !== undefined) {
          renderer?.addSource(id, { bedLabel, atSample: frame.samplePos });
        }
        return { id, atSample: frame.samplePos, bedLabel } satisfies NativeRendererSourceDeclaration;
      });
      const entries = frame.channels.map((samples, ch) => ({ id: sourceDeclarations[ch]!.id, samples }));
      const sequence = this.nextBatchSequence++;
      const pending = { sequence, frame, samples: frameSamples };
      this.inFlight.set(sequence, pending);
      this.submittedFrames.add(frame);
      outstandingSamples += frameSamples;
      if (this.outputBackend === "native-sidecar") {
        const submit = async () => {
          // OAMD must arrive before its object source declaration. Rust preserves
          // unknown-object metadata and applies it during addSource, so each decoded
          // Obj_* PCM route creates its own convolver at its first true direction.
          if (events.length > 0) await this.nativeRendererSink!.events(events);
          await Promise.all(sourceDeclarations.map((source) => this.nativeRendererSink!.addSource(source)));
          const result = await this.nativeRendererSink!.frame(frame.samplePos, entries);
          this.handleBatchResult(this.rendererGeneration, result
            ? { sequence, ...result }
            : { sequence, accepted: false, samples: 0, reason: "native sidecar returned no batch ACK" });
        };
        void submit().catch((error) => {
          this.handleBatchResult(this.rendererGeneration, {
            sequence,
            accepted: false,
            samples: 0,
            reason: error instanceof Error ? error.message : String(error),
          });
        });
      } else {
        try {
          for (const source of sourceDeclarations) this.nativeRendererSink?.addSource(source);
          this.nativeRendererSink?.frame(frame.samplePos, entries);
        } catch (error) {
          console.warn(`[SDA] player#${this.id} native renderer frame mirror failed:`, error);
        }
        if (!this.renderer) return;
        this.renderer.feedBatch(sequence, frame.samplePos, entries);
      }

      if (visualChanged) this.emitVisual();
    }
    this.checkEnded();
  }

  /** 已喂入 worklet 但尚未播出的秒数（真实占着环形缓冲的部分）。 */
  private fedBufferedSeconds(): number {
    if ((this.outputBackend === "web-audio" && !this.renderer) || this.startupOrigin === null) return 0;
    const cursor = this.playbackStarted ? this.consumedSamples() : this.startupOrigin;
    return Math.max(0, this.acceptedEndSample - cursor) / this.sampleRate;
  }

  private checkEnded(): void {
    if (this.ended && this.pcmQueue.length === 0 && this.fedBufferedSeconds() <= 0.2) {
      this.ended = false;
      if (this.visualTimer) clearInterval(this.visualTimer);
      this.visualTimer = null;
      this.soundingObjectIds.clear();
      this.soundingObjectIdsDirty = true;
      this.emitVisual();
      this.cb.onEnded?.();
    }
  }

  private discardPendingVisualEvents(id: number): void {
    this.pendingVisualEvents = withoutPendingObjectEvents(
      this.pendingVisualEvents,
      this.pendingVisualCursor,
      id,
    );
    this.pendingVisualCursor = 0;
    this.pendingVisualTargets.delete(id);
  }

  private queueVisualEvents(events: readonly ObjectEvent[]): void {
    for (const event of events) {
      if (sameObjectTarget(this.pendingVisualTargets.get(event.id), event)) continue;
      this.pendingVisualTargets.set(event.id, event);
      this.pendingVisualEvents.push(event);
    }
  }

  private emitVisual(): void {
    if (this.outputBackend === "native-sidecar") {
      const consumed = this.nativeRendererSink?.getConsumedSamples?.();
      if (typeof consumed === "number" && Number.isFinite(consumed) && consumed > this.nativeConsumedSamples) {
        this.updateNativeConsumedCursor(consumed);
      }
    }
    const streamTimeSec = this.positionSeconds();
    const playedSample = Math.floor(streamTimeSec * this.sampleRate);
    let changed = false;
    while (
      this.pendingVisualCursor < this.pendingVisualEvents.length &&
      this.pendingVisualEvents[this.pendingVisualCursor]!.samplePos <= playedSample
    ) {
      const event = this.pendingVisualEvents[this.pendingVisualCursor++]!;
      this.objects.set(event.id, visualObjectFromEvent(event));
      changed = true;
    }
    if (this.pendingVisualCursor >= 256 && this.pendingVisualCursor * 2 >= this.pendingVisualEvents.length) {
      this.pendingVisualEvents.splice(0, this.pendingVisualCursor);
      this.pendingVisualCursor = 0;
    }
    if (changed || this.visualSnapshotDirty) {
      this.visualObjectsSnapshot = [...this.objects.values()];
      this.visualSnapshotDirty = false;
    }
    // The timeline remains independent from object snapshots. React receives the
    // same array identity while objects are static, so the 3D view can memoise.
    if (this.soundingObjectIdsDirty) {
      this.soundingObjectIdsSnapshot = new Set(this.soundingObjectIds);
      this.soundingObjectIdsDirty = false;
    }
    this.cb.onVisualState?.(this.visualObjectsSnapshot, streamTimeSec, this.soundingObjectIdsSnapshot);
  }
}
