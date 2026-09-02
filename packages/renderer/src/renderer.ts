/**
 * Main-thread spatial renderer.
 *
 * Graph:
 *   AudioWorkletNode(sda-renderer, N-bus output)
 *     ├── "multichannel": → ctx.destination (N discrete channels)
 *     ├── "binaural":     → ChannelSplitter(N)
 *     │                     → 当前逻辑布局的每条主总线: ConvolverNode(stereo BRIR/HRIR mix)
 *     │                     → 当前逻辑布局的 LFE 总线: 120Hz LP（耳机路径不额外 +10dB）→ 直送双耳
 *     │                     → ChannelMerger(2) → ctx.destination
 *     └── "stereo":       → downmix gain matrix → merger(2) → destination
 *
 * 双耳路径遵循业界标准虚拟音箱方案（杜比 BS.2127 / Apple 虚拟化 5.1）：
 * 对象先 VBAP 到固定虚拟音箱环，每个虚拟音箱与该方向的
 * 「干 HRIR + 湿 BRIR 按模式混合」的 IR 卷积求和。卷积数固定（总线数 × 2 耳），
 * 与对象数量解耦。近/中/远（杜比 Binaural Settings）只控制干/湿 IR 混合；
 * 源侧按 Apple inverse 距离定律处理环外对象。
 *
 * 虚拟监听几何采用 Dolby 家庭布局的声道身份，并在 ITU-R BS.2051 允许范围内
 * 选择固定标称方向。Genelec 文档只规定设备组合与 GLM 校准，不发布这些布局的
 * 专属角度或逐音箱 EQ。双耳路径只保留 120Hz LFE 低通，不套用音箱回放的
 * +10dB LFE 补偿，也不包含主声道的低频重定向。
 */

import { admToSpherical, sphericalToAdm, sphericalToWebAudio, type Spherical } from "./coords.js";
import { HeadPoseTracker, type HeadPose, type HeadPoseOptions } from "./head-pose.js";
import {
  LAYOUT_7_1_4,
  LAYOUTS,
  RENDER_TOPOLOGY,
  DENSE_BINAURAL_FILLS,
  positionForLabel,
  isLfeLabel,
  aliasLabel,
  physicalChannelOrder,
  speakerBusKey,
  type VirtualSpeaker,
} from "./layouts.js";
import { VbapSolver } from "./vbap.js";
import { buildBusIrs, type BinauralIrSet, type BinauralMode } from "./hrtf.js";

type BinauralRenderMode = "off" | "near" | "mid" | "far" | "not-indicated";
import { headphoneProfileById, getHeadphoneCompensationBuffers, type HeadphoneCompensationBuffers, type HeadphoneCompensationProfile } from "./headphone-compensation.js";
import { initCore, VbapBatchSolver, type ObjectEvent } from "@sda/core";

export type OutputMode = "multichannel" | "binaural" | "stereo";

export interface BinauralEqBands {
  low: number;
  mid: number;
  high: number;
}

/** Reversible final-output A/B used to isolate excess low-frequency perception. */
export type BinauralLowFrequencyDiagnostic = "reference" | "low-cut";

interface BinauralEqFilter {
  type: BiquadFilterType;
  frequency: number;
  q: number;
  gain: number;
}

const BINAURAL_EQ_BANDS: ReadonlyArray<Omit<BinauralEqFilter, "gain"> & { band: keyof BinauralEqBands }> = [
  { band: "low", type: "lowshelf", frequency: 120, q: 0.7 },
  { band: "mid", type: "peaking", frequency: 1200, q: 0.8 },
  { band: "high", type: "highshelf", frequency: 6000, q: 0.7 },
];
/** Final binaural A/B only: tests perceived low-frequency excess without touching HRTF or LFE routing. */
const BINAURAL_LOW_DIAGNOSTIC_FILTER = { type: "lowshelf" as const, frequency: 180, q: 0.7, gain: -3 };

/** Genelec 7000-series stereo bass-management crossover default. */
const BASS_MANAGEMENT_CROSSOVER_HZ = 85;
/** Dedicated LFE low-pass: distinct from the stereo bass-management crossover. */
const LFE_LOWPASS_HZ = 120;
/** 双耳耳机路径不套用影院/音箱的 LFE +10dB 回放补偿，避免底鼓过量。 */
const BINAURAL_LFE_INBAND_GAIN = 1;
/** KU100 双耳最终输出标定：补偿主观响度，不改方向 IR 或用户主音量。 */
const BINAURAL_MAKEUP_GAIN = Math.pow(10, 6 / 20);
/** Shared stereo-linked lookahead sample-peak ceiling; no oversampling, so it is not true-peak limiting. */
const BINAURAL_PEAK_GUARD_CEILING_DB = -1;
const BINAURAL_PEAK_GUARD_LOOKAHEAD_S = 0.005;
/** LFE 单独约束峰值，避免底鼓等低频驱动最终全频 safety compressor。 */
const BINAURAL_LFE_PEAK_THRESHOLD_DB = -3;
const BINAURAL_LFE_PEAK_KNEE_DB = 0;
const BINAURAL_LFE_PEAK_RATIO = 8;
const BINAURAL_LFE_PEAK_ATTACK_S = 0.003;
const BINAURAL_LFE_PEAK_RELEASE_S = 0.1;

const BINAURAL_BANKS = ["off", "near", "mid", "far"] as const;
export type BinauralBank = (typeof BINAURAL_BANKS)[number];
/** Chromium/Electron caps a single AudioWorklet output at 32 channels. */
const MAX_WORKLET_OUTPUT_CHANNELS = 32;
const BINAURAL_NOT_INDICATED_DEFAULT: BinauralBank = "mid";
const PCM_RING_SAMPLES = 1 << 18;

function biquadMagnitude(
  type: BiquadFilterType,
  frequency: number,
  q: number,
  gainDb: number,
  sampleRate: number,
  probeFrequency: number,
): number {
  const a = 10 ** (gainDb / 40);
  const w0 = (2 * Math.PI * frequency) / sampleRate;
  const cos0 = Math.cos(w0);
  const sin0 = Math.sin(w0);
  const alpha = type === "peaking" ? sin0 / (2 * q) : sin0 / Math.SQRT2;
  const beta = 2 * Math.sqrt(a) * alpha;
  let b0: number;
  let b1: number;
  let b2: number;
  let a0: number;
  let a1: number;
  let a2: number;
  if (type === "peaking") {
    b0 = 1 + alpha * a;
    b1 = -2 * cos0;
    b2 = 1 - alpha * a;
    a0 = 1 + alpha / a;
    a1 = -2 * cos0;
    a2 = 1 - alpha / a;
  } else if (type === "lowshelf") {
    b0 = a * ((a + 1) - (a - 1) * cos0 + beta);
    b1 = 2 * a * ((a - 1) - (a + 1) * cos0);
    b2 = a * ((a + 1) - (a - 1) * cos0 - beta);
    a0 = (a + 1) + (a - 1) * cos0 + beta;
    a1 = -2 * ((a - 1) + (a + 1) * cos0);
    a2 = (a + 1) + (a - 1) * cos0 - beta;
  } else {
    b0 = a * ((a + 1) + (a - 1) * cos0 + beta);
    b1 = -2 * a * ((a - 1) + (a + 1) * cos0);
    b2 = a * ((a + 1) + (a - 1) * cos0 - beta);
    a0 = (a + 1) - (a - 1) * cos0 + beta;
    a1 = 2 * ((a - 1) - (a + 1) * cos0);
    a2 = (a + 1) - (a - 1) * cos0 - beta;
  }
  const w = (2 * Math.PI * probeFrequency) / sampleRate;
  const numeratorRe = b0 + b1 * Math.cos(w) + b2 * Math.cos(2 * w);
  const numeratorIm = -b1 * Math.sin(w) - b2 * Math.sin(2 * w);
  const denominatorRe = a0 + a1 * Math.cos(w) + a2 * Math.cos(2 * w);
  const denominatorIm = -a1 * Math.sin(w) - a2 * Math.sin(2 * w);
  return Math.hypot(numeratorRe, numeratorIm) / Math.hypot(denominatorRe, denominatorIm);
}

export function binauralEqHeadroomDb(bands: BinauralEqBands, sampleRate = 48000): number {
  const nyquist = sampleRate / 2;
  let maxMagnitude = 1;
  for (let i = 0; i <= 2048; i++) {
    const probeFrequency = 10 * (nyquist / 10) ** (i / 2048);
    let magnitude = 1;
    for (const filter of BINAURAL_EQ_BANDS) {
      magnitude *= biquadMagnitude(filter.type, filter.frequency, filter.q, bands[filter.band], sampleRate, probeFrequency);
    }
    maxMagnitude = Math.max(maxMagnitude, magnitude);
  }
  const maxBoostDb = 20 * Math.log10(maxMagnitude);
  return maxBoostDb > 1e-6 ? -maxBoostDb - 0.2 : 0;
}

/**
 * Stereo (Lo/Ro) downmix per Dolby's documented coefficients:
 *   Lo = L + (−3 dB × C) + (−3 dB × Ls), mirrored for Ro.
 * Direct L/R stay at unity; wides, surrounds, rears and height speakers fold
 * at −3 dB into their ear-level side; LFE enters both channels at −10 dB (the
 * reciprocal of bass management's +10 dB). Coincident-channel peaks are left
 * to the shared output peak guard instead of a blanket trim, so stereo keeps
 * the program's native level.
 */
const STEREO_DOWNMIX_DB = -3;
const STEREO_DOWNMIX_LFE_DB = -10;
const STEREO_DOWNMIX_SIDE = Math.pow(10, STEREO_DOWNMIX_DB / 20);
const STEREO_DOWNMIX_LFE = Math.pow(10, STEREO_DOWNMIX_LFE_DB / 20);

export function stereoDownmixGains(speaker: Pick<VirtualSpeaker, "azimuth" | "isLfe">): [number, number] {
  if (speaker.isLfe) return [STEREO_DOWNMIX_LFE, STEREO_DOWNMIX_LFE];
  const azimuth = speaker.azimuth;
  // Center and rear/top-center speakers feed both channels at −3 dB.
  if (Math.abs(azimuth) < 1 || Math.abs(azimuth) >= 179) return [STEREO_DOWNMIX_SIDE, STEREO_DOWNMIX_SIDE];
  // Main L/R pairs (±30) are the downmix carriers and stay at unity.
  if (Math.abs(azimuth) <= 40) return azimuth > 0 ? [1, 0] : [0, 1];
  // Wides, surrounds, rears and heights fold at −3 dB into their own side.
  return azimuth > 0 ? [STEREO_DOWNMIX_SIDE, 0] : [0, STEREO_DOWNMIX_SIDE];
}

export function virtualLayoutForOutput(
  layout: readonly VirtualSpeaker[],
  _mode: OutputMode,
): readonly VirtualSpeaker[] {
  return layout;
}

function binauralBank(mode: BinauralRenderMode | undefined, fallback: BinauralMode): BinauralBank {
  if (mode === "off" || mode === "near" || mode === "mid" || mode === "far") return mode;
  return mode === "not-indicated" ? BINAURAL_NOT_INDICATED_DEFAULT : fallback;
}

export interface RendererStats {
  underrunSamples: number;
  rejectedBatches: number;
  rejectedSources: number;
  /** Output-side process() gaps above the broad 12ms diagnostic threshold. */
  callbackGaps?: number;
  /** Subset of callbackGaps strictly above 25ms, eligible for FIFO escalation. */
  callbackGapsOver25Ms?: number;
  callbackGapMaxMs?: number;
}

export interface BinauralBankHealth {
  bank: BinauralBank;
  /** Measured spatial convolution nodes. LFE and the `off` bank are excluded. */
  spatialConvolutions: number;
  /** Non-convolution direct paths, currently used by the `off` bank. */
  directPaths: number;
}

export interface BinauralHealthTelemetry {
  activeBankCount: number;
  banks: readonly BinauralBankHealth[];
  totalSpatialConvolutions: number;
  totalDirectPaths: number;
}

export interface RendererOptions {
  mode?: OutputMode;
  layout?: readonly VirtualSpeaker[];
  /** 预加载的双耳 IR 集（SADIE II KU100 派生）；也可 init 后 setBinauralData。
   *  缺省时双耳模式回退到浏览器内置 PannerNode HRTF。 */
  binauralIrSet?: BinauralIrSet;
  /** 密集球面 IR 集（逐对象精确方向渲染用，可选开启）。 */
  denseBinauralIrSet?: BinauralIrSet;
  /** 开启后对象在密集球面上按精确方向落位（实验性，CPU 更高）。 */
  denseBinauralObjects?: boolean;
  /** Device-neutral ADM head-to-world pose processing policy. */
  headPose?: HeadPoseOptions;
  /** worklet 每消耗约 1/8 秒回调一次 —— 播放器用它泵入更多 PCM（背压）。 */
  onConsumedTick?: (stats: RendererStats) => void;
  /** Throttled post-gain/post-mute object activity from the render worklet. */
  onObjectActivity?: (ids: readonly number[]) => void;
  onBatchResult?: (result: { sequence: number; accepted: boolean; samples: number; reason?: string }) => void;
}

/** Scalar spread derived from ADM object size (w, d, h in [0,1]). */
function sizeToSpread(size: [number, number, number]): number {
  return Math.min(1, (size[0] + size[1] + size[2]) / 3);
}

interface ScheduledGainMessage {
  type: "gains" | "scheduleGains";
  id: string;
  at?: number;
  gains: Float32Array;
  gain: number;
  lp: number;
  ramp: number;
  /** A scheduled object route that live head tracking must not overwrite. */
  poseControlled?: boolean;
  /** A live pose refresh changes only the spatial route, not metadata gain. */
  poseUpdate?: boolean;
}

interface SourceState {
  id: string;
  spread: number;
  position: Spherical;
  gainDb: number;
  /** At least one codec object event has established this source's target. */
  hasObjectMetadata: boolean;
  /** Sample where the last accepted object ramp reaches its target. */
  objectRampEndSample: number;
  isLfe: boolean;
  /** 对象静音（mute/solo）：静音时标量增益乘 0，走平滑斜坡无爆音。 */
  muted: boolean;
  /** 床声道的规范标签；运行时切布局时重新寻找吸附总线。 */
  bedLabel?: string;
  /** 床声道吸附的逻辑布局音箱索引（-1 = 布局中无此音箱，回退 VBAP）。 */
  snapBus: number;
  lfeBus?: number;
  binauralMode?: BinauralRenderMode;
  lifecycleEvents: { at: number; active: boolean; order: number }[];
  lifecycleEventOrder: number;
  /** Canonical object targets keyed to codec time. Pose refreshes update both
   * the currently audible route and already-buffered future route changes. */
  objectPoseTimeline: {
    at: number;
    fromPosition: Spherical;
    position: Spherical;
    fromSpread: number;
    spread: number;
    gainDb: number;
    rampSamples: number;
  }[];
}

export function interpolateObjectPosition(from: Spherical, to: Spherical, progress: number): Spherical {
  const amount = Math.min(1, Math.max(0, progress));
  if (amount <= 0) return from;
  if (amount >= 1) return to;
  const start = sphericalToAdm(from);
  const end = sphericalToAdm(to);
  return admToSpherical([
    start[0] + (end[0] - start[0]) * amount,
    start[1] + (end[1] - start[1]) * amount,
    start[2] + (end[2] - start[2]) * amount,
  ]);
}

export class SpatialRenderer {
  readonly ctx: AudioContext;
  /** 当前用于 VBAP 与床层语义的布局；运行中可切换。 */
  layout: readonly VirtualSpeaker[];
  /** Active gain-vector layout. Geometry is identical in every output mode. */
  private renderLayout: readonly VirtualSpeaker[];
  /** Current logical-layout bus -> fixed worklet topology bus. Rebuilt only when
   * the layout changes, never while processing object motion. */
  private renderToTopology: Int16Array;
  /** 固定的最大总线拓扑。AudioWorklet 保持存活；双耳后级只接当前布局使用的 bus。
   *  末尾追加的密集"双耳专用"总线仅服务逐对象精确方向渲染，立体声/多声道不映射它们。 */
  private readonly topology: readonly VirtualSpeaker[];
  mode: OutputMode;
  /** 三条常驻模式路径的最终增益，实时切换只对它们做交叉淡化。 */
  private modeGains = new Map<OutputMode, GainNode>();
  private modeVolumeGains = new Map<OutputMode, GainNode>();
  private modeProgramGains = new Map<OutputMode, GainNode>();
  private multichannelOutput: GainNode | null = null;
  private multichannelProjector: { id: string; gain: GainNode; nodes: AudioNode[] } | null = null;
  private volume = 1;
  private volumeBalanceEnabled = false;
  private programLoudnessGainDb: number | null = null;
  private vbap: VbapSolver;
  /** Optional Rust/WASM batch solver. The TypeScript solver remains the
   * correctness fallback until the WASM core is loaded and for unsupported calls. */
  private wasmVbap: VbapBatchSolver | null = null;
  private wasmVbapLayoutRevision = 0;
  /** 开启后对象在密集球面按精确方向落位（仅双耳输出）。 */
  private denseBinauralObjects = false;
  /** 密集球面 IR 集（hrtf-dense，61 个测量方向），仅密集模式挂载。 */
  private denseIrSet: BinauralIrSet | null = null;
  /** Pose changes only recompute source gain vectors; they never touch the graph. */
  private readonly headPose: HeadPoseTracker;
  private poseUpdateTimer: ReturnType<typeof setTimeout> | null = null;
  private poseStaleTimer: ReturnType<typeof setTimeout> | null = null;
  private lastPoseGainUpdateMs = Number.NEGATIVE_INFINITY;
  private poseControlEnabled = false;
  private node: AudioWorkletNode | null = null;
  /** 常驻最终 sample-peak guard；后级图重建时复用，不触碰播放时间线。 */
  private peakGuard: AudioWorkletNode | null = null;
  private master: GainNode | null = null;
  private postNodes: AudioNode[] = [];
  /** Per-bank binaural convolution nodes, keyed by fixed topology bus. */
  private convs = new Map<BinauralBank, Map<number, ConvolverNode | null>>();
  private binauralMerger: ChannelMergerNode | null = null;
  private binauralLfeInput: GainNode | null = null;
  /** Nodes owned only by the replaceable binaural bus graph. */
  private binauralBusNodes: AudioNode[] = [];
  /** Worklet output connections that must be explicitly detached on layout swap. */
  private binauralBankSplitters = new Map<BinauralBank, ChannelSplitterNode>();
  /** Bus identity sequence that owns the current replaceable binaural graph. */
  private binauralBusKeySequence = "";
  private sources = new Map<string, SourceState>();
  private retiringSources = new Map<string, number>();
  private nextRetirementToken = 1;
  /** 独立 LFE 床声道的静音状态；与动态对象静音分开存储。 */
  private lfeMuted = false;
  private irSet: BinauralIrSet | null = null;
  /** 床扩展表（AVR 上混器语义）：床音箱总线 → 派生馈送。内容床小于所选布局时
   *  把床填满布局 —— 侧环绕馈后环、前馈前宽；目标总线已被真实床声道占用则跳过。 */
  private expansion = new Map<number, { bus: number; gain: number }[]>();
  /** 杜比 Binaural Settings 语义：虚拟音箱参考距离。UI 固定"近"（0.7m）；
   *  mid/far 机制保留在引擎内，暂不从界面暴露。 */
  private binauralMode: BinauralMode = "near";
  /** 最终双耳回放补偿。无 profile 时是 literal bypass。 */
  private headphoneProfileId: string | null = null;
  /** User-controlled final 3-band EQ. Never affects stereo or physical multichannel output. */
  private binauralEqBands: BinauralEqBands = { low: 0, mid: 0, high: 0 };
  /** Reversible final-output A/B; default reference is a literal bypass. */
  private binauralLowFrequencyDiagnosticMode: BinauralLowFrequencyDiagnostic = "reference";
  /** 常驻最终双耳 EQ；实时滑动只改这些 AudioParam，不重建输出图。 */
  private binauralEqNodes = new Map<keyof BinauralEqBands, [BiquadFilterNode, BiquadFilterNode]>();
  /** Persistent linked L/R low-shelf used only for the final-output diagnostic A/B. */
  private binauralLowDiagnosticNodes: [BiquadFilterNode, BiquadFilterNode] | null = null;
  private binauralEqHeadroom: GainNode | null = null;
  /** 当前输出图 revision；迟到的 FIR 请求不得接回已重建的图。 */
  private outputGraphRevision = 0;
  /** 已就绪的 context-local FIR buffers；切 profile 或重建 context 时清空。 */
  private headphoneBuffers: HeadphoneCompensationBuffers | null = null;
  /** Persistent final-output dry/wet insert. Profile changes never rebuild spatial BRIR paths. */
  private headphoneDry: [GainNode, GainNode] | null = null;
  private headphoneWet: [GainNode, GainNode] | null = null;
  private headphoneInput: ChannelSplitterNode | null = null;
  private headphoneOutput: ChannelMergerNode | null = null;
  private headphonePreamp: [GainNode, GainNode] | null = null;
  private headphoneConvolvers: [ConvolverNode, ConvolverNode] | null = null;
  private onConsumedTick?: (stats: RendererStats) => void;
  private onObjectActivity?: (ids: readonly number[]) => void;
  private onBatchResult?: (result: { sequence: number; accepted: boolean; samples: number; reason?: string }) => void;
  /** Frames actually rendered by the worklet (authoritative playhead). */
  consumedSamples = 0;
  /** Reset generation. Only ticks from the active generation may move the playhead. */
  private epoch = 0;

  constructor(ctx: AudioContext, options: RendererOptions = {}) {
    this.ctx = ctx;
    this.mode = options.mode ?? "binaural";
    this.layout = options.layout ?? LAYOUT_7_1_4;
    this.denseBinauralObjects = options.denseBinauralObjects === true;
    this.denseIrSet = options.denseBinauralIrSet ?? null;
    this.topology = [...RENDER_TOPOLOGY, ...DENSE_BINAURAL_FILLS];
    this.renderLayout = virtualLayoutForOutput(this.layout, this.mode);
    this.renderToTopology = this.buildRenderProjection();
    this.vbap = new VbapSolver(this.renderLayout);
    this.headPose = new HeadPoseTracker(options.headPose);
    this.refreshWasmVbap();
    if (options.binauralIrSet) this.irSet = options.binauralIrSet;
    this.onConsumedTick = options.onConsumedTick;
    this.onObjectActivity = options.onObjectActivity;
    this.onBatchResult = options.onBatchResult;
    this.buildExpansion();
  }

  /** 上混扩展规则（杜比 DSU / AVR 上混器的静态近似）：
   *  - 侧环绕 → 后环 0.5（5.1 内容在 7.1+ 布局：后环不再沉默，声像略后移
   *    恰好贴近 5.1 环绕的 ±110° 制作位）
   *  - 前左/右 → 前宽 0.35（9.1 布局：拉开前声场宽度，中置对白不动）
   *  顶层不做静态派生（环境声提取超出本渲染器职责）。 */
  private buildExpansion(): void {
    this.expansion.clear();
    const idx = (name: string) => this.layout.findIndex((s) => s.name === name);
    const feed = (from: string, to: string, gain: number) => {
      const a = idx(from);
      const b = idx(to);
      if (a < 0 || b < 0) return;
      const list = this.expansion.get(a) ?? [];
      list.push({ bus: b, gain });
      this.expansion.set(a, list);
    };
    feed("SurroundLeft", "RearLeft", 0.5);
    feed("SurroundRight", "RearRight", 0.5);
    feed("FrontLeft", "WideLeft", 0.35);
    feed("FrontRight", "WideRight", 0.35);
  }

  private updateDestinationChannelCount(): void {
    const destination = this.ctx.destination;
    try {
      destination.channelCountMode = "explicit";
      destination.channelCount = Math.max(2, Math.min(this.layout.length, destination.maxChannelCount || this.layout.length));
    } catch {
      /* Device does not allow an explicit channel count. */
    }
  }

  private layoutId(layout: readonly VirtualSpeaker[]): string {
    return layout.map(speakerBusKey).join(",");
  }

  private retirePostNodes(nodes: readonly AudioNode[], delayMs: number): void {
    globalThis.setTimeout(() => {
      nodes.forEach((node) => node.disconnect());
      const retired = new Set(nodes);
      this.postNodes = this.postNodes.filter((node) => !retired.has(node));
    }, delayMs);
  }

  /** True 2.1 monitoring: a subwoofer feed is derived from the low-frequency
   * main program content. It is separate from a discrete source LFE channel,
   * which receives its own 120 Hz low-pass before both feeds meet at the sub. */
  private createBassManaged21Projector(layout: readonly VirtualSpeaker[], initialGain: number) {
    const output = this.multichannelOutput;
    if (!this.node || !output) return null;
    const left = layout.findIndex((speaker) => speaker.name === "FrontLeft");
    const right = layout.findIndex((speaker) => speaker.name === "FrontRight");
    const lfe = layout.findIndex((speaker) => speaker.isLfe);
    if (left < 0 || right < 0 || lfe < 0) return null;
    const topologyBus = (layoutBus: number) => this.topology.findIndex(
      (speaker) => speakerBusKey(speaker) === speakerBusKey(layout[layoutBus]!),
    );
    const leftBus = topologyBus(left);
    const rightBus = topologyBus(right);
    const lfeBus = topologyBus(lfe);
    if (leftBus < 0 || rightBus < 0 || lfeBus < 0) return null;

    const nodes: AudioNode[] = [];
    const merger = this.ctx.createChannelMerger(3);
    BINAURAL_BANKS.forEach((_bank, outputIndex) => {
      const splitter = this.ctx.createChannelSplitter(this.topology.length);
      this.node!.connect(splitter, outputIndex);
      for (const [bus, outputChannel] of [[leftBus, 0], [rightBus, 1]] as const) {
        const [hpIn, hpOut] = this.lr4("highpass", BASS_MANAGEMENT_CROSSOVER_HZ);
        splitter.connect(hpIn, bus);
        hpOut.connect(merger, 0, outputChannel);
        nodes.push(hpIn, hpOut);
      }
      const subSum = this.ctx.createGain();
      // Equal-power summing prevents correlated mono bass from clipping before
      // the user-controlled master stage.
      subSum.gain.value = Math.SQRT1_2;
      for (const bus of [leftBus, rightBus]) {
        const [lpIn, lpOut] = this.lr4("lowpass", BASS_MANAGEMENT_CROSSOVER_HZ);
        splitter.connect(lpIn, bus);
        lpOut.connect(subSum);
        nodes.push(lpIn, lpOut);
      }
      const [lfeIn, lfeOut] = this.lr4("lowpass", LFE_LOWPASS_HZ);
      splitter.connect(lfeIn, lfeBus);
      lfeOut.connect(subSum);
      subSum.connect(merger, 0, 2);
      nodes.push(splitter, subSum, lfeIn, lfeOut);
    });
    const gain = this.ctx.createGain();
    gain.gain.value = initialGain;
    merger.connect(gain);
    gain.connect(output);
    nodes.push(merger, gain);
    this.postNodes.push(...nodes);
    return { id: this.layoutId(layout), gain, nodes };
  }

  private createMultichannelProjector(layout: readonly VirtualSpeaker[], initialGain: number) {
    if (this.layoutId(layout) === this.layoutId(LAYOUTS["2.1"])) {
      return this.createBassManaged21Projector(layout, initialGain);
    }
    const output = this.multichannelOutput;
    if (!this.node || !output) return null;
    const nodes: AudioNode[] = [];
    const merger = this.ctx.createChannelMerger(layout.length);
    BINAURAL_BANKS.forEach((_bank, outputIndex) => {
      const splitter = this.ctx.createChannelSplitter(this.topology.length);
      this.node!.connect(splitter, outputIndex);
      physicalChannelOrder(layout).forEach((layoutBus, channel) => {
        const topologyBus = this.topology.findIndex(
          (speaker) => speakerBusKey(speaker) === speakerBusKey(layout[layoutBus]!),
        );
        if (topologyBus >= 0) splitter.connect(merger, topologyBus, channel);
      });
      nodes.push(splitter);
    });
    const gain = this.ctx.createGain();
    gain.gain.value = initialGain;
    merger.connect(gain);
    gain.connect(output);
    nodes.push(merger, gain);
    this.postNodes.push(...nodes);
    return { id: this.layoutId(layout), gain, nodes };
  }

  private updateMultichannelLayout(): void {
    const id = this.layoutId(this.layout);
    if (!this.multichannelOutput || this.multichannelProjector?.id === id) return;
    const next = this.createMultichannelProjector(this.layout, 0);
    if (!next) return;
    const previous = this.multichannelProjector;
    this.multichannelProjector = next;
    const now = this.ctx.currentTime;
    next.gain.gain.setValueAtTime(0, now);
    next.gain.gain.linearRampToValueAtTime(1, now + 0.05);
    if (previous) {
      previous.gain.gain.cancelScheduledValues(now);
      previous.gain.gain.setValueAtTime(previous.gain.gain.value, now);
      previous.gain.gain.linearRampToValueAtTime(0, now + 0.05);
      this.retirePostNodes(previous.nodes, 100);
    }
  }

  private buildRenderProjection(): Int16Array {
    const topologyByKey = new Map(
      this.topology.map((speaker, index) => [speakerBusKey(speaker), index]),
    );
    return Int16Array.from(
      this.renderLayout.map((speaker) => topologyByKey.get(speakerBusKey(speaker)) ?? -1),
    );
  }

  /** Dense binaural render layout: logical layout speakers plus dense fills that
   * are not already within ~10° of a layout speaker (so bed channels keep their
   * exact buses while objects gain precise directions). Only used for binaural. */
  private denseBinauralLayout(): readonly VirtualSpeaker[] {
    const base = virtualLayoutForOutput(this.layout, "binaural");
    const fills = DENSE_BINAURAL_FILLS.filter((fill) => !base.some((speaker) => {
      if (speaker.isLfe) return false; // LFE has no direction; never evicts a fill.
      const dAz = Math.abs(((speaker.azimuth - fill.azimuth + 540) % 360) - 180);
      return dAz < 10 && Math.abs(speaker.elevation - fill.elevation) < 10;
    }));
    return [...base, ...fills];
  }

  /** Build the optional Rust/WASM geometry solver for the active logical layout.
   * Layout revision guards keep a slow async core initialization from replacing a
   * newer layout's solver. Any failure deliberately preserves the JS fallback. */
  private refreshWasmVbap(): void {
    const revision = ++this.wasmVbapLayoutRevision;
    this.wasmVbap?.free();
    this.wasmVbap = null;
    const directions = new Float64Array(this.renderLayout.length * 3);
    const lfeMask = new Uint8Array(this.renderLayout.length);
    const azimuths = new Float64Array(this.renderLayout.length);
    this.renderLayout.forEach((speaker, index) => {
      const [x, y, z] = sphericalToAdm(speaker);
      directions.set([x, y, z], index * 3);
      lfeMask[index] = speaker.isLfe ? 1 : 0;
      azimuths[index] = speaker.azimuth;
    });
    void initCore()
      .then(() => {
        if (revision !== this.wasmVbapLayoutRevision) return;
        this.wasmVbap = new VbapBatchSolver(directions, lfeMask, azimuths);
      })
      .catch((error) => console.warn("[SDA] Rust VBAP 不可用，保持 TypeScript 回退:", error));
  }

  /** Batch spatial gain vectors for same-layout object metadata. Pose updates
   * intentionally stay on the direct JS path because they are wall-clock live. */
  private panObjectBatch(states: readonly SourceState[]): readonly Float32Array[] | null {
    const solver = this.wasmVbap;
    if (!solver || states.length === 0) return null;
    try {
      const positions = new Float64Array(states.length * 3);
      const spreads = new Float64Array(states.length);
      states.forEach((state, index) => {
        const [x, y, z] = sphericalToAdm(state.position);
        positions.set([x, y, z], index * 3);
        spreads[index] = state.spread;
      });
      const packed = solver.panBatch(positions, spreads);
      if (packed.length !== states.length * this.renderLayout.length) return null;
      return states.map((_, index) => packed.subarray(
        index * this.renderLayout.length,
        (index + 1) * this.renderLayout.length,
      ));
    } catch (error) {
      console.warn("[SDA] Rust VBAP 批处理失败，保持 TypeScript 回退:", error);
      return null;
    }
  }

  private updateRenderLayout(): void {
    this.renderLayout = this.mode === "binaural" && this.denseBinauralObjects
      ? this.denseBinauralLayout()
      : virtualLayoutForOutput(this.layout, this.mode);
    this.renderToTopology = this.buildRenderProjection();
    this.vbap = new VbapSolver(this.renderLayout);
    this.refreshWasmVbap();
    for (const state of this.sources.values()) {
      if (state.bedLabel && !state.isLfe) {
        state.snapBus = this.renderLayout.findIndex((speaker) => speaker.name === state.bedLabel);
      }
    }
  }

  /** 改变逻辑布局而不重建 AudioContext/worklet。现有 PCM、播放头继续存活；
   * 双耳图只重建当前布局实际使用的 bus，并立即断开旧节点。 */
  setLayout(layout: readonly VirtualSpeaker[]): void {
    if (layout === this.layout) return;
    this.layout = layout;
    this.updateRenderLayout();
    this.buildExpansion();
    this.updateMultichannelLayout();
    this.rebuildBinauralBusGraph();
    this.updateDestinationChannelCount();
    for (const state of this.sources.values()) {
      this.applyGains(state, 2048);
    }
  }

  async init(workletModuleUrl: string | URL): Promise<void> {
    if (this.topology.length > MAX_WORKLET_OUTPUT_CHANNELS) {
      throw new Error(`双耳 worklet 总线数 ${this.topology.length} 超出 ${MAX_WORKLET_OUTPUT_CHANNELS} 路平台上限`);
    }
    await this.ctx.audioWorklet.addModule(workletModuleUrl);
    this.master = this.ctx.createGain();
    this.master.connect(this.ctx.destination);
    this.node = new AudioWorkletNode(this.ctx, "sda-renderer", {
      numberOfInputs: 0,
      numberOfOutputs: BINAURAL_BANKS.length,
      outputChannelCount: BINAURAL_BANKS.map(() => this.topology.length),
      processorOptions: { busCount: this.topology.length, epoch: this.epoch },
    });
    if (this.poseControlEnabled) this.node.port.postMessage({ type: "headTracking", enabled: true });
    this.node.port.onmessage = (e: MessageEvent) => {
      if (e.data?.type === "ready") {
        console.log(`[SDA] audio worklet ${String(e.data.build ?? "unknown")} ring=${e.data.ringSize}`);
      } else if (e.data?.type === "tick" && e.data.epoch === this.epoch) {
        this.consumedSamples = e.data.consumed;
        this.onConsumedTick?.({
          underrunSamples: Number(e.data.underrunSamples) || 0,
          rejectedBatches: Number(e.data.rejectedBatches) || 0,
          rejectedSources: Number(e.data.rejectedSources) || 0,
          callbackGaps: Number(e.data.callbackGaps) || 0,
          callbackGapsOver25Ms: Number(e.data.callbackGapsOver25Ms) || 0,
          callbackGapMaxMs: Number(e.data.callbackGapMaxMs) || 0,
        });
        const activeObjectIds = Array.isArray(e.data.activeObjectIds)
          ? e.data.activeObjectIds.map(Number).filter(Number.isSafeInteger)
          : [];
        this.onObjectActivity?.(activeObjectIds);
      } else if (e.data?.type === "sourceRetired") {
        const id = String(e.data.id ?? "");
        const token = Number(e.data.token);
        if (this.retiringSources.get(id) === token) {
          this.retiringSources.delete(id);
          this.sources.delete(id);
        }
      } else if (e.data?.type === "batchAck") {
        this.onBatchResult?.({ sequence: e.data.sequence, accepted: true, samples: e.data.samples });
      } else if (e.data?.type === "batchRejected") {
        this.onBatchResult?.({ sequence: e.data.sequence, accepted: false, samples: 0, reason: String(e.data.reason ?? "unknown") });
      }
    };
    this.peakGuard = new AudioWorkletNode(this.ctx, "sda-final-peak-guard", {
      numberOfInputs: 1,
      numberOfOutputs: 1,
      outputChannelCount: [2],
      processorOptions: { ceilingDb: BINAURAL_PEAK_GUARD_CEILING_DB },
    });
    this.peakGuard.port.postMessage({ type: "programEnabled", enabled: this.volumeBalanceEnabled });
    this.peakGuard.port.postMessage({
      type: "programGain",
      gain: this.programLoudnessGainDb === null ? 1 : Math.pow(10, this.programLoudnessGainDb / 20),
    });
    this.buildOutputGraph();
  }

  /** 注入双耳 IR 集；双耳路径常驻，即使当前未选双耳也立即更新，便于实时切回。 */
  setBinauralData(set: BinauralIrSet): void {
    this.irSet = set;
    if (this.node) this.buildOutputGraph();
  }

  /** True only after the measured binaural IR set has replaced browser Panner fallback. */
  get hasBinauralData(): boolean {
    return this.irSet !== null;
  }

  /** 注入密集球面 IR 集（逐对象精确方向渲染）。密集模式已开启时强制重建双耳总线，
   * 让填充方向从"最近床层方向"切到密集集里的精确方向。 */
  setDenseBinauralIrSet(set: BinauralIrSet | null): void {
    // Dense directions share the same final L/R merger as calibrated bed buses.
    // Reject an uncalibrated set instead of mixing two incompatible direct/tail
    // reference systems, which is especially destructive with many objects.
    if (set && !set.calibrated) {
      console.warn("[SDA] 拒绝未校准 dense HRTF：保持标准 KU100 双耳路径");
      this.denseIrSet = null;
      if (this.denseBinauralObjects) this.setDenseBinauralObjects(false);
      return;
    }
    this.denseIrSet = set;
    if (this.node && this.denseBinauralObjects) this.rebuildBinauralBusGraph(true);
  }

  get hasDenseBinauralData(): boolean {
    return this.denseIrSet !== null;
  }

  /** 逐对象精确方向渲染开关（仅双耳输出生效）。开启后对象 VBAP 到密集球面：
   * 落点不再吸附到床层扬声器方向，填充总线用密集 IR 集卷积；床层声道不变。 */
  setDenseBinauralObjects(enabled: boolean): void {
    if (enabled === this.denseBinauralObjects) return;
    this.denseBinauralObjects = enabled;
    this.updateRenderLayout();
    this.rebuildBinauralBusGraph();
    for (const state of this.sources.values()) {
      this.applyGains(state, 2048);
    }
  }

  get denseBinauralObjectsEnabled(): boolean {
    return this.denseBinauralObjects;
  }

  /** 切换杜比近/中/远：重混每总线 IR（干 HRIR ↔ 湿 BRIR）；对象的空间位置和
   * 制作响度不变，播放不中断。 */
  setBinauralMode(mode: BinauralMode): void {
    if (mode === this.binauralMode) return;
    this.binauralMode = mode;
    this.buildBinauralBank(mode);
    const bank = BINAURAL_BANKS.indexOf(mode);
    for (const state of this.sources.values()) {
      if (!state.binauralMode) this.node?.port.postMessage({ type: "binauralMode", id: state.id, bank });
    }
  }

  get binauralModeName(): BinauralMode {
    return this.binauralMode;
  }

  /** Apply static program-level DBMD metadata. It is deliberately not tied to
   * sample events: Dolby Binaural Render Mode is not automatable. */
  setSourceBinauralMode(id: string, mode: BinauralRenderMode): boolean {
    const state = this.sources.get(id);
    if (!state) return false;
    state.binauralMode = mode;
    const bankName = binauralBank(mode, this.binauralMode);
    const bank = BINAURAL_BANKS.indexOf(bankName);
    this.buildBinauralBank(bankName);
    this.node?.port.postMessage({ type: "binauralMode", id, bank });
    return true;
  }

  /** Select final binaural compensation without rebuilding spatial BRIR paths. */
  setHeadphoneCompensation(profileId: string | null): void {
    if (profileId !== null && !headphoneProfileById(profileId)) {
      throw new Error(`未知或未注册的耳机补偿 profile: ${profileId}`);
    }
    this.headphoneProfileId = profileId;
    if (!profileId) {
      this.headphoneBuffers = null;
      const retired = [
        ...(this.headphoneWet ?? []),
        ...(this.headphonePreamp ?? []),
        ...(this.headphoneConvolvers ?? []),
      ];
      this.crossfadeHeadphoneCompensation(false);
      this.headphoneWet = null;
      this.headphonePreamp = null;
      this.headphoneConvolvers = null;
      this.retirePostNodes(retired, 250);
      return;
    }
    this.headphoneBuffers = null;
    this.loadHeadphoneCompensation();
  }

  get headphoneCompensationProfile(): HeadphoneCompensationProfile | null {
    return headphoneProfileById(this.headphoneProfileId);
  }

  private setEqHeadroom(bands: BinauralEqBands): void {
    if (!this.binauralEqHeadroom) return;
    const attenuationDb = binauralEqHeadroomDb(bands, this.ctx.sampleRate);
    const target = Math.pow(10, attenuationDb / 20);
    const now = this.ctx.currentTime;
    const gain = this.binauralEqHeadroom.gain;
    gain.cancelScheduledValues(now);
    gain.setValueAtTime(gain.value, now);
    gain.linearRampToValueAtTime(target, now + (target < gain.value ? 0.01 : 0.1));
  }

  /** 设置最终双耳输出的低、中、高三段连续 EQ（-12 至 +12 dB）。 */
  setBinauralEqBands(bands: BinauralEqBands): void {
    const next = {
      low: Math.max(-12, Math.min(12, bands.low)),
      mid: Math.max(-12, Math.min(12, bands.mid)),
      high: Math.max(-12, Math.min(12, bands.high)),
    };
    if (next.low === this.binauralEqBands.low && next.mid === this.binauralEqBands.mid && next.high === this.binauralEqBands.high) return;
    this.binauralEqBands = next;
    this.setEqHeadroom(next);
    const now = this.ctx.currentTime;
    for (const [band, nodes] of this.binauralEqNodes) {
      for (const node of nodes) {
        node.gain.cancelScheduledValues(now);
        node.gain.setValueAtTime(node.gain.value, now);
        node.gain.linearRampToValueAtTime(next[band], now + 0.04);
      }
    }
  }

  get binauralEq(): Readonly<BinauralEqBands> {
    return this.binauralEqBands;
  }

  /** 切换最终双耳低频诊断；只自动化左右链接的最终 shelf，不重建空间图或播放头。 */
  setBinauralLowFrequencyDiagnostic(mode: BinauralLowFrequencyDiagnostic): void {
    if (mode === this.binauralLowFrequencyDiagnosticMode) return;
    this.binauralLowFrequencyDiagnosticMode = mode;
    const targetDb = mode === "low-cut" ? BINAURAL_LOW_DIAGNOSTIC_FILTER.gain : 0;
    const now = this.ctx.currentTime;
    for (const node of this.binauralLowDiagnosticNodes ?? []) {
      node.gain.cancelScheduledValues(now);
      node.gain.setValueAtTime(node.gain.value, now);
      node.gain.linearRampToValueAtTime(targetDb, now + 0.04);
    }
  }

  get binauralLowFrequencyDiagnostic(): BinauralLowFrequencyDiagnostic {
    return this.binauralLowFrequencyDiagnosticMode;
  }

  /** 实时切换最终输出模式。三条后级图保持常驻，worklet/PCM/播放头不重建。 */
  setOutputMode(mode: OutputMode): void {
    if (mode === this.mode) return;
    const graphReady = this.modeGains.size > 0;
    if (graphReady) {
      const now = this.ctx.currentTime;
      const duration = 0.05;
      for (const [id, gain] of this.modeGains) {
        const target = id === mode ? 1 : 0;
        gain.gain.cancelScheduledValues(now);
        gain.gain.setValueAtTime(gain.gain.value, now);
        gain.gain.linearRampToValueAtTime(target, now + duration);
      }
    }
    this.mode = mode;
    this.updateRenderLayout();
    // A mode can select a different logical layout even when its speaker count
    // is unchanged. Rebuild only if the fixed bus identity sequence changed.
    this.rebuildBinauralBusGraph();
    this.syncPoseControl(this.mode === "binaural" && this.headPose.isActive(performance.now()));
    // 床层扩展只属于物理多声道。输出模式切换后必须重推 gains，不能沿用
    // 旧模式的前宽/后环派生馈送。
    for (const state of this.sources.values()) this.applyGains(state, 2048);
  }

  get outputMode(): OutputMode {
    return this.mode;
  }

  /** Apply a calibrated canonical ADM head-to-world pose. Pose updates use
   * immediate worklet gain ramps, deliberately never rewriting codec-timeline
   * metadata that may already be scheduled in the future. */
  setHeadPose(pose: HeadPose): boolean {
    const now = performance.now();
    if (!this.headPose.set(pose, now)) return false;
    this.syncPoseControl(this.mode === "binaural");
    this.schedulePoseGainUpdate(now);
    if (this.poseStaleTimer !== null) globalThis.clearTimeout(this.poseStaleTimer);
    this.poseStaleTimer = globalThis.setTimeout(() => {
      this.poseStaleTimer = null;
      // isActive() clears a stale orientation; force one neutral gain refresh.
      if (!this.headPose.isActive(performance.now())) this.schedulePoseGainUpdate(performance.now(), true);
    }, this.headPose.options.staleAfterMs + 1);
    return true;
  }

  /** Disable head tracking and smoothly return world-locked sources to neutral. */
  clearHeadPose(): void {
    if (this.poseStaleTimer !== null) {
      globalThis.clearTimeout(this.poseStaleTimer);
      this.poseStaleTimer = null;
    }
    if (!this.headPose.clear()) return;
    this.syncPoseControl(false);
    this.schedulePoseGainUpdate(performance.now(), true);
  }

  /** Set the current active head orientation as the neutral viewing direction. */
  recenterHeadPose(): boolean {
    if (!this.headPose.recenter()) return false;
    this.schedulePoseGainUpdate(performance.now(), true);
    return true;
  }

  private schedulePoseGainUpdate(now: number, force = false): void {
    const interval = 1000 / this.headPose.options.updateHz;
    const wait = force ? 0 : Math.max(0, interval - (now - this.lastPoseGainUpdateMs));
    if (this.poseUpdateTimer !== null) return;
    this.poseUpdateTimer = globalThis.setTimeout(() => {
      this.poseUpdateTimer = null;
      this.lastPoseGainUpdateMs = performance.now();
      // Stale input automatically disables tracking here as well, so a provider
      // disappearing returns to the unrotated world rather than freezing pose.
      const trackingActive = this.mode === "binaural" && this.headPose.isActive(this.lastPoseGainUpdateMs);
      this.syncPoseControl(trackingActive);
      const futurePoseMessages: ScheduledGainMessage[] = [];
      for (const state of this.sources.values()) {
        if (state.isLfe) continue;
        // Object events are often prebuffered seconds ahead. Select the last
        // target due at the rendered codec cursor instead of using `state`'s
        // newest (possibly future) scheduled metadata.
        let immediate = state;
        if (!state.bedLabel && state.objectPoseTimeline.length > 0) {
          let dueIndex = -1;
          for (let index = 0; index < state.objectPoseTimeline.length; index++) {
            if (state.objectPoseTimeline[index]!.at > this.consumedSamples) break;
            dueIndex = index;
          }
          if (dueIndex >= 0) {
            if (dueIndex >= 64) {
              state.objectPoseTimeline.splice(0, dueIndex);
              dueIndex = 0;
            }
            const due = state.objectPoseTimeline[dueIndex]!;
            const progress = Math.min(1, Math.max(
              0,
              (this.consumedSamples - due.at) / due.rampSamples,
            ));
            immediate = {
              ...state,
              position: interpolateObjectPosition(due.fromPosition, due.position, progress),
              spread: due.fromSpread + (due.spread - due.fromSpread) * progress,
              gainDb: due.gainDb,
            };
          }
          if (trackingActive) {
            // Audio continues if Electron's main thread is briefly descheduled.
            // Keep every already-buffered metadata boundary paired with a
            // head-relative route so the worklet never waits on the next timer.
            for (let index = dueIndex + 1; index < state.objectPoseTimeline.length; index++) {
              const target = state.objectPoseTimeline[index]!;
              const future = { ...state, position: target.position, spread: target.spread, gainDb: target.gainDb };
              futurePoseMessages.push(this.gainMessage(future, target.rampSamples, target.at, true));
            }
          }
          if (dueIndex < 0) continue;
        }
        // Keep a pose route ramp alive across several 120 Hz updates. Retargeting
        // starts from the current sample-accurate gain, so even a 100-degree turn
        // traverses the HRTF/VBAP field instead of switching between directions.
        const poseRampSamples = Math.max(1, Math.round(this.ctx.sampleRate * 0.024));
        this.applyGains(immediate, poseRampSamples, undefined, true);
      }
      if (futurePoseMessages.length === 1) this.node?.port.postMessage(futurePoseMessages[0]);
      else if (futurePoseMessages.length > 1) {
        this.node?.port.postMessage({ type: "scheduleGainsBatch", entries: futurePoseMessages });
      }
      // Continue convergence after a smoothed pose update even if the provider
      // sends a lower-rate sample stream; staleness cancels this naturally.
      if (trackingActive) this.schedulePoseGainUpdate(this.lastPoseGainUpdateMs);
    }, wait);
  }

  private syncPoseControl(enabled: boolean): void {
    if (enabled === this.poseControlEnabled) return;
    this.poseControlEnabled = enabled;
    this.node?.port.postMessage({ type: "headTracking", enabled });
  }

  private teardownPostNodes(): void {
    this.outputGraphRevision++;
    for (const splitter of this.binauralBankSplitters.values()) this.node?.disconnect(splitter);
    for (const n of this.postNodes) n.disconnect();
    this.postNodes = [];
    this.convs.clear();
    this.binauralBusNodes = [];
    this.binauralBankSplitters.clear();
    this.binauralBusKeySequence = "";
    this.binauralMerger = null;
    this.binauralLfeInput = null;
    this.headphoneDry = null;
    this.headphoneWet = null;
    this.headphoneInput = null;
    this.headphoneOutput = null;
    this.headphonePreamp = null;
    this.headphoneConvolvers = null;
    this.binauralEqNodes.clear();
    this.binauralLowDiagnosticNodes = null;
    this.binauralEqHeadroom = null;
    this.modeGains.clear();
    this.modeVolumeGains.clear();
    this.modeProgramGains.clear();
    this.multichannelOutput = null;
    this.multichannelProjector = null;
  }

  /** LR4（Linkwitz-Riley 四阶）滤波对：两个 Q=1/√2 的二阶 biquad 级联，
   *  级联后分频点处 -6dB，高低通同相叠加平坦。返回 [入口, 出口]。 */
  private lr4(type: BiquadFilterType, freq: number): [BiquadFilterNode, BiquadFilterNode] {
    const a = this.ctx.createBiquadFilter();
    const b = this.ctx.createBiquadFilter();
    for (const f of [a, b]) {
      f.type = type;
      f.frequency.value = freq;
      f.Q.value = Math.SQRT1_2;
    }
    a.connect(b);
    return [a, b];
  }

  private buildOutputGraph(): void {
    if (!this.node || !this.master) return;
    this.peakGuard?.disconnect();
    this.teardownPostNodes();
    const n = this.topology.length;
    const master = this.master;
    this.updateDestinationChannelCount();

    const createModeOutput = (mode: OutputMode) => {
      const volume = this.ctx.createGain();
      const program = this.ctx.createGain();
      const gain = this.ctx.createGain();
      volume.gain.value = this.volume ** 2;
      program.gain.value = 1;
      gain.gain.value = mode === this.mode ? 1 : 0;
      volume.connect(program);
      program.connect(gain);
      if (mode === "multichannel") {
        const delay = this.ctx.createDelay(BINAURAL_PEAK_GUARD_LOOKAHEAD_S);
        delay.delayTime.value = BINAURAL_PEAK_GUARD_LOOKAHEAD_S;
        gain.connect(delay);
        delay.connect(master);
        this.postNodes.push(delay);
      } else {
        const peakGuard = this.peakGuard;
        if (!peakGuard) throw new Error("SpatialRenderer.init() peak guard missing");
        gain.connect(peakGuard);
      }
      this.modeVolumeGains.set(mode, volume);
      this.modeProgramGains.set(mode, program);
      this.modeGains.set(mode, gain);
      this.postNodes.push(volume, program, gain);
      return volume;
    };

    this.buildMultichannelPath(n, createModeOutput("multichannel"));
    this.buildStereoPath(n, createModeOutput("stereo"));
    this.buildBinauralPath(createModeOutput("binaural"));
    this.peakGuard?.connect(master);
    this.loadHeadphoneCompensation();
  }

  private crossfadeHeadphoneCompensation(wet: boolean): void {
    if (!this.headphoneDry || !this.headphoneWet) return;
    const now = this.ctx.currentTime;
    const duration = 0.05;
    const dryTarget = wet ? 0 : 1;
    const wetTarget = wet ? 1 : 0;
    for (const node of [...this.headphoneDry, ...this.headphoneWet]) {
      const target = this.headphoneDry.includes(node) ? dryTarget : wetTarget;
      node.gain.cancelScheduledValues(now);
      node.gain.setValueAtTime(node.gain.value, now);
      node.gain.linearRampToValueAtTime(target, now + duration);
    }
  }

  private installHeadphoneCompensation(buffers: HeadphoneCompensationBuffers): void {
    const profile = headphoneProfileById(this.headphoneProfileId);
    if (!profile || !this.headphoneInput || !this.headphoneOutput || !this.headphoneDry) return;

    const left = this.ctx.createConvolver();
    const right = this.ctx.createConvolver();
    const preampLeft = this.ctx.createGain();
    const preampRight = this.ctx.createGain();
    const wetLeft = this.ctx.createGain();
    const wetRight = this.ctx.createGain();
    left.normalize = false;
    right.normalize = false;
    left.buffer = buffers.left;
    right.buffer = buffers.right;
    const preamp = Math.pow(10, profile.preampDb / 20);
    preampLeft.gain.value = preamp;
    preampRight.gain.value = preamp;
    wetLeft.gain.value = 0;
    wetRight.gain.value = 0;
    this.headphoneInput.connect(left, 0);
    this.headphoneInput.connect(right, 1);
    left.connect(preampLeft);
    right.connect(preampRight);
    preampLeft.connect(wetLeft);
    preampRight.connect(wetRight);
    wetLeft.connect(this.headphoneOutput, 0, 0);
    wetRight.connect(this.headphoneOutput, 0, 1);

    const retired = [
      ...(this.headphoneWet ?? []),
      ...(this.headphonePreamp ?? []),
      ...(this.headphoneConvolvers ?? []),
    ];
    const oldWet = this.headphoneWet;
    this.headphoneWet = [wetLeft, wetRight];
    this.headphonePreamp = [preampLeft, preampRight];
    this.headphoneConvolvers = [left, right];
    this.postNodes.push(left, right, preampLeft, preampRight, wetLeft, wetRight);

    const now = this.ctx.currentTime;
    for (const node of this.headphoneDry) {
      node.gain.cancelScheduledValues(now);
      node.gain.setValueAtTime(node.gain.value, now);
      node.gain.linearRampToValueAtTime(0, now + 0.05);
    }
    for (const node of oldWet ?? []) {
      node.gain.cancelScheduledValues(now);
      node.gain.setValueAtTime(node.gain.value, now);
      node.gain.linearRampToValueAtTime(0, now + 0.05);
    }
    for (const node of [wetLeft, wetRight]) node.gain.linearRampToValueAtTime(1, now + 0.05);

    this.retirePostNodes(retired, 250);
  }

  private loadHeadphoneCompensation(): void {
    const profile = headphoneProfileById(this.headphoneProfileId);
    if (!profile) return;
    if (this.headphoneBuffers) {
      this.installHeadphoneCompensation(this.headphoneBuffers);
      return;
    }
    const revision = this.outputGraphRevision;
    void getHeadphoneCompensationBuffers(this.ctx, profile)
      .then((buffers) => {
        if (this.headphoneProfileId !== profile.id || revision !== this.outputGraphRevision || this.ctx.state === "closed") return;
        this.headphoneBuffers = buffers;
        this.installHeadphoneCompensation(buffers);
        console.log(`[SDA] 耳机补偿已启用: ${profile.id} (${buffers.left.length}/${buffers.right.length} taps)`);
      })
      .catch((error) => console.warn(`[SDA] 耳机补偿加载失败，保持 bypass: ${profile.id}`, error));
  }

  /** Physical output keeps the worklet's full topology internal, then compacts
   * the selected layout into contiguous WASAPI-mask order. */
  private buildMultichannelPath(_n: number, output: GainNode): void {
    this.multichannelOutput = output;
    this.multichannelProjector = this.createMultichannelProjector(this.layout, 1);
  }

  /** 常驻立体声 downmix，输出只占固定物理通道 0/1。 */
  private buildStereoPath(n: number, output: GainNode): void {
    const merger = this.ctx.createChannelMerger(2);
    BINAURAL_BANKS.forEach((_bank, outputIndex) => {
      const splitter = this.ctx.createChannelSplitter(n);
      this.node!.connect(splitter, outputIndex);
      for (let bus = 0; bus < n; bus++) {
        const spk = this.topology[bus]!;
        // Dense binaural-only fills are not real speakers; they must never feed
        // the stereo downmix.
        if (spk.binauralOnly) continue;
        const gainL = this.ctx.createGain();
        const gainR = this.ctx.createGain();
        const [left, right] = stereoDownmixGains(spk);
        gainL.gain.value = left;
        gainR.gain.value = right;
        splitter.connect(gainL, bus);
        splitter.connect(gainR, bus);
        gainL.connect(merger, 0, 0);
        gainR.connect(merger, 0, 1);
        this.postNodes.push(gainL, gainR);
      }
      this.postNodes.push(splitter);
    });
    merger.connect(output);
    this.postNodes.push(merger);
  }

  /** Fixed worklet buses used by the current logical layout, including LFE. */
  private activeBinauralBuses(): { topologyBus: number; speaker: VirtualSpeaker; layoutBus: number }[] {
    const buses: { topologyBus: number; speaker: VirtualSpeaker; layoutBus: number }[] = [];
    for (let layoutBus = 0; layoutBus < this.renderLayout.length; layoutBus++) {
      const topologyBus = this.renderToTopology[layoutBus] ?? -1;
      if (topologyBus >= 0) buses.push({ topologyBus, speaker: this.renderLayout[layoutBus]!, layoutBus });
    }
    return buses;
  }

  /** Fixed topology bus identities used by the current logical binaural layout. */
  private currentBinauralBusKeySequence(): string {
    return this.activeBinauralBuses()
      .map(({ topologyBus }) => speakerBusKey(this.topology[topologyBus]!))
      .join(",");
  }

  /** Disconnect only the replaceable bus branches. The worklet, source rings, and
   * final binaural processing remain connected, so a layout change cannot reset
   * the codec timeline or leak obsolete ConvolverNodes. */
  private rebuildBinauralBusGraph(force = false): void {
    if (!this.binauralMerger) return;
    const nextBusKeySequence = this.currentBinauralBusKeySequence();
    if (!force && nextBusKeySequence === this.binauralBusKeySequence) return;
    for (const splitter of this.binauralBankSplitters.values()) this.node?.disconnect(splitter);
    for (const node of this.binauralBusNodes) node.disconnect();
    const retired = new Set(this.binauralBusNodes);
    this.postNodes = this.postNodes.filter((node) => !retired.has(node));
    this.binauralBusNodes = [];
    this.binauralBankSplitters.clear();
    this.convs.clear();
    this.binauralBusKeySequence = nextBusKeySequence;
    const activeBanks = new Set<BinauralBank>([this.binauralMode]);
    for (const source of this.sources.values()) activeBanks.add(binauralBank(source.binauralMode, this.binauralMode));
    for (const bank of activeBanks) this.buildBinauralBank(bank);
  }

  /** Current measured spatial convolution and direct-path topology for diagnostics. */
  get binauralHealth(): BinauralHealthTelemetry {
    const banks: BinauralBankHealth[] = [];
    let totalSpatialConvolutions = 0;
    let totalDirectPaths = 0;
    for (const [bank, buses] of this.convs) {
      let spatialConvolutions = 0;
      let directPaths = 0;
      for (const [topologyBus, convolver] of buses) {
        if (convolver) spatialConvolutions++;
        // `off` paths are direct stereo sends. Its LFE remains on the dedicated
        // low-pass/LFE path, so it is neither a spatial convolution nor direct.
        else if (bank === "off" && !this.topology[topologyBus]?.isLfe) directPaths++;
      }
      banks.push({ bank, spatialConvolutions, directPaths });
      totalSpatialConvolutions += spatialConvolutions;
      totalDirectPaths += directPaths;
    }
    return { activeBankCount: banks.length, banks, totalSpatialConvolutions, totalDirectPaths };
  }

  private trackBinauralBusNodes(...nodes: AudioNode[]): void {
    this.binauralBusNodes.push(...nodes);
    this.postNodes.push(...nodes);
  }

  private buildBinauralBank(bank: BinauralBank): void {
    if (!this.node || !this.binauralMerger || this.convs.has(bank)) return;
    const outputIndex = BINAURAL_BANKS.indexOf(bank);
    const splitter = this.ctx.createChannelSplitter(this.topology.length);
    this.node.connect(splitter, outputIndex);
    this.binauralBankSplitters.set(bank, splitter);
    const convs = new Map<number, ConvolverNode | null>();
    const mode: BinauralMode = bank === "far" ? "far" : bank === "mid" ? "mid" : "near";
    // Build measured buffers from logical speaker geometry, then connect them to
    // their fixed worklet bus. This retains calibrated per-layout IR selection.
    // Dense binaural-only fills take their IRs from the dense sphere set (exact
    // 20°/45° grid directions); named bed speakers keep the calibrated set.
    const busIrs = this.irSet && bank !== "off" ? buildBusIrs(this.ctx, this.irSet, this.renderLayout, mode) : null;
    const denseBusIrs = this.denseIrSet && bank !== "off" && this.denseBinauralObjects
      ? buildBusIrs(this.ctx, this.denseIrSet, this.renderLayout, mode)
      : null;
    for (const { topologyBus, speaker, layoutBus } of this.activeBinauralBuses()) {
      if (speaker.isLfe) {
        const lfeGain = this.ctx.createGain();
        const [lpIn, lpOut] = this.lr4("lowpass", LFE_LOWPASS_HZ);
        splitter.connect(lpIn, topologyBus);
        lfeGain.gain.value = BINAURAL_LFE_INBAND_GAIN;
        lpOut.connect(lfeGain);
        if (this.binauralLfeInput) lfeGain.connect(this.binauralLfeInput);
        this.trackBinauralBusNodes(lpIn, lpOut, lfeGain);
        convs.set(topologyBus, null);
        continue;
      }
      if (bank === "off") {
        const direct = this.ctx.createGain();
        direct.gain.value = 0.5;
        splitter.connect(direct, topologyBus);
        direct.connect(this.binauralMerger, 0, 0);
        direct.connect(this.binauralMerger, 0, 1);
        this.trackBinauralBusNodes(direct);
        convs.set(topologyBus, null);
        continue;
      }
      const ir = (speaker.binauralOnly ? denseBusIrs?.get(layoutBus) ?? busIrs?.get(layoutBus) : busIrs?.get(layoutBus));
      if (ir) {
        const conv = this.ctx.createConvolver();
        conv.normalize = false;
        conv.buffer = ir;
        const earSplit = this.ctx.createChannelSplitter(2);
        splitter.connect(conv, topologyBus);
        conv.connect(earSplit);
        earSplit.connect(this.binauralMerger, 0, 0);
        earSplit.connect(this.binauralMerger, 1, 1);
        this.trackBinauralBusNodes(conv, earSplit);
        convs.set(topologyBus, conv);
      } else {
        const panner = this.ctx.createPanner();
        panner.panningModel = "HRTF";
        panner.distanceModel = "linear";
        panner.refDistance = 1;
        panner.maxDistance = 1;
        panner.rolloffFactor = 0;
        const [x, y, z] = sphericalToWebAudio(speaker);
        panner.positionX.value = x;
        panner.positionY.value = y;
        panner.positionZ.value = z;
        const earSplit = this.ctx.createChannelSplitter(2);
        splitter.connect(panner, topologyBus);
        panner.connect(earSplit);
        earSplit.connect(this.binauralMerger, 0, 0);
        earSplit.connect(this.binauralMerger, 1, 1);
        this.trackBinauralBusNodes(panner, earSplit);
        convs.set(topologyBus, null);
      }
    }
    this.convs.set(bank, convs);
    this.trackBinauralBusNodes(splitter);
  }

  /** Per-mode double-ear rendering. The worklet exposes four topology-channel
   * outputs, avoiding the browser's single-node channel limit. */
  private buildBinauralPath(output: GainNode): void {
    const merger = this.ctx.createChannelMerger(2);
    const makeup = this.ctx.createGain();
    makeup.gain.value = BINAURAL_MAKEUP_GAIN;
    const peakGuard = this.peakGuard;
    if (!peakGuard) throw new Error("SpatialRenderer.init() peak guard missing");

    this.binauralMerger = merger;
    this.binauralBusKeySequence = this.currentBinauralBusKeySequence();
    const lfeSum = this.ctx.createGain();
    const lfePeak = this.ctx.createDynamicsCompressor();
    const lfeOut = this.ctx.createGain();
    lfePeak.threshold.value = BINAURAL_LFE_PEAK_THRESHOLD_DB;
    lfePeak.knee.value = BINAURAL_LFE_PEAK_KNEE_DB;
    lfePeak.ratio.value = BINAURAL_LFE_PEAK_RATIO;
    lfePeak.attack.value = BINAURAL_LFE_PEAK_ATTACK_S;
    lfePeak.release.value = BINAURAL_LFE_PEAK_RELEASE_S;
    lfeOut.gain.value = 0.5;
    lfeSum.connect(lfePeak);
    lfePeak.connect(lfeOut);
    lfeOut.connect(merger, 0, 0);
    lfeOut.connect(merger, 0, 1);
    this.binauralLfeInput = lfeSum;
    this.postNodes.push(lfeSum, lfePeak, lfeOut);
    const activeBanks = new Set<BinauralBank>([this.binauralMode]);
    for (const source of this.sources.values()) activeBanks.add(binauralBank(source.binauralMode, this.binauralMode));
    for (const bank of activeBanks) this.buildBinauralBank(bank);
    let finalBinaural: AudioNode = merger;
    const compensationSplit = this.ctx.createChannelSplitter(2);
    const compensationMerge = this.ctx.createChannelMerger(2);
    const dryLeft = this.ctx.createGain();
    const dryRight = this.ctx.createGain();
    dryLeft.gain.value = 1;
    dryRight.gain.value = 1;
    merger.connect(compensationSplit);
    compensationSplit.connect(dryLeft, 0);
    compensationSplit.connect(dryRight, 1);
    dryLeft.connect(compensationMerge, 0, 0);
    dryRight.connect(compensationMerge, 0, 1);
    this.headphoneInput = compensationSplit;
    this.headphoneOutput = compensationMerge;
    this.headphoneDry = [dryLeft, dryRight];
    this.headphoneWet = null;
    this.headphonePreamp = null;
    this.headphoneConvolvers = null;
    this.postNodes.push(compensationSplit, compensationMerge, dryLeft, dryRight);
    finalBinaural = compensationMerge;
    const eqSplit = this.ctx.createChannelSplitter(2);
    const eqMerge = this.ctx.createChannelMerger(2);
    const eqHeadroom = this.ctx.createGain();
    this.binauralEqHeadroom = eqHeadroom;
    this.setEqHeadroom(this.binauralEqBands);
    finalBinaural.connect(eqHeadroom);
    eqHeadroom.connect(eqSplit);
    for (const filter of BINAURAL_EQ_BANDS) {
      const left = this.ctx.createBiquadFilter();
      const right = this.ctx.createBiquadFilter();
      for (const node of [left, right]) {
        node.type = filter.type;
        node.frequency.value = filter.frequency;
        node.Q.value = filter.q;
        node.gain.value = this.binauralEqBands[filter.band];
      }
      this.binauralEqNodes.set(filter.band, [left, right]);
      this.postNodes.push(left, right);
    }
    const low = this.binauralEqNodes.get("low")!;
    const mid = this.binauralEqNodes.get("mid")!;
    const high = this.binauralEqNodes.get("high")!;
    eqSplit.connect(low[0], 0);
    low[0].connect(mid[0]);
    mid[0].connect(high[0]);
    high[0].connect(eqMerge, 0, 0);
    eqSplit.connect(low[1], 1);
    low[1].connect(mid[1]);
    mid[1].connect(high[1]);
    high[1].connect(eqMerge, 0, 1);
    this.postNodes.push(eqHeadroom, eqSplit, eqMerge);
    const diagnosticSplit = this.ctx.createChannelSplitter(2);
    const diagnosticMerge = this.ctx.createChannelMerger(2);
    const diagnosticLeft = this.ctx.createBiquadFilter();
    const diagnosticRight = this.ctx.createBiquadFilter();
    const diagnosticGain = this.binauralLowFrequencyDiagnosticMode === "low-cut"
      ? BINAURAL_LOW_DIAGNOSTIC_FILTER.gain
      : 0;
    for (const node of [diagnosticLeft, diagnosticRight]) {
      node.type = BINAURAL_LOW_DIAGNOSTIC_FILTER.type;
      node.frequency.value = BINAURAL_LOW_DIAGNOSTIC_FILTER.frequency;
      node.Q.value = BINAURAL_LOW_DIAGNOSTIC_FILTER.q;
      node.gain.value = diagnosticGain;
    }
    this.binauralLowDiagnosticNodes = [diagnosticLeft, diagnosticRight];
    eqMerge.connect(diagnosticSplit);
    diagnosticSplit.connect(diagnosticLeft, 0);
    diagnosticLeft.connect(diagnosticMerge, 0, 0);
    diagnosticSplit.connect(diagnosticRight, 1);
    diagnosticRight.connect(diagnosticMerge, 0, 1);
    this.postNodes.push(eqHeadroom, eqSplit, eqMerge, diagnosticSplit, diagnosticLeft, diagnosticRight, diagnosticMerge);
    diagnosticMerge.connect(makeup);
    makeup.connect(output);
    this.postNodes.push(merger, makeup);
  }

  rebindBedSource(id: string, bedLabel: string, atSample: number): void {
    const state = this.sources.get(id);
    if (!state) {
      this.addSource(id, { bedLabel, atSample });
      return;
    }
    const normalized = aliasLabel(bedLabel);
    if (state.bedLabel === normalized) return;
    state.bedLabel = normalized;
    state.isLfe = isLfeLabel(bedLabel);
    state.position = positionForLabel(bedLabel);
    state.snapBus = state.isLfe ? -1 : this.renderLayout.findIndex((speaker) => speaker.name === normalized);
    this.recomputeBedGainsAt(Math.trunc(atSample), 32);
  }

  /** Register a source. Bed channels pass their speaker label; objects an event id.
   *  重复声明同一 id（稀疏声明变化时 player 会重放整组）完全幂等：保留
   *  SourceState/元数据/静音状态，也不向 worklet 重发即时 gains。 */
  addSource(id: string, opts: { bedLabel?: string; atSample?: number } = {}): void {
    if (this.sources.has(id)) {
      if (this.retiringSources.delete(id)) {
        const at = Number.isSafeInteger(opts.atSample) ? Math.trunc(opts.atSample!) : this.consumedSamples;
        const state = this.sources.get(id)!;
        this.scheduleSourceLifecycle(state, at, true);
        this.node?.port.postMessage({ type: "add", id });
        this.node?.port.postMessage({
          type: "binauralMode",
          id,
          bank: BINAURAL_BANKS.indexOf(binauralBank(state.binauralMode, this.binauralMode)),
        });
        if (state.muted) this.node?.port.postMessage({ type: "mute", id, muted: true, ramp: 32 });
        this.node?.port.postMessage({ type: "resumeAt", id, at });
        if (state.bedLabel) this.recomputeBedGainsAt(at, 32);
        else this.applyGains(state, 32, at);
      }
      return;
    }
    if (!this.node) throw new Error("SpatialRenderer.init() first");
    const state: SourceState = {
      id,
      spread: 0,
      position: { azimuth: 0, elevation: 0, distance: 1 },
      gainDb: 0,
      hasObjectMetadata: false,
      objectRampEndSample: Number.NEGATIVE_INFINITY,
      isLfe: opts.bedLabel ? isLfeLabel(opts.bedLabel) : false,
      muted: false,
      bedLabel: opts.bedLabel ? aliasLabel(opts.bedLabel) : undefined,
      snapBus: -1,
      binauralMode: undefined,
      lifecycleEvents: [],
      lifecycleEventOrder: 0,
      objectPoseTimeline: [],
    };
    if (opts.bedLabel) {
      state.position = positionForLabel(opts.bedLabel);
      // 床声道吸附：标签归一化后命中布局音箱 → 直送该音箱总线（物理直出语义），
      // 不再用 VBAP 摊到相邻音箱；布局里没有这个音箱才回退 VBAP 平移。
      if (!state.isLfe) {
        state.snapBus = this.renderLayout.findIndex((s) => s.name === state.bedLabel);
      }
    }
    this.sources.set(id, state);
    this.node.port.postMessage({ type: "add", id });
    this.node.port.postMessage({ type: "binauralMode", id, bank: BINAURAL_BANKS.indexOf(binauralBank(state.binauralMode, this.binauralMode)) });
    const atSample = Number.isSafeInteger(opts.atSample) ? Math.trunc(opts.atSample!) : undefined;
    if (state.bedLabel && atSample !== undefined) {
      this.recomputeBedGainsAt(atSample, 32);
    } else {
      this.applyGains(state, 0);
      if (state.snapBus >= 0) this.recomputeBedGains(id);
    }
  }

  private scheduleSourceLifecycle(state: SourceState, at: number, active: boolean): void {
    state.lifecycleEvents.push({ at, active, order: state.lifecycleEventOrder++ });
    state.lifecycleEvents.sort((left, right) => left.at - right.at || left.order - right.order);
  }

  private sourceActiveAt(state: SourceState, samplePos: number): boolean {
    let active = true;
    for (const event of state.lifecycleEvents) {
      if (event.at > samplePos) break;
      active = event.active;
    }
    return active;
  }

  /** 床声道集合变化（新床声道占用/释放了扩展目标总线）→ 重推其余床声道的增益，
   *  让上混馈送跳过/恢复被真实声道占用的总线。 */
  private recomputeBedGains(excludeId: string): void {
    for (const s of this.sources.values()) {
      if (s.id !== excludeId && s.snapBus >= 0) this.applyGains(s, 512);
    }
  }

  private recomputeBedGainsAt(atSample: number, rampSamples: number): void {
    for (const state of this.sources.values()) {
      if (state.bedLabel && this.sourceActiveAt(state, atSample)) {
        this.applyGains(state, rampSamples, atSample);
      }
    }
  }

  /** 其余床声道在指定 sample 占用的总线（扩展馈送要避开）。 */
  private bedOccupiedBuses(excludeId: string, atSample?: number): Set<number> {
    const occ = new Set<number>();
    for (const state of this.sources.values()) {
      const active = atSample === undefined || this.sourceActiveAt(state, atSample);
      if (state.id !== excludeId && state.snapBus >= 0 && active) occ.add(state.snapBus);
    }
    return occ;
  }

  /** 静音/取消静音一个源（Omniphony 式对象 mute/solo 的底层原语）。
   *  走 2048 采样斜坡（@48k ≈ 43ms），切换无爆音。
   *  返回 false = 源不存在（调用方可据此提示 id 不匹配）。 */
  setSourceMuted(id: string, muted: boolean): boolean {
    const state = this.sources.get(id);
    if (!state) {
      console.warn(`[SDA] setSourceMuted 无源 "${id}"，现有源: ${[...this.sources.keys()].join(", ") || "(空)"}`);
      return false;
    }
    if (state.muted === muted) return true;
    state.muted = muted;
    this.node?.port.postMessage({ type: "mute", id, muted, ramp: 2048 });
    console.log(`[SDA] ${id} ${muted ? "静音" : "解除静音"} → scalar ${muted ? 0 : 1}`);
    return true;
  }

  /** 静音/恢复所有独立 LFE 床声道；状态会应用到迟到注册的 LFE 源。 */
  setLfeMuted(muted: boolean): void {
    this.lfeMuted = muted;
    for (const state of this.sources.values()) {
      if (state.isLfe) this.applyGains(state, 2048);
    }
  }

  retireSourceAt(id: string, samplePos: number): void {
    const state = this.sources.get(id);
    if (!state) return;
    const at = Math.trunc(samplePos);
    const token = this.nextRetirementToken++;
    this.scheduleSourceLifecycle(state, at, false);
    this.retiringSources.set(id, token);
    this.node?.port.postMessage({ type: "removeAt", id, at, token });
    if (state.bedLabel) this.recomputeBedGainsAt(at, 32);
  }

  removeSource(id: string): void {
    const state = this.sources.get(id);
    this.sources.delete(id);
    this.retiringSources.delete(id);
    this.node?.port.postMessage({ type: "remove", id });
    if (state && state.snapBus >= 0) this.recomputeBedGains(id);
  }

  /** Largest safe prebuffer time, leaving one codec frame of ring headroom. */
  maxBufferedSeconds(): number {
    return Math.max(0.25, (PCM_RING_SAMPLES - 8192) / this.ctx.sampleRate);
  }

  startAt(samplePos: number): void {
    const origin = Math.trunc(samplePos);
    // Keep the main-thread cursor coherent with the worklet immediately after a
    // renderer recreation. Otherwise it remains at zero until the first tick,
    // briefly making a nonzero codec-timeline restart appear to jump backwards.
    this.consumedSamples = origin;
    this.node?.port.postMessage({ type: "start", origin });
    this.peakGuard?.port.postMessage({ type: "start", origin });
  }

  /** Feed PCM for a source (legacy single-source path). */
  feed(id: string, samples: Float32Array): void {
    this.node?.port.postMessage({ type: "feed", id, samples }, [samples.buffer]);
  }

  /** Atomically enqueue every channel of one decoded frame at its absolute
   * codec sample position. Partial frame writes are rejected by the worklet. */
  feedBatch(
    sequence: number,
    samplePos: number,
    entries: readonly { id: string; samples: Float32Array }[],
  ): void {
    if (!this.node || entries.length === 0) {
      this.onBatchResult?.({ sequence, accepted: false, samples: 0, reason: "invalid" });
      return;
    }
    this.node.port.postMessage({ type: "feedBatch", sequence, start: Math.trunc(samplePos), entries });
  }

  /** Queue object events on the same absolute sample clock as their PCM. Exact
   * repeated targets are discarded before VBAP and MessagePort allocation. */
  applyEvents(events: readonly ObjectEvent[]): number {
    if (!this.node || events.length === 0) return 0;
    const messages: ScheduledGainMessage[] = [];
    const pending: { state: SourceState; ramp: number; at: number }[] = [];
    let accepted = 0;
    for (const ev of events) {
      const state = this.sources.get(`obj:${ev.id}`);
      if (!state) continue;
      const nextPosition = ev.hasPos ? admToSpherical(ev.pos) : state.position;
      const nextSpread = ev.hasPos ? sizeToSpread(ev.size) : state.spread;
      const ramp = ev.rampDuration || 128;
      const at = Math.trunc(ev.samplePos);
      const unchanged = state.hasObjectMetadata
        && state.objectRampEndSample <= at
        && state.position.azimuth === nextPosition.azimuth
        && state.position.elevation === nextPosition.elevation
        && state.position.distance === nextPosition.distance
        && state.spread === nextSpread
        && state.gainDb === ev.gainDb;
      if (unchanged) continue;
      const previousPose = state.objectPoseTimeline.at(-1);
      const previousProgress = previousPose
        ? Math.min(1, Math.max(0, (at - previousPose.at) / previousPose.rampSamples))
        : 1;
      const fromPosition = previousPose
        ? interpolateObjectPosition(previousPose.fromPosition, previousPose.position, previousProgress)
        : state.position;
      const fromSpread = previousPose
        ? previousPose.fromSpread + (previousPose.spread - previousPose.fromSpread) * previousProgress
        : state.spread;
      state.position = nextPosition;
      state.spread = nextSpread;
      state.gainDb = ev.gainDb;
      state.hasObjectMetadata = true;
      state.objectRampEndSample = at + Math.max(1, ramp);
      state.objectPoseTimeline.push({
        at,
        fromPosition,
        position: nextPosition,
        fromSpread,
        spread: nextSpread,
        gainDb: ev.gainDb,
        rampSamples: Math.max(1, ramp),
      });
      pending.push({ state, ramp, at });
      accepted++;
    }
    // Prototype-level renderer tests and narrow control surfaces may provide only
    // the legacy gainMessage surface; in that case keep the existing JS path.
    const batchGains = typeof this.panObjectBatch === "function"
      ? this.panObjectBatch(pending.map(({ state }) => state))
      : null;
    pending.forEach(({ state, ramp, at }, index) => {
      messages.push(this.gainMessage(state, ramp, at, false, batchGains?.[index]));
      // Prebuffer a head-relative route at the same codec boundary. Later pose
      // ticks replace it in-place; this first copy covers a main-thread stall
      // immediately after the decoder queues the frame. Head-relative routes
      // remain on the direct JS solver because their pose is wall-clock live.
      if (this.mode === "binaural" && this.headPose.isActive(performance.now())) {
        messages.push(this.gainMessage(state, ramp, at, true));
      }
    });
    if (messages.length === 1) this.node.port.postMessage(messages[0]);
    else if (messages.length > 1) this.node.port.postMessage({ type: "scheduleGainsBatch", entries: messages });
    return accepted;
  }

  /** Queue one object event. Kept for control surfaces and focused tests. */
  applyEvent(ev: ObjectEvent, rampSamples: number): boolean {
    const event = rampSamples === ev.rampDuration ? ev : { ...ev, rampDuration: rampSamples };
    return this.applyEvents([event]) > 0;
  }

  private gainMessage(
    state: SourceState,
    rampSamples: number,
    atSample?: number,
    poseUpdate = false,
    precomputedSpatialGains?: Float32Array,
  ): ScheduledGainMessage {
    // Codec metadata remains canonical world-space. Only immediate gain updates
    // use the live wall-clock pose; scheduled events must retain their original
    // codec-clock semantics and are followed by subsequent pose refreshes.
    const poseNow = performance.now();
    // Physical speaker and plain stereo outputs stay room-locked. The UI exposes
    // tracking only for binaural playback, and this guard preserves that rule for
    // programmatic callers as well.
    const headTrackingActive = this.mode === "binaural"
      && (atSample === undefined || poseUpdate)
      && this.headPose.isActive(poseNow);
    const spatialPosition = headTrackingActive
      ? this.headPose.headRelative(state.position, poseNow)
      : state.position;
    // A codec-clock event may use the same-layout Rust batch result. Any live
    // head-relative update keeps the direct JS calculation to avoid pose lag.
    const gains = precomputedSpatialGains && !headTrackingActive
      ? new Float32Array(precomputedSpatialGains)
      : this.vbap.pan(spatialPosition, state.spread);

    // ADM 半径是对象定位的归一化坐标：1 = 虚拟音箱环。渲染器只在环外
    // 按 Apple inverse 距离定律衰减；不从没有明确物理米制语义的 ADM 半径
    // 推导空气吸收，避免把正常的沉浸声对象错误低通得发闷。
    const normalizedDistance = Math.max(1e-3, state.position.distance);
    let distGain = 1;
    let lp = 1; // 保持内容高频；空气吸收需明确的物理距离元数据才可启用。
    if (normalizedDistance > 1) {
      distGain = 1 / normalizedDistance;
    }

    // The codec event model uses -128 as the i8 representation of -∞ dB.
    // Preserve exact silence instead of turning it into a tiny residual gain.
    const metadataGain = state.gainDb <= -128
      ? 0
      : Math.pow(10, state.gainDb / 20);
    let scalar = metadataGain * distGain;
    if (state.isLfe) {
      // LFE bypasses spatial panning: straight to the LFE bus.
      gains.fill(0);
      const lfeBus = this.renderLayout.findIndex((s) => s.isLfe);
      if (lfeBus >= 0) gains[lfeBus] = 1;
      scalar = metadataGain;
      if (this.lfeMuted) scalar = 0;
      lp = 1;
    } else if (state.snapBus >= 0 && !headTrackingActive) {
      // 床声道吸附：直送同名音箱总线（AVR direct 语义）。
      // 上混扩展馈送仅用于多声道物理输出 —— 物理后环在真实房间里被房间
      // 反射去相关，听着是"填满"；而双耳/立体声里馈送是相干拷贝
      // （BRIR(100°) + 0.5·BRIR(140°) 在鼓膜处同相叠加），梳状滤波 + 声像
      // 向中间涂抹，整个声场挤成一团。AVR 上混器对派生声道做去相关，
      // 虚拟音箱域没有这个环节 —— 也不需要有：吸附已把床放到混音师
      // 本来的位置。扩展目标总线被真实床声道占用时跳过（7.1 内容的
      // 后环不吃 5.1 式馈送）。
      gains.fill(0);
      gains[state.snapBus] = 1;
      if (this.mode === "multichannel") {
        const occupied = this.bedOccupiedBuses(state.id, atSample);
        for (const e of this.expansion.get(state.snapBus) ?? []) {
          if (!occupied.has(e.bus)) gains[e.bus] = e.gain;
        }
      }
    }
    // User mute is a separate real-time worklet envelope. Keeping it out of
    // metadata gain ramps prevents future scheduled object events from
    // accidentally clearing mute/solo state.
    // 工作节点固定为所有标准位置的并集；把当前逻辑布局的增益按稳定总线键投影。
    // 逻辑布局不存在的总线保持 0，切换布局仅更新这组斜坡，不触碰 PCM 缓冲。
    const topologyGains = new Float32Array(this.topology.length);
    for (let bus = 0; bus < gains.length; bus++) {
      const target = this.renderToTopology[bus] ?? -1;
      if (target >= 0) topologyGains[target] = gains[bus]!;
    }
    return {
      type: atSample === undefined ? "gains" : "scheduleGains",
      id: state.id,
      at: atSample,
      gains: topologyGains,
      gain: scalar,
      lp,
      ramp: Math.max(1, rampSamples),
      poseControlled: !state.bedLabel && !state.isLfe,
      poseUpdate,
    };
  }

  /** Recompute and send a source's gain vector over the buses. */
  private applyGains(
    state: SourceState,
    rampSamples: number,
    atSample?: number,
    poseUpdate = false,
  ): void {
    this.node?.port.postMessage(this.gainMessage(state, rampSamples, atSample, poseUpdate));
  }

  /** Reset the codec timeline. MessagePort FIFO guarantees a following feed is
   * handled after reset; the epoch only rejects already-queued stale ticks. */
  resetBuffers(): void {
    this.epoch++;
    this.consumedSamples = 0;
    for (const id of this.retiringSources.keys()) {
      this.sources.delete(id);
      this.node?.port.postMessage({ type: "remove", id });
    }
    this.retiringSources.clear();
    for (const state of this.sources.values()) {
      state.lifecycleEvents.length = 0;
      state.lifecycleEventOrder = 0;
      if (!state.bedLabel) {
        state.hasObjectMetadata = false;
        state.objectRampEndSample = Number.NEGATIVE_INFINITY;
        state.objectPoseTimeline.length = 0;
      }
    }
    this.node?.port.postMessage({ type: "reset", epoch: this.epoch });
    this.peakGuard?.port.postMessage({ type: "reset" });
  }

  /** Playhead in seconds: frames the worklet actually rendered. */
  consumedSeconds(): number {
    return this.consumedSamples / this.ctx.sampleRate;
  }

  /** Worklet-level pause: outputs silence without consuming the ring buffers,
   *  so resume continues from the exact sample. */
  setPaused(paused: boolean): void {
    this.node?.port.postMessage({ type: "pause", paused });
    this.peakGuard?.port.postMessage({ type: "pause", paused });
  }

  setVolumeBalance(enabled: boolean): void {
    this.volumeBalanceEnabled = enabled;
    this.peakGuard?.port.postMessage({ type: "programEnabled", enabled });
  }

  setProgramLoudnessGainDb(gainDb: number | null, atSample?: number): void {
    this.programLoudnessGainDb = gainDb === null || !Number.isFinite(gainDb) ? null : Math.min(0, gainDb);
    const gain = this.programLoudnessGainDb === null ? 1 : Math.pow(10, this.programLoudnessGainDb / 20);
    this.peakGuard?.port.postMessage({
      type: atSample === undefined ? "programGain" : "scheduleProgramGain",
      gain,
      at: atSample === undefined ? undefined : Math.trunc(atSample),
    });
  }

  /** Master output volume, 0..1 (applied perceptually: gain = v²). */
  setVolume(v: number): void {
    this.volume = Math.max(0, Math.min(1, v));
    const now = this.ctx.currentTime;
    for (const node of this.modeVolumeGains.values()) {
      node.gain.cancelScheduledValues(now);
      node.gain.setValueAtTime(node.gain.value, now);
      node.gain.linearRampToValueAtTime(this.volume ** 2, now + 0.02);
    }
  }

  async close(): Promise<void> {
    if (this.poseUpdateTimer !== null) {
      globalThis.clearTimeout(this.poseUpdateTimer);
      this.poseUpdateTimer = null;
    }
    if (this.poseStaleTimer !== null) {
      globalThis.clearTimeout(this.poseStaleTimer);
      this.poseStaleTimer = null;
    }
    this.wasmVbapLayoutRevision++;
    this.wasmVbap?.free();
    this.wasmVbap = null;
    this.teardownPostNodes();
    this.peakGuard?.disconnect();
    this.peakGuard = null;
    this.node?.disconnect();
    this.master?.disconnect();
    if (this.ctx.state !== "closed") await this.ctx.close();
  }
}
