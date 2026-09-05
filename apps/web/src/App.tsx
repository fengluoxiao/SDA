import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { SdaPlayer, type BinauralRenderMetadata, type NativeRendererSink, type NativeRendererSourceDeclaration, type PlayerHealthSnapshot, type ProgramLoudnessMetadata, type VisualObject } from "@sda/player";
import {
  availableHeadphoneCompensationProfiles,
  registerLocalHeadphoneCompensation,
  setBinauralAssetLoader,
  setHeadphoneCompensationAssetLoader,
  type HeadPose,
  type LocalHeadphoneCompensationData,
  LAYOUTS,
  detectLayoutId,
  type LayoutId,
  type OutputMode,
  type BinauralEqBands,
  type BinauralLowFrequencyDiagnostic,
} from "@sda/renderer";
// @ts-ignore — plain JS asset served by Vite
import workletUrl from "@sda/renderer/worklet/sda-renderer.worklet.js?url";
import { ObjectView, type Theme } from "./components/ObjectView";
import { MiniPlayer, type TrackInfo } from "./components/MiniPlayer";
import { ObjectPanel } from "./components/ObjectPanel";
import { speakerLabel, speakerPosition, WIDE_SPEAKERS } from "./speaker-labels";
import {
  quaternionAngularVelocity,
  quaternionEulerAngles,
  type HeadTrackingTelemetrySample,
  type Quaternion,
} from "./head-tracking-telemetry";
import { HeadTrackingSession } from "./head-tracking-session";

type PlaybackSource = { kind: "file"; file: File } | { kind: "path"; path: string };
type HeadTrackingPlayer = {
  setHeadPose?: (pose: HeadPose) => void;
  clearHeadPose?: () => void;
};

function rendererHeadPose(pose: HeadTrackingPose): HeadPose {
  // Electron receives epoch timestamps from the local provider, whereas the
  // renderer filters against performance.now(). Timestamp at bridge receipt so
  // both sides use one monotonic clock; transport jitter is handled by smoothing.
  return {
    timestampMs: performance.now(),
    orientation: [pose.orientation.x, pose.orientation.y, pose.orientation.z, pose.orientation.w],
  };
}

interface PlaylistItem {
  id: string;
  source: PlaybackSource;
  title: string;
  identity: string;
}

// Keep each Electron worker decode turn short for object-heavy JOC streams.
// push() now waits for a worker ACK, so this also bounds renderer message bursts.
const FILE_CHUNK_SIZE = 1 << 18;
const OUTPUT_LATENCY_STORAGE_KEY = "sda-output-latency-seconds";
const BINAURAL_LOW_FREQUENCY_DIAGNOSTIC_STORAGE_KEY = "sda-binaural-low-frequency-diagnostic";
type OutputLatencySeconds = 0.1 | 0.2 | 0.3;
const DEFAULT_OUTPUT_LATENCY_SECONDS: OutputLatencySeconds = 0.1;
const HEAD_TRACKING_TELEMETRY_INTERVAL_MS = 1000 / 30;
const HEAD_TRACKING_TELEMETRY_HISTORY_MS = 6_000;
const assetUrl = (path: string): string => new URL(path, document.baseURI).toString();
const ownedArrayBuffer = (bytes: Uint8Array): ArrayBuffer => Uint8Array.from(bytes).buffer;

function readOutputLatencySeconds(): OutputLatencySeconds {
  const desktopValue = window.sdaDesktop?.getOutputLatencySeconds?.();
  if (desktopValue === 0.1 || desktopValue === 0.2 || desktopValue === 0.3) return desktopValue;
  try {
    const value = Number(localStorage.getItem(OUTPUT_LATENCY_STORAGE_KEY));
    return value === 0.1 || value === 0.2 || value === 0.3 ? value : DEFAULT_OUTPUT_LATENCY_SECONDS;
  } catch {
    return DEFAULT_OUTPUT_LATENCY_SECONDS;
  }
}

function persistOutputLatencySeconds(seconds: OutputLatencySeconds): void {
  const persistDesktopValue = window.sdaDesktop?.setOutputLatencySeconds;
  if (persistDesktopValue) {
    persistDesktopValue(seconds);
    return;
  }
  try {
    localStorage.setItem(OUTPUT_LATENCY_STORAGE_KEY, String(seconds));
  } catch {
    // Private-mode or quota failures must not interfere with playback.
  }
}

function readVolumeBalanceEnabled(): boolean {
  const desktopValue = window.sdaDesktop?.getVolumeBalanceEnabled?.();
  if (typeof desktopValue === "boolean") return desktopValue;
  try {
    return localStorage.getItem("sda-volume-balance-enabled") === "true";
  } catch {
    return false;
  }
}

function persistVolumeBalanceEnabled(enabled: boolean): void {
  const persistDesktopValue = window.sdaDesktop?.setVolumeBalanceEnabled;
  if (persistDesktopValue) {
    persistDesktopValue(enabled);
    return;
  }
  try {
    localStorage.setItem("sda-volume-balance-enabled", String(enabled));
  } catch {
    // Private-mode or quota failures must not interfere with playback.
  }
}

function readBinauralLowFrequencyDiagnostic(): BinauralLowFrequencyDiagnostic {
  try {
    return localStorage.getItem(BINAURAL_LOW_FREQUENCY_DIAGNOSTIC_STORAGE_KEY) === "low-cut"
      ? "low-cut"
      : "reference";
  } catch {
    return "reference";
  }
}

function persistBinauralLowFrequencyDiagnostic(mode: BinauralLowFrequencyDiagnostic): void {
  try {
    localStorage.setItem(BINAURAL_LOW_FREQUENCY_DIAGNOSTIC_STORAGE_KEY, mode);
  } catch {
    // Persistence failures must not prevent changing the active output graph.
  }
}

/** 完整 HRTF 测量档案。每个选择只使用一个人头/受试者的完整 HRIR/BRIR 系统；
 * 不把 KU100 与 D2/Hx 的头部、耳道或房间响应做频段拼接。 */
type BinauralHead = string;
const BINAURAL_HEAD_STORAGE_KEY = "sda-binaural-head";
/** 完整 HRTF 档案 id（KU100、D2 或 H3–H20 受试者）。 */
const BINAURAL_HEAD_IDS = ["ku100", "d2", ...Array.from({ length: 18 }, (_, i) => `h${i + 3}`)] as const;
function readBinauralHead(): BinauralHead {
  try {
    const v = localStorage.getItem(BINAURAL_HEAD_STORAGE_KEY);
    return v && (BINAURAL_HEAD_IDS as readonly string[]).includes(v) ? v : "ku100";
  } catch {
    return "ku100";
  }
}
/** 每个完整测量 subject 直接选择自己的 HRTF 集；禁止加载旧的 KU100+耳廓 hybrid。 */
function binauralHeadBaseUrl(head: BinauralHead): string {
  return assetUrl(head === "ku100" ? "hrtf" : `hrtf-${head}`);
}
function nativeHrtfSetName(head: BinauralHead, dense = readDenseBinauralObjects()): string {
  return head === "ku100" ? (dense ? "hrtf-dense" : "hrtf") : `hrtf-${head}`;
}

/** 逐对象精确方向渲染（实验性）：对象按精确方位 VBAP 到密集球面，而不是吸附到
 *  床层扬声器环。密集 IR 集固定为 KU100（D1 校准数据，61 个测量方向）。 */
const DENSE_BINAURAL_STORAGE_KEY = "sda-dense-binaural-objects";
function readDenseBinauralObjects(): boolean {
  try {
    return localStorage.getItem(DENSE_BINAURAL_STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}
function denseBinauralBaseUrl(): string {
  return assetUrl("hrtf-dense");
}

function telemetryPolyline(  samples: readonly HeadTrackingTelemetrySample[],
  axis: "x" | "y" | "z",
  scale: number,
): string {
  if (samples.length === 0) return "";
  const end = samples[samples.length - 1]!.timestampMs;
  const start = end - HEAD_TRACKING_TELEMETRY_HISTORY_MS;
  return samples.map((sample) => {
    const x = Math.max(0, Math.min(288, ((sample.timestampMs - start) / HEAD_TRACKING_TELEMETRY_HISTORY_MS) * 288));
    const y = 58 - Math.max(-scale, Math.min(scale, sample[axis])) / scale * 50;
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  }).join(" ");
}

function HeadTrackingTelemetryPanel({ samples }: { samples: readonly HeadTrackingTelemetrySample[] }) {
  const latest = samples[samples.length - 1];
  const scale = Math.max(90, Math.min(1080, Math.ceil(Math.max(
    ...samples.flatMap((sample) => [Math.abs(sample.x), Math.abs(sample.y), Math.abs(sample.z)]),
    0,
  ) / 30) * 30));

  return (
    <div className="panel float-panel head-tracking-panel" aria-label="头部追踪实时数据">
      <div className="telemetry-heading">
        <h2>头部追踪</h2>
        <span className={latest ? "telemetry-live" : ""}>{latest ? "实时" : "等待数据"}</span>
      </div>
      <p className="telemetry-note">角速度由连续姿态差分计算，并非 AirPods 未公开的原始 IMU 字段。</p>
      <div className="telemetry-values" aria-live="off">
        {(["x", "y", "z"] as const).map((axis) => (
          <div key={axis} className={`telemetry-value axis-${axis}`}>
            <span>{axis.toUpperCase()}</span>
            <strong>{latest ? latest[axis].toFixed(1) : "--"}</strong>
            <small>°/s</small>
          </div>
        ))}
      </div>
      <svg className="telemetry-chart" viewBox="0 0 288 116" role="img" aria-label="最近六秒三轴角速度曲线">
        <line x1="0" y1="58" x2="288" y2="58" className="telemetry-zero" />
        <line x1="72" y1="0" x2="72" y2="116" className="telemetry-grid" />
        <line x1="144" y1="0" x2="144" y2="116" className="telemetry-grid" />
        <line x1="216" y1="0" x2="216" y2="116" className="telemetry-grid" />
        {(["x", "y", "z"] as const).map((axis) => (
          <polyline key={axis} points={telemetryPolyline(samples, axis, scale)} className={`telemetry-line axis-${axis}`} />
        ))}
      </svg>
      <div className="telemetry-scale"><span>±{scale} °/s</span><span>最近 6 秒</span></div>
      <dl className="telemetry-orientation">
        <dt>Yaw</dt><dd>{latest ? `${latest.yaw.toFixed(1)}°` : "--"}</dd>
        <dt>Pitch</dt><dd>{latest ? `${latest.pitch.toFixed(1)}°` : "--"}</dd>
        <dt>Roll</dt><dd>{latest ? `${latest.roll.toFixed(1)}°` : "--"}</dd>
      </dl>
    </div>
  );
}

/** Cache key for a track's measured BS.1770-4 loudness (volume balance for
 *  metadata-less content such as ALAC). */
function measuredLoudnessStorageKey(info: { title?: string; channels: number; sampleRate: number }): string {
  return `sda-measured-lufs:${info.title ?? "track"}:${info.channels}:${info.sampleRate}`;
}

/** 完整单一测量系统：每项的头部、耳道、耳廓与 BRIR 来自同一 subject。 */
const BINAURAL_HEADS: ReadonlyArray<{ id: BinauralHead; label: string; description: string }> = [
  { id: "ku100", label: "KU100", description: "Neumann KU100 完整校准 HRTF/BRIR（默认）" },
  { id: "d2", label: "D2 假人头", description: "SADIE II D2 完整单一测量 HRTF/BRIR" },
  ...Array.from({ length: 18 }, (_, i) => ({
    id: `h${i + 3}` as BinauralHead,
    label: `H${i + 3} 受试者`,
    description: "SADIE II 真人受试者完整单一测量 HRTF/BRIR",
  })),
];

try {
  localStorage.removeItem("sda-layout-level-compensation-enabled");
} catch {
  // Private-mode failures must not prevent app startup.
}

const bundledHrtfReader = window.sdaDesktop?.readBundledHrtf;
setBinauralAssetLoader(bundledHrtfReader
  ? async (assetPath) => {
      const bytes = await bundledHrtfReader(assetPath);
      return ownedArrayBuffer(bytes);
    }
  : null);

const bundledFirReader = window.sdaDesktop?.readBundledHeadphoneFir;
setHeadphoneCompensationAssetLoader(bundledFirReader
  ? async (assetPath) => {
      const bytes = await bundledFirReader(assetPath);
      return ownedArrayBuffer(bytes);
    }
  : null);

export function App() {
  const playerRef = useRef<SdaPlayer | null>(null);
  /** Retains the outgoing decoder/player until the replacement native session is ready. */
  const retiringPlayerRef = useRef<SdaPlayer | null>(null);
  const [playerReady, setPlayerReady] = useState<SdaPlayer | null>(null);
  const [mode, setMode] = useState<OutputMode>("binaural");
  /** "auto" = 按码流内容自动检测（床标签 + 是否有动态对象）。 */
  const [layoutId, setLayoutId] = useState<LayoutId | "auto">("auto");
  /** Latest requested layout, including a selection made while sidecar/HRTF
   * setup is still awaiting. The newly created player reconciles this before
   * its decoder is allowed to submit PCM. */
  const layoutIdRef = useRef<LayoutId | "auto">("auto");
  /** 自动模式下首帧检测出的布局（用于界面回显 + 3D 视图）。 */
  const [detectedLayout, setDetectedLayout] = useState<LayoutId | null>(null);
  const [theme, setTheme] = useState<Theme>("dark");
  const [track, setTrack] = useState<TrackInfo | null>(null);
  const [binauralMetadata, setBinauralMetadata] = useState<BinauralRenderMetadata | null>(null);
  const [objects, setObjects] = useState<VisualObject[]>([]);
  const [soundingObjectIds, setSoundingObjectIds] = useState<ReadonlySet<number>>(new Set());
  const [diagnosticObjects, setDiagnosticObjects] = useState<VisualObject[]>([]);
  const lastDiagnosticUpdateRef = useRef(0);
  const lastVisualUiUpdateRef = useRef(0);
  const lastHealthUiUpdateRef = useRef(0);
  const lastHealthEscalationRef = useRef<PlayerHealthSnapshot["callbackGapEscalation"]>("none");
  /** 被静音的对象 id（Omniphony Studio 语义：mute 独立切换；
   *  solo = mute 其他全部对象，独奏态由"只剩一个未静音"导出）。 */
  const [mutedIds, setMutedIds] = useState<ReadonlySet<number>>(new Set());
  const [soloIds, setSoloIds] = useState<ReadonlySet<number>>(new Set());
  const [mutedSpeakerNames, setMutedSpeakerNames] = useState<ReadonlySet<string>>(new Set());
  const [soloSpeakerNames, setSoloSpeakerNames] = useState<ReadonlySet<string>>(new Set());
  const [focusedSpeakers, setFocusedSpeakers] = useState<ReadonlySet<string>>(new Set());
  const speakerControlLayout = LAYOUTS[layoutId === "auto" ? detectedLayout ?? "7.1.4" : layoutId];
  const speakerMixLocked = speakerControlLayout.some(speaker => focusedSpeakers.has(speaker.name));
  const speakerFocusLocked = speakerControlLayout.some(speaker => mutedSpeakerNames.has(speaker.name) || soloSpeakerNames.has(speaker.name));
  useEffect(() => {
    // Prune unavailable speakers when the layout changes; keep the others selected.
    setFocusedSpeakers(current => {
      const next = new Set([...current].filter(name => !speakerFocusLocked && speakerControlLayout.some(speaker => speaker.name === name)));
      return next.size === current.size ? current : next;
    });
  }, [speakerControlLayout, speakerFocusLocked]);
  const toggleSpeakerFocus = useCallback((name: string) => {
    if (speakerFocusLocked) return;
    setFocusedSpeakers(current => {
      const next = new Set(current);
      if (next.has(name)) next.delete(name); else next.add(name);
      return next;
    });
  }, [speakerFocusLocked]);
  const toggleSpeakerMuteGroup = useCallback((names: readonly string[]) => {
    if (speakerMixLocked) return;
    setMutedSpeakerNames(current => {
      const next = new Set(current);
      const remove = names.every(name => current.has(name));
      for (const name of names) { if (remove) next.delete(name); else next.add(name); }
      return next;
    });
  }, [speakerMixLocked]);
  const toggleSpeakerSoloGroup = useCallback((names: readonly string[], clear = false) => {
    if (speakerMixLocked) return;
    setSoloSpeakerNames(current => {
      if (clear) return new Set();
      const next = new Set(current);
      const remove = names.every(name => current.has(name));
      for (const name of names) { if (remove) next.delete(name); else next.add(name); }
      return next;
    });
  }, [speakerMixLocked]);
  const [position, setPosition] = useState(0);
  const [duration, setDuration] = useState(0);
  const [debug, setDebug] = useState("");
  const [health, setHealth] = useState<PlayerHealthSnapshot | null>(null);
  /** Internal adaptive state, persisted before future player construction. */
  const outputLatencySecondsRef = useRef<OutputLatencySeconds>(readOutputLatencySeconds());
  /** 运行期错误只进 console，不再在页面上显示日志面板。 */
  const [, setErrors] = useState<string[]>([]);
  const [playing, setPlaying] = useState(false);
  const [paused, setPaused] = useState(false);
  /** 暂停意图的 ref 镜像：player 还在创建中（createPlayer 未 resolve）时按暂停，
   *  playerRef 是空的、pause() 会丢 —— play() 建好 player 后按此补发。 */
  const pausedRef = useRef(false);
  const [volume, setVolume] = useState(1);
  const [volumeBalanceEnabled, setVolumeBalanceEnabled] = useState(readVolumeBalanceEnabled);
  const [programLoudness, setProgramLoudness] = useState<ProgramLoudnessMetadata | null>(null);
  const [binauralEqBands, setBinauralEqBands] = useState<BinauralEqBands>(() => {
    const readBand = (band: keyof BinauralEqBands) => {
      const value = Number(localStorage.getItem(`sda-binaural-eq-${band}-db`));
      return Number.isFinite(value) ? Math.max(-12, Math.min(12, value)) : 0;
    };
    return { low: readBand("low"), mid: readBand("mid"), high: readBand("high") };
  });
  const [binauralLowFrequencyDiagnostic, setBinauralLowFrequencyDiagnostic] = useState<BinauralLowFrequencyDiagnostic>(readBinauralLowFrequencyDiagnostic);
  /** 完整人头/受试者 HRTF 档案。 */
  const [binauralHead, setBinauralHead] = useState<BinauralHead>(readBinauralHead);
  /** 逐对象精确方向双耳渲染（实验性，仅完整 KU100 资产族）。 */
  const [denseBinauralObjects, setDenseBinauralObjects] = useState<boolean>(
    () => readBinauralHead() === "ku100" && readDenseBinauralObjects(),
  );
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [directObjectHrtf, setDirectObjectHrtf] = useState(() => localStorage.getItem("sda-direct-object-hrtf") === "true");
  const [directObjectHrtfBusy, setDirectObjectHrtfBusy] = useState(false);
  const [headTrackingStatus, setHeadTrackingStatus] = useState<HeadTrackingStatus | null>(null);
  /** Desktop playback is owned by the WASAPI native object renderer. */
  const [nativeRendererStatus, setNativeRendererStatus] = useState<NativeRendererStatus | null>(null);
  const nativeRendererRunningRef = useRef(false);
  const nativeRendererSampleRef = useRef(0);
  /** Invalidates every native sink owned by a replaced player immediately. */
  const nativeSessionEpochRef = useRef(0);
  nativeRendererRunningRef.current = nativeRendererStatus?.running === true;
  nativeRendererSampleRef.current = nativeRendererStatus?.samplePos ?? nativeRendererSampleRef.current;
  const [nativeRendererBusy, setNativeRendererBusy] = useState(false);
  const [headTrackingHelper, setHeadTrackingHelper] = useState<HeadTrackingHelperConfiguration | null>(null);
  const [headTrackingBusy, setHeadTrackingBusy] = useState(false);
  const [headTrackingTelemetry, setHeadTrackingTelemetry] = useState<HeadTrackingTelemetrySample[]>([]);
  const headTrackingSessionRef = useRef(new HeadTrackingSession());
  const previousTelemetryPoseRef = useRef<{ orientation: Quaternion; timestampMs: number } | null>(null);
  const lastTelemetryUiUpdateRef = useRef(0);
  const [floatPanel, setFloatPanel] = useState<"stream" | "binaural" | "headphone" | "head-tracking" | "objects" | "channels" | "playlist" | "pinna" | null>(null);
  const [playlist, setPlaylist] = useState<PlaylistItem[]>([]);
  const [playlistCurrentId, setPlaylistCurrentId] = useState<string | null>(null);
  /** null = 不改写 KU100 空间化后的最终双耳信号。 */
  const [headphoneProfileId, setHeadphoneProfileId] = useState<string | null>(null);
  const [headphoneProfiles, setHeadphoneProfiles] = useState(() => availableHeadphoneCompensationProfiles());
  const [profileBusy, setProfileBusy] = useState(false);
  const coverUrlRef = useRef<string | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const lastSourceRef = useRef<PlaybackSource | null>(null);
  /** 静音推送回调用的最新对象列表（避免闭包拿旧 state）。 */
  const objectsRef = useRef<VisualObject[]>([]);
  /** 当前文件名，容器没有标题元数据时给 miniplayer 兜底用。 */
  const fileNameRef = useRef<string | null>(null);
  /** 当前曲目的实测响度缓存键（音量平衡用于无元数据内容）。 */
  const measuredLoudnessKeyRef = useRef<string | null>(null);
  /** A monotonically increasing token makes the most recent play request win. */
  const playRequestRef = useRef(0);
  const playlistItemSerialRef = useRef(0);
  const playlistRef = useRef<PlaylistItem[]>([]);
  const playlistCurrentIdRef = useRef<string | null>(null);
  /** Invalidates ended callbacks from an item removed/replaced mid-playback. */
  const playlistRevisionRef = useRef(0);
  const playingRef = useRef(false);
  const playRef = useRef<(source: PlaybackSource) => Promise<void>>(async () => {});
  const playPlaylistItemRef = useRef<(id: string) => void>(() => {});
  const volumeRef = useRef(volume);
  const volumeBalanceRef = useRef(volumeBalanceEnabled);
  const binauralEqBandsRef = useRef(binauralEqBands);
  const binauralLowFrequencyDiagnosticRef = useRef(binauralLowFrequencyDiagnostic);
  volumeRef.current = volume;
  volumeBalanceRef.current = volumeBalanceEnabled;
  binauralEqBandsRef.current = binauralEqBands;
  binauralLowFrequencyDiagnosticRef.current = binauralLowFrequencyDiagnostic;
  playlistRef.current = playlist;
  playlistCurrentIdRef.current = playlistCurrentId;
  playingRef.current = playing;

  const createPlayer = useCallback(
    async (
      m: OutputMode,
      lid: LayoutId | "auto",
      isCurrent: () => boolean,
      playbackPlaylistRevision: number,
    ) => {
      // Assigned right after construction; worker callbacks fire later (async).
      let createdPlayer: SdaPlayer | null = null;
      let nativeSessionReady = false;
      const nativeSessionEpoch = nativeSessionEpochRef.current;
      const isNativeSessionCurrent = () => nativeSessionEpochRef.current === nativeSessionEpoch && isCurrent();
      const ownsNativeSession = () => nativeSessionReady && isNativeSessionCurrent();
      const desktop = window.sdaDesktop;
      if (!desktop?.startNativeRenderer || !desktop.getNativeRendererStatus || !desktop.nativeRendererHrtf) {
        throw new Error("SDA Desktop native renderer is required for audio playback");
      }
      let nativeStatus = await desktop.getNativeRendererStatus();
      if (!nativeStatus.running) await desktop.startNativeRenderer();
      if (!isNativeSessionCurrent()) throw new Error("native renderer replacement session expired");
      const hrtfQueued = await desktop.nativeRendererHrtf(nativeHrtfSetName(readBinauralHead()), 0.04);
      if (!await desktop.nativeRendererObjectHrtf?.(localStorage.getItem("sda-direct-object-hrtf") === "true")) {
        throw new Error("逐对象 HRTF 设置未被原生渲染器接受");
      }
      if (!hrtfQueued) throw new Error("native renderer could not queue the selected complete HRTF set");
      if (!isNativeSessionCurrent()) throw new Error("native renderer replacement session expired");
      // A replacement session owns the sidecar from this exact point. Reset it
      // before any new source declaration; an outgoing player must never reset
      // or remove sources later, after this session starts submitting PCM.
      const resetQueued = await desktop.nativeRendererReset?.(0);
      if (resetQueued === false) throw new Error("native renderer could not reset the replacement session");
      if (!isNativeSessionCurrent()) throw new Error("native renderer replacement session expired");
      nativeSessionReady = true;
      nativeRendererRunningRef.current = true;
      nativeRendererSampleRef.current = 0;
      setNativeRendererStatus({ ...nativeStatus, running: true, hrtfReady: true, samplePos: 0 });
      // All desktop PCM, source lifecycle, object events, and pose updates are
      // serialized through the WASAPI sidecar. A frame resolves only after its
      // full codec batch was accepted by the native ring.
      let nativeCommandChain: Promise<void> = Promise.resolve();
      const enqueueNative = <T,>(
        label: string,
        operation: () => T | Promise<T>,
        rejectFalse = true,
      ): Promise<T> => {
        const result = nativeCommandChain.then(async () => {
          if (!ownsNativeSession()) throw new Error(`native sidecar session expired before ${label}`);
          const value = await operation();
          if (!ownsNativeSession()) throw new Error(`native sidecar session expired during ${label}`);
          if (rejectFalse && (value as unknown) === false) {
            throw new Error(`native sidecar rejected ${label}`);
          }
          return value;
        });
        // A failed operation rejects its caller but must not permanently poison
        // the serial transport: a later source declaration may repair the state.
        nativeCommandChain = result.then(() => undefined, () => undefined);
        return result;
      };
      const nativeSourceAcks = new Map<string, Promise<void>>();
      const nativeSourceDeclarations = new Map<string, NativeRendererSourceDeclaration>();
      const nativeSourceKey = (source: NativeRendererSourceDeclaration) =>
        `${source.id}|${source.bedLabel ?? "object"}`;
      const nativeRendererSink: NativeRendererSink | undefined = desktop?.nativeRendererSource && desktop.nativeRendererFrame
        ? {
            addSource: async (source) => {
              if (!ownsNativeSession()) throw new Error("native renderer session unavailable");
              const key = nativeSourceKey(source);
              nativeSourceDeclarations.set(source.id, source);
              const known = nativeSourceAcks.get(key);
              if (known) return known;
              const declared = enqueueNative(
                `addSource ${source.id}@${source.atSample}`,
                () => desktop.nativeRendererSource!(source),
              ).then(() => {});
              nativeSourceAcks.set(key, declared);
              try {
                await declared;
              } catch (error) {
                if (nativeSourceAcks.get(key) === declared) nativeSourceAcks.delete(key);
                throw error;
              }
            },
            removeSource: async (id, atSample) => {
              if (!ownsNativeSession()) return;
              for (const key of nativeSourceAcks.keys()) {
                if (key.startsWith(`${id}|`)) nativeSourceAcks.delete(key);
              }
              nativeSourceDeclarations.delete(id);
              await enqueueNative(`removeSource ${id}@${atSample}`, () => desktop.nativeRendererRemoveSource?.(id, atSample));
            },
            setMuted: async (id, muted, atSample) => {
              if (!ownsNativeSession()) return;
              await enqueueNative(`setMuted ${id}=${muted}`, () => desktop.nativeRendererMuted?.(id, muted, atSample));
            },
            setLfeMuted: async (muted) => {
              if (!ownsNativeSession()) return;
              await enqueueNative(`setLfeMuted=${muted}`, () => desktop.nativeRendererLfeMuted?.(muted));
            },
            setSpeakerMutes: async (names, focus) => {
              if (!ownsNativeSession()) return;
              if (!desktop.nativeRendererSpeakerMutes) throw new Error("请重启 Electron 以加载音箱控制接口");
              await enqueueNative("setSpeakerMutes", () => desktop.nativeRendererSpeakerMutes!(names, focus));
            },
            setVolume: async (volume) => {
              if (!ownsNativeSession()) return;
              await enqueueNative(`setVolume=${volume}`, () => desktop.nativeRendererVolume?.(volume));
            },
            setProgramEnabled: async (enabled) => {
              if (!ownsNativeSession()) return;
              await enqueueNative(`setProgramEnabled=${enabled}`, () => desktop.nativeRendererProgramEnabled?.(enabled));
            },
            setProgramGainDb: async (gainDb, atSample) => {
              if (!ownsNativeSession()) return;
              const gain = gainDb === null || !Number.isFinite(gainDb)
                ? 1
                : Math.pow(10, Math.min(0, gainDb) / 20);
              await enqueueNative(
                `setProgramGain=${gain}@${atSample ?? "now"}`,
                () => desktop.nativeRendererProgramGain?.(gain, atSample),
              );
            },
            setBinauralEq: async (bands, lowCut) => {
              if (!ownsNativeSession()) return;
              await enqueueNative(
                `setBinauralEq low=${bands.low} mid=${bands.mid} high=${bands.high} lowCut=${lowCut}`,
                () => desktop.nativeRendererBinauralEq?.(bands, lowCut),
              );
            },
            setHeadphoneProfile: async (id) => {
              if (!ownsNativeSession()) return;
              await enqueueNative(
                `setHeadphoneProfile=${id ?? "bypass"}`,
                () => desktop.nativeRendererHeadphoneProfile?.(id),
              );
            },
            setLayout: async (layout) => {
              if (!ownsNativeSession()) return;
              await enqueueNative(`setLayout=${layout}`, () => desktop.nativeRendererLayout?.(layout));
            },
            events: async (events) => {
              if (!ownsNativeSession()) throw new Error("native renderer session unavailable");
              await enqueueNative(`objectEvents (${events.length})`, () => desktop.nativeRendererEvents?.(events));
            },
            frame: async (samplePos, entries) => {
              if (!ownsNativeSession()) return { accepted: false, samples: 0, reason: "native renderer session unavailable" };
              let result = await enqueueNative(
                `frame @${samplePos} (${entries.length} sources)`,
                () => desktop.nativeRendererFrame!(samplePos, entries),
                false,
              );
              // A stale player cleanup or sidecar reset can remove sources after
              // their declaration promise was cached. Re-declare and retry this
              // exact codec batch once instead of dropping the current track.
              if (!result.accepted && /unknown source/i.test(result.reason ?? "")) {
                for (const entry of entries) {
                  for (const key of nativeSourceAcks.keys()) {
                    if (key.startsWith(`${entry.id}|`)) nativeSourceAcks.delete(key);
                  }
                }
                for (const entry of entries) {
                  const source = nativeSourceDeclarations.get(entry.id) ?? { id: entry.id, atSample: samplePos };
                  await nativeRendererSink!.addSource(source);
                }
                result = await enqueueNative(
                  `frame retry @${samplePos} (${entries.length} sources)`,
                  () => desktop.nativeRendererFrame!(samplePos, entries),
                  false,
                );
              }
              return result;
            },
            reset: async (origin) => {
              if (!ownsNativeSession()) return;
              await enqueueNative(`reset ${origin}`, () => desktop.nativeRendererReset?.(origin));
            },
            setHeadPose: (pose) => {
              if (ownsNativeSession()) void enqueueNative("headPose", () => desktop.nativeRendererPose?.(pose.orientation)).catch((error) => {
                console.warn("[SDA] native head pose rejected:", error);
              });
            },
            clearHeadPose: () => {
              if (ownsNativeSession()) void enqueueNative("clearHeadPose", () => desktop.nativeRendererClearPose?.()).catch((error) => {
                console.warn("[SDA] native clear head pose rejected:", error);
              });
            },
            startAt: async (origin) => {
              if (!ownsNativeSession()) return false;
              try {
                await enqueueNative(`startAt ${origin}`, () => desktop.nativeRendererStartAt?.(origin) ?? false);
                return true;
              } catch (error) {
                console.warn("[SDA] native startAt rejected:", error);
                return false;
              }
            },
            pause: async (paused) => {
              if (!ownsNativeSession()) return false;
              try {
                await enqueueNative(`pause=${paused}`, () => desktop.nativeRendererPause?.(paused) ?? false);
                return true;
              } catch (error) {
                console.warn("[SDA] native pause rejected:", error);
                return false;
              }
            },
            getConsumedSamples: () => nativeRendererSampleRef.current,
            onConsumedSamples: (callback) => desktop.onNativeRendererStatus?.((status) => {
              const samplePos = status.samplePos;
              if (!ownsNativeSession() || !status.running || typeof samplePos !== "number" || !Number.isSafeInteger(samplePos)) return;
              nativeRendererSampleRef.current = samplePos;
              callback(samplePos);
            }),
            onObjectActivity: (callback) => desktop.onNativeRendererObjectActivity?.((activity) => {
              if (!ownsNativeSession() || !Array.isArray(activity?.ids)) return;
              callback(activity.ids);
            }),
          }
        : undefined;
      const player = new SdaPlayer({
        onOutputLatencyRecommendation: (seconds) => {
          if (!isCurrent()) return;
          persistOutputLatencySeconds(seconds);
          outputLatencySecondsRef.current = seconds;
        },
        onTrack: (t) => {
          if (!isCurrent()) return;
          if (coverUrlRef.current) URL.revokeObjectURL(coverUrlRef.current);
          const coverUrl = t.coverArt
            ? URL.createObjectURL(new Blob([ownedArrayBuffer(t.coverArt.bytes)], { type: t.coverArt.mimeType }))
            : undefined;
          coverUrlRef.current = coverUrl ?? null;
          setTrack({ ...t, coverUrl, title: t.title ?? fileNameRef.current ?? undefined });
          setProgramLoudness(null);
          // Feed the persisted measurement so balance applies from sample 0.
          const key = measuredLoudnessStorageKey(t);
          measuredLoudnessKeyRef.current = key;
          try {
            const cached = Number(localStorage.getItem(key));
            createdPlayer?.setMeasuredLoudness(Number.isFinite(cached) ? cached : null);
          } catch {
            createdPlayer?.setMeasuredLoudness(null);
          }
        },
        onMeasuredLoudness: (integratedLufs) => {
          const key = measuredLoudnessKeyRef.current;
          if (!isCurrent() || !key) return;
          try {
            localStorage.setItem(key, String(integratedLufs));
          } catch {
            // Persistence failures must not affect playback.
          }
        },
        onDecodedFormat: ({ rawBedLabels, bedLabels, objectChannels }) => {
          if (isCurrent()) setTrack((current) => current && { ...current, rawBedLabels, bedLabels, objectChannels });
        },
        onBinauralMetadata: (metadata) => {
          if (isCurrent()) setBinauralMetadata(metadata);
        },
        onVisualState: (objs, t, sounding) => {
          if (!isCurrent()) return;
          objectsRef.current = objs;
          // Visual events can fire per decoded frame while objects move, and
          // every state write here re-renders the whole app in competition with
          // the 3D paint. Flush at ~15 Hz (10 Hz baseline from the player
          // timer is untouched); ObjectDot eases toward targets at full frame
          // rate, so this only caps target delivery, not motion smoothness.
          // Paused or seek emissions flush immediately — no further ticks may
          // follow them.
          const now = performance.now();
          if (!playingRef.current || t === 0 || now - lastVisualUiUpdateRef.current >= 66) {
            lastVisualUiUpdateRef.current = now;
            setObjects(objs);
            setSoundingObjectIds(sounding);
            if (t === 0 || t - lastDiagnosticUpdateRef.current >= 0.2) {
              lastDiagnosticUpdateRef.current = t;
              setDiagnosticObjects(objs);
              setProgramLoudness(playerRef.current?.programLoudnessInfo() ?? null);
            }
            setPosition(t);
            const p = playerRef.current;
            setDuration(p?.durationSeconds() ?? 0);
            setDebug(p ? `#${p.id} 已解码 ${p.durationSeconds().toFixed(1)}s / 播放头 ${t.toFixed(1)}s` : "");
          }
        },
        onHealth: (snapshot) => {
          if (!isCurrent()) return;
          const now = performance.now();
          const escalationChanged = snapshot.callbackGapEscalation !== lastHealthEscalationRef.current;
          if (!escalationChanged && now - lastHealthUiUpdateRef.current < 250) return;
          lastHealthUiUpdateRef.current = now;
          lastHealthEscalationRef.current = snapshot.callbackGapEscalation;
          setHealth(snapshot);
        },
        onError: (message) => {
          if (!isCurrent()) return;
          console.warn(`[SDA] ${message}`);
          setErrors((prev) => [...prev.slice(-19), message]);
        },
        onEnded: () => {
          // A queue edit or playback request invalidates the old item's
          // completion so it cannot advance a replacement/cleared playlist.
          if (!isCurrent() || playlistRevisionRef.current !== playbackPlaylistRevision) return;
          const currentId = playlistCurrentIdRef.current;
          const currentIndex = playlistRef.current.findIndex((item) => item.id === currentId);
          const next = currentIndex >= 0 ? playlistRef.current[currentIndex + 1] : null;
          if (next) playPlaylistItemRef.current(next.id);
          else {
            playingRef.current = false;
            setPlaying(false);
          }
        },
      }, {
        initialOutputLatencySeconds: outputLatencySecondsRef.current,
        denseBinauralObjects: readBinauralHead() === "ku100" && readDenseBinauralObjects(),
        denseBinauralBaseUrl: denseBinauralBaseUrl(),
        outputBackend: "native-sidecar",
        nativeRendererSink: nativeRendererSink!,
        // KU100 stays at the room origin while world-locked sources are viewed
        // through the inverse head rotation. Apple-like feel: 1:1 rotation, a
        // few hundred ms of damping, and a dead zone that swallows tremor — the
        // drift fixes made the old 1.5x amplification read as hypersensitive.
        headPose: {
          yawMode: "yaw",
          sensitivity: 1,
          smoothingMs: 220,
          deadZoneDegrees: 1,
          maxDegreesPerSecond: 480,
          updateHz: 120,
        },
      });
      createdPlayer = player;
      const fallbackLayout = lid === "auto" ? LAYOUTS["7.1.4"] : LAYOUTS[lid];
      const resolver = (labels: readonly string[], hasDynamics: boolean) => {
        const id = detectLayoutId(labels, hasDynamics);
        setDetectedLayout(id);
        return LAYOUTS[id];
      };
      await player.init(
        m,
        workletUrl,
        fallbackLayout,
        binauralHeadBaseUrl(readBinauralHead()),
        resolver,
        lid === "auto",
      );
      const requestedLayout = layoutIdRef.current;
      if (requestedLayout !== lid) {
        if (requestedLayout === "auto") player.setAutoLayout();
        else player.setLayout(LAYOUTS[requestedLayout]);
      }
      if (!isCurrent()) {
        await player.dispose();
        return null;
      }
      const latestHeadPose = headTrackingSessionRef.current.latestPose;
      if (latestHeadPose) player.setHeadPose(latestHeadPose);
      player.setVolume(volumeRef.current);
      player.setVolumeBalance(volumeBalanceRef.current);
      player.setBinauralEqBands(binauralEqBandsRef.current);
      player.setBinauralLowFrequencyDiagnostic(binauralLowFrequencyDiagnosticRef.current);
      return player;
    },
    [],
  );

  useEffect(
    () => () => {
      // Invalidate a player still awaiting sidecar/HRTF setup before disposing
      // published instances. This prevents a dev remount from reviving a stale
      // session after the component has gone away.
      playRequestRef.current++;
      nativeSessionEpochRef.current++;
      playingRef.current = false;
      if (coverUrlRef.current) URL.revokeObjectURL(coverUrlRef.current);
      void playerRef.current?.dispose();
      if (retiringPlayerRef.current !== playerRef.current) void retiringPlayerRef.current?.dispose();
      setPlayerReady(null);
    },
    [],
  );

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  useEffect(() => {
    if (headTrackingStatus?.running) return;
    headTrackingSessionRef.current.clear();
    previousTelemetryPoseRef.current = null;
    lastTelemetryUiUpdateRef.current = 0;
    setHeadTrackingTelemetry([]);
    setFloatPanel((panel) => panel === "head-tracking" ? null : panel);
  }, [headTrackingStatus?.running]);

  useEffect(() => {
    const desktop = window.sdaDesktop;
    if (!desktop?.getHeadTrackingStatus) return;
    void desktop.getHeadTrackingStatus().then(setHeadTrackingStatus).catch((error) => {
      console.warn("[SDA] 读取头部追踪状态失败:", error);
    });
    void desktop.getHeadTrackingHelper?.().then(setHeadTrackingHelper).catch((error) => {
      console.warn("[SDA] 读取头追 helper 配置失败:", error);
    });
    const stopStatus = desktop.onHeadTrackingStatus?.(setHeadTrackingStatus);
    const applyPose = (pose: HeadTrackingPose) => {
      const headPose = headTrackingSessionRef.current.update(rendererHeadPose(pose));
      const player = playerRef.current as (SdaPlayer & HeadTrackingPlayer) | null;
      player?.setHeadPose?.(headPose);
      const timestampMs = performance.now();
      const orientation: Quaternion = [pose.orientation.x, pose.orientation.y, pose.orientation.z, pose.orientation.w];
      const previous = previousTelemetryPoseRef.current;
      previousTelemetryPoseRef.current = { orientation, timestampMs };
      if (!previous || timestampMs - lastTelemetryUiUpdateRef.current < HEAD_TRACKING_TELEMETRY_INTERVAL_MS) return;
      lastTelemetryUiUpdateRef.current = timestampMs;
      const angularVelocity = quaternionAngularVelocity(previous.orientation, orientation, timestampMs - previous.timestampMs);
      const angles = quaternionEulerAngles(orientation);
      const sample: HeadTrackingTelemetrySample = { timestampMs, ...angularVelocity, ...angles };
      setHeadTrackingTelemetry((history) => [
        ...history.filter((entry) => timestampMs - entry.timestampMs <= HEAD_TRACKING_TELEMETRY_HISTORY_MS),
        sample,
      ]);
    };
    const stopPose = desktop.onHeadTrackingPose?.(applyPose);
    const stopRecenter = desktop.onHeadTrackingRecenter?.((pose) => {
      const player = playerRef.current as (SdaPlayer & HeadTrackingPlayer) | null;
      const headPose = pose
        ? headTrackingSessionRef.current.recenter(rendererHeadPose(pose))
        : headTrackingSessionRef.current.recenter();
      if (headPose) {
        // Drop the renderer-local reference before installing the session-level
        // identity; otherwise a previous per-renderer recenter is applied twice.
        player?.clearHeadPose?.();
        player?.setHeadPose?.(headPose);
      }
    });
    return () => {
      stopStatus?.();
      stopPose?.();
      stopRecenter?.();
    };
  }, []);

  useEffect(() => {
    const desktop = window.sdaDesktop;
    if (!desktop?.getNativeRendererStatus) return;
    void desktop.getNativeRendererStatus().then(setNativeRendererStatus).catch((error) => {
      console.warn("[SDA] 读取 native renderer 状态失败:", error);
    });
    return desktop.onNativeRendererStatus?.(setNativeRendererStatus);
  }, []);

  useEffect(() => {
    const desktop = window.sdaDesktop;
    if (!desktop?.listHeadphoneProfiles || !desktop.readHeadphoneProfile) return;
    void desktop.listHeadphoneProfiles()
      .then(async (manifests) => {
        const entries = await Promise.all(manifests.map(async (profile) => {
          const data = await desktop.readHeadphoneProfile!(profile.id);
          return {
            profile: data.profile,
            leftFir: ownedArrayBuffer(data.leftFir),
            rightFir: ownedArrayBuffer(data.rightFir),
          } satisfies LocalHeadphoneCompensationData;
        }));
        for (const entry of entries) registerLocalHeadphoneCompensation(entry);
        setHeadphoneProfiles(availableHeadphoneCompensationProfiles());
        localStorage.removeItem("sda-headphone-profile-id");
      })
      .catch((error) => setErrors((prev) => [...prev, `加载本地耳机档案失败: ${String(error)}`]));
  }, []);

  const selectHeadTrackingHelper = useCallback(async () => {
    const desktop = window.sdaDesktop;
    if (!desktop?.selectHeadTrackingHelper) return;
    setHeadTrackingBusy(true);
    try {
      setHeadTrackingHelper(await desktop.selectHeadTrackingHelper());
      if (desktop.getHeadTrackingStatus) setHeadTrackingStatus(await desktop.getHeadTrackingStatus());
    } catch (error) {
      console.warn("[SDA] 选择头追 helper 失败:", error);
      setErrors((prev) => [...prev, `选择头追 helper 失败: ${String(error)}`]);
    } finally {
      setHeadTrackingBusy(false);
    }
  }, []);

  const useBundledHeadTrackingHelper = useCallback(async () => {
    const desktop = window.sdaDesktop;
    if (!desktop?.useBundledHeadTrackingHelper) return;
    setHeadTrackingBusy(true);
    try {
      setHeadTrackingHelper(await desktop.useBundledHeadTrackingHelper());
      if (desktop.getHeadTrackingStatus) setHeadTrackingStatus(await desktop.getHeadTrackingStatus());
    } catch (error) {
      console.warn("[SDA] 切换内置头追 helper 失败:", error);
      setErrors((prev) => [...prev, `切换内置头追 helper 失败: ${String(error)}`]);
    } finally {
      setHeadTrackingBusy(false);
    }
  }, []);

  const setHeadTrackingRunning = useCallback(async (running: boolean) => {
    const desktop = window.sdaDesktop;
    const operation = running ? desktop?.startHeadTracking : desktop?.stopHeadTracking;
    if (!operation) return;
    setHeadTrackingBusy(true);
    try {
      setHeadTrackingStatus(await operation());
      if (!running) {
        const player = playerRef.current as (SdaPlayer & HeadTrackingPlayer) | null;
        player?.clearHeadPose?.();
        headTrackingSessionRef.current.clear();
        previousTelemetryPoseRef.current = null;
        lastTelemetryUiUpdateRef.current = 0;
        setHeadTrackingTelemetry([]);
        setFloatPanel((panel) => panel === "head-tracking" ? null : panel);
      }
    } catch (error) {
      console.warn("[SDA] 头部追踪切换失败:", error);
      setErrors((prev) => [...prev, `头部追踪切换失败: ${String(error)}`]);
    } finally {
      setHeadTrackingBusy(false);
    }
  }, []);

  const toggleNativeRenderer = useCallback(async () => {
    const desktop = window.sdaDesktop;
    if (!desktop?.startNativeRenderer || !desktop.stopNativeRenderer) return;
    setNativeRendererBusy(true);
    try {
      const next = nativeRendererStatus?.running
        ? await desktop.stopNativeRenderer()
        : await desktop.startNativeRenderer();
      if (next.running) await desktop.nativeRendererHrtf?.(nativeHrtfSetName(binauralHead), 0.04);
      if (next.running && !await desktop.nativeRendererObjectHrtf?.(localStorage.getItem("sda-direct-object-hrtf") === "true")) {
        throw new Error("逐对象 HRTF 设置未被原生渲染器接受");
      }
      setNativeRendererStatus(next);
    } catch (error) {
      console.warn("[SDA] native renderer 切换失败:", error);
      setErrors((prev) => [...prev, `native renderer 切换失败: ${String(error)}`]);
    } finally {
      setNativeRendererBusy(false);
    }
  }, [nativeRendererStatus?.running]);

  const changeDirectObjectHrtf = async (enabled: boolean) => {
    if (directObjectHrtfBusy) return;
    setDirectObjectHrtfBusy(true);
    try {
      const desktop = window.sdaDesktop;
      const status = await desktop?.getNativeRendererStatus?.();
      if (status?.running && !await desktop?.nativeRendererObjectHrtf?.(enabled)) {
        throw new Error("原生渲染器未接受设置");
      }
      localStorage.setItem("sda-direct-object-hrtf", String(enabled));
      setDirectObjectHrtf(enabled);
    } catch (error) {
      setErrors((prev) => [...prev, `逐对象 HRTF 切换失败: ${String(error)}`]);
    } finally {
      setDirectObjectHrtfBusy(false);
    }
  };

  const recenterHeadTracking = useCallback(async () => {
    const desktop = window.sdaDesktop;
    if (!desktop?.recenterHeadTracking) return;
    setHeadTrackingBusy(true);
    try {
      // The main process broadcasts the pose + recenter event to every window.
      // Do not apply the returned pose a second time: smoothing would otherwise
      // make a multi-window provider recenter inconsistently.
      await desktop.recenterHeadTracking();
    } catch (error) {
      console.warn("[SDA] 头部追踪重置失败:", error);
      setErrors((prev) => [...prev, `头部追踪重置失败: ${String(error)}`]);
    } finally {
      setHeadTrackingBusy(false);
    }
  }, []);

  const importHeadphoneProfile = useCallback(async () => {
    const desktop = window.sdaDesktop;
    if (!desktop?.importHeadphoneProfile) return;
    setProfileBusy(true);
    try {
      const data = await desktop.importHeadphoneProfile();
      if (!data) return;
      registerLocalHeadphoneCompensation({
        profile: data.profile,
        leftFir: ownedArrayBuffer(data.leftFir),
        rightFir: ownedArrayBuffer(data.rightFir),
      });
      setHeadphoneProfiles(availableHeadphoneCompensationProfiles());
    } catch (error) {
      setErrors((prev) => [...prev, `导入耳机档案失败: ${String(error)}`]);
    } finally {
      setProfileBusy(false);
    }
  }, []);

  /** solo 非空时，未独奏对象全部视为静音；独奏对象仍尊重手动静音（mute 优先）。 */
  const effectiveMutedIds = useMemo(() => {
    if (soloIds.size === 0) return mutedIds;
    const next = new Set<number>();
    for (const object of objects) {
      if (!soloIds.has(object.id) || mutedIds.has(object.id)) next.add(object.id);
    }
    return next;
  }, [soloIds, mutedIds, objects]);

  const outputSpeakers = LAYOUTS[layoutId === "auto" ? detectedLayout ?? "7.1.4" : layoutId];
  const [speakerGroup, setSpeakerGroup] = useState<"all" | "top" | "front" | "rear">("all");
  const filteredSpeakers = outputSpeakers.filter(speaker => {
    if (speakerGroup === "all") return true;
    if (speakerGroup === "top") return !speaker.isLfe && speaker.elevation > 0;
    if (speaker.elevation > 0 && !speaker.isLfe) return false;
    if (speaker.isLfe) return speakerGroup === "front";
    const azimuth = Math.abs(((speaker.azimuth + 540) % 360) - 180);
    return speakerGroup === "front" ? azimuth <= 90 : azimuth > 90;
  });
  const activeSpeakerSolo = outputSpeakers.some(speaker => soloSpeakerNames.has(speaker.name));
  const activeSpeakerFocus = useMemo(() => new Set([...focusedSpeakers].filter(name => outputSpeakers.some(speaker => speaker.name === name))), [focusedSpeakers, outputSpeakers]);
  const effectiveSpeakerMutes = useMemo(() => new Set(outputSpeakers
    .filter(speaker => mutedSpeakerNames.has(speaker.name) || (activeSpeakerSolo && !soloSpeakerNames.has(speaker.name)))
    .map(speaker => speaker.name)), [outputSpeakers, mutedSpeakerNames, soloSpeakerNames, activeSpeakerSolo]);
  useEffect(() => {
    playerReady?.syncSpeakerMutes(effectiveSpeakerMutes, activeSpeakerFocus);
  }, [playerReady, effectiveSpeakerMutes, activeSpeakerFocus]);

  // React state and worker frames are asynchronous. Keep the player's durable
  // mute set synchronized whenever either the active player or the UI set changes.
  useEffect(() => {
    playerReady?.syncObjectMutes(effectiveMutedIds);
  }, [playerReady, effectiveMutedIds]);

  /** 把整组静音状态推到播放器（新建播放器后也要重放一遍）。 */
  const applyMutes = useCallback((muted: ReadonlySet<number>) => {
    const player = playerRef.current;
    console.log(
      `[SDA] applyMutes: 静音集=[${[...muted].join(",")}] 面板对象=[${objectsRef.current
        .map((o) => o.id)
        .join(",")}] player=${player ? `#${player.id}` : "无"}`,
    );
    player?.syncObjectMutes(muted);
  }, []);

  /** 手动静音（M）：与 solo 独立；对象被 solo 时仍可手动静音它。 */
  const toggleMute = useCallback(
    (id: number) => {
      const nextMuted = new Set(mutedIds);
      if (nextMuted.has(id)) nextMuted.delete(id);
      else nextMuted.add(id);
      setMutedIds(nextMuted);
      const effective = soloIds.size === 0
        ? nextMuted
        : new Set(objectsRef.current
            .filter((object) => !soloIds.has(object.id) || nextMuted.has(object.id))
            .map((object) => object.id));
      applyMutes(effective);
    },
    [mutedIds, soloIds, applyMutes],
  );

  /**
   * 多对象 solo（S）：点击切换单个对象的独奏；Ctrl/Cmd+点击任意 S 取消全部独奏。
   * solo 不是独立状态，而是"静音其他全部"的快捷方式。
   */
  const toggleSolo = useCallback(
    (id: number, clearAll = false) => {
      const nextSolo = new Set(soloIds);
      if (clearAll) nextSolo.clear();
      else if (nextSolo.has(id)) nextSolo.delete(id);
      else nextSolo.add(id);
      setSoloIds(nextSolo);
      const effective = nextSolo.size === 0
        ? mutedIds
        : new Set(objectsRef.current
            .filter((object) => !nextSolo.has(object.id) || mutedIds.has(object.id))
            .map((object) => object.id));
      applyMutes(effective);
    },
    [soloIds, mutedIds, applyMutes],
  );

  const play = useCallback(
    async (source: PlaybackSource) => {
      const request = ++playRequestRef.current;
      // Claim playback before any await. The preload can synchronously drain a
      // burst of open-file events, and every later append must see this request
      // as active instead of constructing another player/session.
      playingRef.current = true;
      setPlaying(true);
      nativeSessionEpochRef.current++;
      const playbackPlaylistRevision = playlistRevisionRef.current;
      const isCurrent = () => playRequestRef.current === request;
      // The old decoder must stop before the new native session resets to clock
      // zero. Its sink is already invalidated above, but terminating it here also
      // prevents stale decoded frames from consuming queue/pump capacity.
      const previous = playerRef.current;
      if (previous) {
        playerRef.current = null;
        retiringPlayerRef.current = null;
        await previous.dispose();
      }
      setPlayerReady(null);
      if (!isCurrent()) return;
      setErrors([]);
      setTrack(null);
      setBinauralMetadata(null);
      objectsRef.current = [];
      setObjects([]);
      setSoundingObjectIds(new Set());
      setDiagnosticObjects([]);
      lastDiagnosticUpdateRef.current = 0;
      lastVisualUiUpdateRef.current = 0;
      setPosition(0);
      setDuration(0);
      setHealth(null);
      setDetectedLayout(null);
      setPaused(false);
      pausedRef.current = false;
      lastSourceRef.current = source;
      const sourceName = source.kind === "file"
        ? source.file.name
        : source.path.split(/[\\/]/).pop() ?? source.path;
      fileNameRef.current = sourceName.replace(/\.[^.]+$/, "");
      try {
        // Build privately first. A stale request never gets to replace or dispose
        // the active player published by a newer request.
        const player = await createPlayer(mode, layoutId, isCurrent, playbackPlaylistRevision);
        if (!player || !isCurrent()) return;
        playerRef.current = player;
        setPlayerReady(player);
        if (!isCurrent()) {
          if (playerRef.current === player) playerRef.current = null;
          await player.dispose();
          return;
        }
        player.setVolume(volume);
        player.setVolumeBalance(volumeBalanceEnabled);
        player.setBinauralEqBands(binauralEqBandsRef.current);
        player.setBinauralLowFrequencyDiagnostic(binauralLowFrequencyDiagnostic);
        player.setHeadphoneCompensation(headphoneProfileId);
        applyMutes(effectiveMutedIds); // 恢复静音/solo 状态（新播放器默认全不静音）
        const outgoing = retiringPlayerRef.current;
        if (outgoing && outgoing !== player) {
          retiringPlayerRef.current = null;
          await outgoing.dispose();
          // A newer request now owns this player as its outgoing context.
          if (!isCurrent()) return;
        }
        if (!isCurrent() || playerRef.current !== player) return;
        // 建 player 期间用户已按暂停：补发暂停意图
        if (pausedRef.current) void player.pause();
        if (source.kind === "file") {
          await player.playFile(source.file, "auto");
        } else {
          const desktop = window.sdaDesktop;
          if (!desktop?.openPath || !desktop.readSlice || !desktop.close) {
            throw new Error("桌面文件读取接口不可用");
          }
          const opened = await desktop.openPath(source.path);
          try {
            if (!isCurrent() || playerRef.current !== player) return;
            player.open("auto");
            for (let offset = 0; offset < opened.size; offset += FILE_CHUNK_SIZE) {
              const chunk = await desktop.readSlice(opened.id, offset, Math.min(FILE_CHUNK_SIZE, opened.size - offset));
              if (!isCurrent() || playerRef.current !== player) return;
              if (chunk.byteLength === 0) throw new Error(`文件在 ${offset} 字节处提前结束`);
              await player.push(chunk);
            }
            if (isCurrent() && playerRef.current === player) player.end();
          } finally {
            await desktop.close(opened.id);
          }
        }
      } catch (e) {
        if (!isCurrent()) return;
        const outgoing = retiringPlayerRef.current;
        retiringPlayerRef.current = null;
        if (outgoing) await outgoing.dispose().catch(() => {});
        setErrors((prev) => [...prev, String(e)]);
        playingRef.current = false;
        setPlaying(false);
      }
    },
    [createPlayer, mode, layoutId, volume, volumeBalanceEnabled, binauralLowFrequencyDiagnostic, headphoneProfileId, applyMutes, effectiveMutedIds],
  );
  playRef.current = play;

  const playPlaylistItem = useCallback((id: string) => {
    const item = playlistRef.current.find((candidate) => candidate.id === id);
    if (!item) return;
    playlistCurrentIdRef.current = id;
    setPlaylistCurrentId(id);
    void play(item.source);
  }, [play]);
  playPlaylistItemRef.current = playPlaylistItem;

  const appendToPlaylist = useCallback((sources: readonly PlaybackSource[]) => {
    const existing = playlistRef.current;
    const identities = new Set(existing.map((item) => item.identity));
    const additions: PlaylistItem[] = [];
    for (const source of sources) {
      const identity = source.kind === "path"
        ? `path:${source.path.toLowerCase()}`
        : `file:${source.file.name}:${source.file.size}:${source.file.lastModified}`;
      if (identities.has(identity)) continue;
      identities.add(identity);
      const title = source.kind === "path"
        ? source.path.split(/[\\/]/).pop() ?? source.path
        : source.file.name;
      additions.push({ id: `playlist-${++playlistItemSerialRef.current}`, source, title, identity });
    }
    if (additions.length === 0) return;
    const next = [...existing, ...additions];
    playlistRef.current = next;
    setPlaylist(next);
    const firstAddition = additions[0];
    if (
      firstAddition &&
      (!playlistCurrentIdRef.current || (!playingRef.current && !pausedRef.current))
    ) playPlaylistItemRef.current(firstAddition.id);
  }, []);

  useEffect(() => window.sdaDesktop?.onOpenFile?.((path) => {
    appendToPlaylist([{ kind: "path", path }]);
  }), [appendToPlaylist]);

  const onDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      setDragOver(false);
      appendToPlaylist([...e.dataTransfer.files].map((file) => ({ kind: "file", file })));
    },
    [appendToPlaylist],
  );

  /** 播放中 → 暂停；暂停中 → 继续；已播完 → 重播（macOS 播放键行为）。
   *  UI 状态立即切换（乐观更新），不等 suspend/resume 的 promise —
   *  某些环境下这些 promise 不 resolve，会表现为按钮"没反应"。 */
  const togglePlay = useCallback(() => {
    const player = playerRef.current;
    if (playing && !paused) {
      pausedRef.current = true;
      setPaused(true);
      void player?.pause();
    } else if (paused) {
      pausedRef.current = false;
      setPaused(false);
      void player?.resume();
    } else if (lastSourceRef.current) {
      void play(lastSourceRef.current);
    }
  }, [playing, paused, play]);

  const changeOutputMode = useCallback((next: OutputMode) => {
    playerRef.current?.setOutputMode(next);
    if (next === "stereo" && layoutId !== "2.0" && layoutId !== "2.1") {
      layoutIdRef.current = "2.0";
      setLayoutId("2.0");
      setDetectedLayout(null);
      playerRef.current?.setLayout(LAYOUTS["2.0"]);
    }
    setMode(next);
  }, [layoutId]);

  const changeLayout = useCallback((next: LayoutId | "auto") => {
    layoutIdRef.current = next;
    setLayoutId(next);
    if (next === "auto") {
      playerRef.current?.setAutoLayout();
      return;
    }
    setDetectedLayout(null);
    playerRef.current?.setLayout(LAYOUTS[next]);
  }, []);

  const changeVolume = useCallback((v: number) => {
    setVolume(v);
    playerRef.current?.setVolume(v);
  }, []);

  const changeVolumeBalance = useCallback((enabled: boolean) => {
    setVolumeBalanceEnabled(enabled);
    persistVolumeBalanceEnabled(enabled);
    playerRef.current?.setVolumeBalance(enabled);
  }, []);

  const changeBinauralEqBand = useCallback((band: keyof BinauralEqBands, db: number) => {
    const next = { ...binauralEqBands, [band]: Math.max(-12, Math.min(12, db)) };
    setBinauralEqBands(next);
    localStorage.setItem(`sda-binaural-eq-${band}-db`, String(next[band]));
    localStorage.removeItem("sda-headphone-accommodations-enabled");
    localStorage.removeItem("sda-headphone-accommodations-tone");
    localStorage.removeItem("sda-headphone-accommodations-soft-sound-db");
    playerRef.current?.setBinauralEqBands(next);
  }, [binauralEqBands]);

  const changeHeadphoneCompensation = useCallback((id: string) => {
    const next = id || null;
    setHeadphoneProfileId(next);
    if (next) localStorage.setItem("sda-headphone-profile-id", next);
    else localStorage.removeItem("sda-headphone-profile-id");
    playerRef.current?.setHeadphoneCompensation(next);
  }, []);

  const changeBinauralLowFrequencyDiagnostic = useCallback((next: BinauralLowFrequencyDiagnostic) => {
    persistBinauralLowFrequencyDiagnostic(next);
    setBinauralLowFrequencyDiagnostic(next);
    playerRef.current?.setBinauralLowFrequencyDiagnostic(next);
  }, []);

  const changeBinauralHead = useCallback((next: BinauralHead) => {
    try {
      localStorage.setItem(BINAURAL_HEAD_STORAGE_KEY, next);
    } catch {
      // 持久化失败不影响本次切换。
    }
    setBinauralHead(next);
    // The 61-direction dense object set is KU100-only. Keeping it active with a
    // complete D2/Hx subject set would recreate a cross-subject HRTF hybrid.
    if (next !== "ku100") {
      try { localStorage.setItem(DENSE_BINAURAL_STORAGE_KEY, "0"); } catch {}
      setDenseBinauralObjects(false);
      void playerRef.current?.setDenseBinauralObjects(false);
    }
    void playerRef.current?.setBinauralHead(binauralHeadBaseUrl(next));
    if (nativeRendererRunningRef.current) void window.sdaDesktop?.nativeRendererHrtf?.(nativeHrtfSetName(next), 0.04);
  }, []);

  const [denseBinauralBusy, setDenseBinauralBusy] = useState(false);
  const changeDenseBinauralObjects = useCallback(async (on: boolean) => {
    if (denseBinauralBusy) return;
    const allowed = binauralHead === "ku100";
    const next = allowed && on;
    setDenseBinauralBusy(true);
    try {
      const desktop = window.sdaDesktop;
      const status = await desktop?.getNativeRendererStatus?.();
      if (status?.running && !await desktop?.nativeRendererHrtf?.(nativeHrtfSetName(binauralHead, next), 0.04)) {
        throw new Error("原生渲染器未接受高解析 HRTF 设置");
      }
      await playerRef.current?.setDenseBinauralObjects(next, denseBinauralBaseUrl());
      try { localStorage.setItem(DENSE_BINAURAL_STORAGE_KEY, next ? "1" : "0"); } catch {}
      setDenseBinauralObjects(next);
    } catch (error) {
      setErrors((prev) => [...prev, `高解析 HRTF 切换失败: ${String(error)}`]);
    } finally {
      setDenseBinauralBusy(false);
    }
  }, [binauralHead, denseBinauralBusy]);

  const resetBinauralEq = useCallback(() => {
    for (const band of ["low", "mid", "high"] as const) localStorage.setItem(`sda-binaural-eq-${band}-db`, "0");
    const next = { low: 0, mid: 0, high: 0 };
    setBinauralEqBands(next);
    playerRef.current?.setBinauralEqBands(next);
  }, []);

  const selectedHeadphoneProfile = headphoneProfiles.find((profile) => profile.id === headphoneProfileId) ?? null;
  const stereoProgram = track?.objectChannels === 0
    && track.bedLabels?.length === 2
    && new Set(track.bedLabels).size === 2
    && track.bedLabels.some((label) => label === "L" || label === "FrontLeft")
    && track.bedLabels.some((label) => label === "R" || label === "FrontRight");

  // A decoded fixed L/R programme has no meaningful immersive speaker layout.
  // Lock it to 2.0 initially; users may then opt into 2.1, which retains the
  // same binaural FL/FR room while keeping a discrete LFE direct path.
  useEffect(() => {
    if (!stereoProgram || layoutId === "2.0" || layoutId === "2.1") return;
    layoutIdRef.current = "2.0";
    setLayoutId("2.0");
    setDetectedLayout(null);
    playerRef.current?.setLayout(LAYOUTS["2.0"]);
  }, [stereoProgram, layoutId]);

  const replay = useCallback(() => {
    const source = lastSourceRef.current;
    if (source) void play(source);
  }, [play]);

  const openFile = useCallback(async () => {
    const desktop = window.sdaDesktop;
    if (desktop?.pickFile) {
      const path = await desktop.pickFile();
      if (path) appendToPlaylist([{ kind: "path", path }]);
      return;
    }
    const input = document.createElement("input");
    input.type = "file";
    input.multiple = true;
    input.accept = ".mkv,.mka,.mp4,.m4a,.wav,.bwf,.rf64,.thd,.mlp,.ec3,.eac3,.ac3,.dts";
    input.onchange = () => appendToPlaylist([...input.files ?? []].map((file) => ({ kind: "file", file })));
    input.click();
  }, [appendToPlaylist]);

  const openFolder = useCallback(async () => {
    const result = await window.sdaDesktop?.pickFolder?.();
    if (!result || result.canceled) return;
    appendToPlaylist(result.paths.map((path) => ({ kind: "path", path })));
  }, [appendToPlaylist]);

  const removePlaylistItem = useCallback((id: string) => {
    const current = playlistRef.current;
    const index = current.findIndex((item) => item.id === id);
    if (index < 0) return;
    const next = current.filter((item) => item.id !== id);
    const replacingCurrent = playlistCurrentIdRef.current === id;
    // Removing an upcoming/non-current item must still let the current item's
    // end callback choose the new next item. Only invalidate when its identity
    // is being replaced or removed.
    if (replacingCurrent) playlistRevisionRef.current++;
    playlistRef.current = next;
    setPlaylist(next);
    if (!replacingCurrent) return;
    const replacement = next[index] ?? next[index - 1] ?? null;
    playlistCurrentIdRef.current = replacement?.id ?? null;
    setPlaylistCurrentId(replacement?.id ?? null);
    if (replacement) playPlaylistItemRef.current(replacement.id);
    else {
      // Invalidate the active reader and dispose it; SdaPlayer intentionally has
      // no public stop because file/session disposal is the safe stop boundary.
      playRequestRef.current++;
      const active = playerRef.current;
      const retiring = retiringPlayerRef.current;
      playerRef.current = null;
      retiringPlayerRef.current = null;
      setPlayerReady(null);
      void active?.dispose();
      if (retiring !== active) void retiring?.dispose();
      playingRef.current = false;
      pausedRef.current = false;
      setPlaying(false);
      setPaused(false);
    }
  }, []);

  const clearPlaylist = useCallback(() => {
    playlistRevisionRef.current++;
    playlistRef.current = [];
    playlistCurrentIdRef.current = null;
    setPlaylist([]);
    setPlaylistCurrentId(null);
    playRequestRef.current++;
    const active = playerRef.current;
    const retiring = retiringPlayerRef.current;
    playerRef.current = null;
    retiringPlayerRef.current = null;
    setPlayerReady(null);
    void active?.dispose();
    if (retiring !== active) void retiring?.dispose();
    playingRef.current = false;
    pausedRef.current = false;
    setPlaying(false);
    setPaused(false);
  }, []);

  return (
    <div
      className={`app ${dragOver ? "drag" : ""}`}
      onDragOver={(e) => {
        e.preventDefault();
        setDragOver(true);
      }}
      onDragLeave={() => setDragOver(false)}
      onDrop={onDrop}
    >
      <header>
        <h1>SDA · 空间音频解码器</h1>
        <div className="controls">
          <select
            value="binaural"
            disabled
            title="桌面 WASAPI sidecar 当前仅提供固定虚拟扬声器的双耳 HRTF 输出"
          >
            <option value="binaural">双耳 (耳机 HRTF)</option>
          </select>
          <select value={layoutId} onChange={(e) => changeLayout(e.target.value as LayoutId | "auto")}>
            {!stereoProgram && mode !== "stereo" && <option value="auto">自动{detectedLayout ? `（${detectedLayout}）` : ""}</option>}
            {(Object.keys(LAYOUTS) as LayoutId[])
              .filter((id) => (stereoProgram || mode === "stereo")
                ? id === "2.0" || id === "2.1"
                : id !== "2.0" && id !== "2.1")
              .map((id) => (
                <option key={id} value={id}>
                  {id === "2.1" ? "2.1（低音管理）" : id === "2.0" ? "2.0（立体声）" : `Dolby ${id}`}
                </option>
              ))}
          </select>
          <select
            value={headphoneProfileId ?? ""}
            disabled={mode !== "binaural"}
            title={mode === "binaural" ? "应用经完整性校验的最终双耳 EQ；平均测量档案会明确标注其限制" : "耳机补偿仅用于双耳输出"}
            onChange={(e) => changeHeadphoneCompensation(e.target.value)}
          >
            <option value="">耳机补偿：无</option>
            {headphoneProfiles.map((profile) => (
              <option key={profile.id} value={profile.id}>{profile.name}</option>
            ))}
          </select>
          {window.sdaDesktop?.importHeadphoneProfile && (
            <button disabled={profileBusy} onClick={() => void importHeadphoneProfile()} title="导入经 FIR、SHA-256、测量类别和来源证明验证的 profile.json">
              导入耳机档案
            </button>
          )}
          <button
            onClick={() => setTheme((t) => (t === "dark" ? "light" : "dark"))}
            title={theme === "dark" ? "切换到浅色模式" : "切换到深色模式"}
          >
            {theme === "dark" ? "☀️" : "🌙"}
          </button>
          <button onClick={() => void openFile()}>
            打开文件
          </button>
          {window.sdaDesktop?.pickFolder && (
            <button onClick={() => void openFolder()}>
              添加文件夹
            </button>
          )}
          <button className="settings-toggle" onClick={() => setSettingsOpen((open) => !open)} title="系统设置" aria-expanded={settingsOpen}>
            ⚙
          </button>
        </div>
      </header>

      {settingsOpen && (
        <div className="settings-layer" onMouseDown={() => setSettingsOpen(false)}>
          <section className="settings-panel" aria-label="系统设置" onMouseDown={(event) => event.stopPropagation()}>
            <div className="settings-header">
              <h2>系统设置</h2>
              <button className="settings-close" onClick={() => setSettingsOpen(false)} title="关闭系统设置" aria-label="关闭系统设置">
                ×
              </button>
            </div>
            <fieldset className="settings-group" disabled={mode === "multichannel"}>
              <legend>输出</legend>
              <label className="settings-switch" title="Dolby 对话归一化（dialnorm，ETSI TS 102 366 / ATSC A/52）：把节目对白响度对齐到 -31 LUFS 参考，只衰减过响的节目（dialnorm ≤ 31 故增益恒 ≤ 0 dB），不启用 DRC 动态范围压缩。作用于双耳与立体声输出。无响度元数据的节目（ALAC/立体声 PCM 等）按 BS.1770-4 实测综合响度平衡到 -18 LKFS（杜比 Atmos 音乐交付目标），同样只衰减。">
                <span>音量平衡</span>
                <input
                  type="checkbox"
                  role="switch"
                  checked={volumeBalanceEnabled}
                  onChange={(event) => changeVolumeBalance(event.target.checked)}
                />
              </label>
            </fieldset>
            {window.sdaDesktop?.startNativeRenderer && (
              <fieldset className="settings-group settings-section">
                <legend>原生空间渲染器</legend>
                <label className="settings-switch" title="开启：逐对象 HRTF 卷积。关闭：虚拟音箱总线。两种模式均保留独立对象与 Solo。">
                  <span>逐对象 HRTF（实验）</span>
                  <input type="checkbox" role="switch" checked={directObjectHrtf}
                    disabled={directObjectHrtfBusy || nativeRendererBusy}
                    onChange={(event) => void changeDirectObjectHrtf(event.target.checked)} />
                </label>
                <p className="settings-description">
                  Rust/WASAPI 是桌面版唯一可听输出：对象 PCM 在 native sidecar 中按完整 subject HRTF/BRIR 分区卷积后直接送入 WASAPI。启动前会校验 48 kHz、完整 v4 HRTF 与原子 codec-clock 预缓冲。
                </p>
                <label className="settings-switch" title="启动或停止桌面唯一的 Rust/WASAPI 空间输出。停止后桌面不会退回 Web Audio 输出。">
                  <span>WASAPI 空间输出 <small>{nativeRendererStatus?.running ? nativeRendererStatus.detail : "未启动（无法播放）"}</small></span>
                  <button type="button" disabled={nativeRendererBusy} onClick={() => void toggleNativeRenderer()}>
                    {nativeRendererBusy ? "处理中" : nativeRendererStatus?.running ? "停止" : "启动"}
                  </button>
                </label>
              </fieldset>
            )}
            <fieldset className="settings-group settings-section" disabled={mode !== "binaural"}>
              <legend>耳机 EQ</legend>
              <p className="settings-description">最终双耳输出的三段连续调整，不改变空间渲染或耳机补偿档案。</p>
              <label className="settings-switch" title="将最终双耳低/中/高三段 EQ 全部恢复为 0 dB；不改变 HRTF、LFE 或耳机补偿档案。">
                <span>耳机 EQ 归零</span>
                <button type="button" onClick={resetBinauralEq}>归零</button>
              </label>
              <label className="settings-switch" title="仅用于鼓声 A/B：在最终双耳输出应用左右链接的 180 Hz、-3 dB low shelf。选择会在重启后恢复；不会改动 HRTF、LFE、主声道路由、物理多声道或耳机补偿 FIR。">
                <span>双耳低频诊断 <small>{binauralLowFrequencyDiagnostic === "low-cut" ? "180 Hz / -3 dB" : "参考（旁路）"}</small></span>
                <select
                  value={binauralLowFrequencyDiagnostic}
                  onChange={(event) => changeBinauralLowFrequencyDiagnostic(event.target.value as "reference" | "low-cut")}
                >
                  <option value="reference">参考</option>
                  <option value="low-cut">低频诊断</option>
                </select>
              </label>
              {([
                ["low", "低频", "120 Hz"],
                ["mid", "中频", "1.2 kHz"],
                ["high", "高频", "6 kHz"],
              ] as const).map(([band, label, frequency]) => (
                <label className="eq-band-control" key={band}>
                  <span className="eq-band-label"><b>{label}</b><small>{frequency}</small></span>
                  <input
                    type="range"
                    min="-12"
                    max="12"
                    step="0.5"
                    title="双击重置为 0.0 dB"
                    value={binauralEqBands[band]}
                    onChange={(event) => changeBinauralEqBand(band, Number(event.target.value))}
                    onDoubleClick={() => changeBinauralEqBand(band, 0)}
                  />
                  <output>{binauralEqBands[band] > 0 ? "+" : ""}{binauralEqBands[band].toFixed(1)} dB</output>
                </label>
              ))}
            </fieldset>
            {window.sdaDesktop?.getHeadTrackingStatus && (
              <fieldset className="settings-group settings-section" disabled={mode !== "binaural" || headTrackingBusy}>
                <legend>实验性 AirPods 头部追踪</legend>
                <p className="settings-description">
                  通过独立 helper 进程读取已配对 AirPods 的 motion data；不是 Apple Personalized Spatial Audio，不读取配对密钥。相对姿态流存在时变漂移，静止数秒后声像会缓慢回到最近一次重置的朝向（锚定回正）。
                </p>
                <p className="settings-description">
                  Helper：{headTrackingHelper?.usingBundled ? "内置 Windows helper" : headTrackingHelper?.configured ? `外部 ${headTrackingHelper.fileName}` : "未配置"}
                </p>
                <p className="settings-description">
                  状态：{headTrackingStatus?.running ? `运行中（${headTrackingStatus.source}；${headTrackingStatus.detail}）` : headTrackingStatus?.detail ?? "正在读取"}
                </p>
                <div className="settings-switch">
                  <span>Helper 来源</span>
                  <div>
                    <button onClick={() => void selectHeadTrackingHelper()} title="选择其它 Windows AirPods 头追 helper (.exe)">选择外部</button>
                    {headTrackingHelper?.bundledAvailable && headTrackingHelper.externalSelected && (
                      <button onClick={() => void useBundledHeadTrackingHelper()} title="恢复使用 SDA 随附的 helper">使用内置</button>
                    )}
                  </div>
                </div>
                <div className="settings-switch">
                  <span>追踪控制</span>
                  <button
                    disabled={!headTrackingStatus?.running && !headTrackingHelper?.configured && !headTrackingHelper?.mockAvailable}
                    onClick={() => void setHeadTrackingRunning(!(headTrackingStatus?.running ?? false))}
                  >{headTrackingStatus?.running ? "停止" : "启动"}</button>
                </div>
                <div className="settings-switch">
                  <span>面向前方</span>
                  <button
                    disabled={!headTrackingStatus?.running}
                    onClick={() => void recenterHeadTracking()}
                    title="将当前头部朝向设为前方"
                  >重置</button>
                </div>
                {headTrackingHelper?.mockAvailable && !headTrackingHelper.configured && (
                  <p className="settings-description">开发模式已启用模拟 yaw 追踪；它不访问 AirPods 或蓝牙硬件。</p>
                )}
                <p className="settings-description">请先在 Windows 设置中配对并连接 AirPods；关闭可能独占 motion stream 的其它 AirPods 控制程序。</p>
              </fieldset>
            )}
            {mode === "multichannel" && <p className="settings-disabled">音量平衡仅用于双耳和立体声输出。</p>}
            {mode !== "binaural" && <p className="settings-disabled">切换至双耳输出后可启用耳机 EQ 和头部追踪。</p>}
          </section>
        </div>
      )}

      <main>
        <section className="view">
          <ObjectView objects={objects} layout={outputSpeakers} theme={theme} mutedIds={effectiveMutedIds} soundingIds={soundingObjectIds} focusedSpeakers={activeSpeakerFocus} onSpeakerFocus={speakerFocusLocked ? undefined : toggleSpeakerFocus} hiddenSpeakerNames={effectiveSpeakerMutes} />
          <div className={`view-hint ${track ? "shifted" : ""}`}>拖动旋转 · 右键平移 · 滚轮缩放</div>
          <MiniPlayer
            track={track}
            position={position}
            duration={duration}
            playing={playing}
            paused={paused}
            objectCount={objects.length}
            volume={volume}
            onTogglePlay={togglePlay}
            onReplay={replay}
            onVolume={changeVolume}
          />
        </section>
      </main>

      <div className="float-dock">
        {floatPanel === "head-tracking" && headTrackingStatus?.running && (
          <HeadTrackingTelemetryPanel samples={headTrackingTelemetry} />
        )}
        {floatPanel === "stream" && (
          <div className="panel float-panel">
            <h2>码流</h2>
            {track ? (
              <dl>
                <dt>编码</dt>
                <dd>{track.codec}</dd>
                <dt>采样率</dt>
                <dd>{track.sampleRate} Hz</dd>
                <dt>原始声道</dt>
                <dd>{track.rawBedLabels?.length ? `${track.rawBedLabels.length} 声道 (${track.rawBedLabels.join(", ")})` : "等待首帧"}</dd>
                <dt>解码床层</dt>
                <dd>{track.bedLabels?.length ? `${track.bedLabels.length} 声道 (${track.bedLabels.join(", ")})` : "等待首帧"}</dd>
                <dt>对象 PCM</dt>
                <dd>{track.objectChannels === undefined ? "等待首帧" : `${track.objectChannels} 路动态对象`}</dd>
                <dt>渲染</dt>
                <dd>{mode === "multichannel"
                  ? `虚拟 ${layoutId === "auto" ? detectedLayout ?? "7.1.4" : layoutId} → 耳机 L/R（native HRTF）`
                  : `虚拟 ${layoutId === "auto" ? detectedLayout ?? "7.1.4" : layoutId} → 立体声 L/R`}</dd>
                <dt>容器</dt>
                <dd>{track.container}</dd>
                <dt>响度</dt>
                <dd>{programLoudness
                  ? `对白 ${programLoudness.dialogueLevelDb} LUFS → 目标 ${programLoudness.targetDb} LUFS（${volumeBalanceEnabled ? `平衡中 ${programLoudness.gainDb} dB` : "平衡关闭"}）`
                  : "无响度元数据"}</dd>
                <dt>播放</dt>
                <dd>{position.toFixed(1)} s</dd>
                <dt>PCM 前瞻</dt>
                <dd>{health ? `已喂入 ${health.fedBufferedSeconds.toFixed(2)} s / 排队 ${health.queuedSeconds.toFixed(2)} s` : "等待数据"}</dd>
                <dt>双耳空间图</dt>
                <dd>{health
                  ? `${health.binaural.activeBankCount} 个活动 bank；空间卷积 ${health.binaural.totalSpatialConvolutions}（${health.binaural.banks.map((bank) => `${bank.bank}: ${bank.spatialConvolutions}${bank.directPaths ? `，直通 ${bank.directPaths}` : ""}`).join("；") || "未建图"}）`
                  : "等待图"}</dd>
                <dt>请求输出延迟</dt>
                <dd>{health
                  ? `请求 ${Math.round(health.requestedOutputLatencySeconds * 1000)} ms / 实际 base ${Math.round(health.baseLatencySeconds * 1000)} ms${health.outputLatencySeconds !== null ? ` / output ${Math.round(health.outputLatencySeconds * 1000)} ms` : ""} / ${health.audioContextSampleRate} Hz${health.outputLatencyHintLimited ? "（latency hint 未充分采纳）" : ""}；下次 ${Math.round(health.nextRecommendedOutputLatencySeconds * 1000)} ms`
                  : "等待数据"}</dd>
                <dt>解码实时倍率</dt>
                <dd>{health ? `${health.decodeRealtimeMultiplier.toFixed(2)}×（5 秒滑窗，前瞻填满后会被节流）` : "等待数据"}</dd>
                <dt>Worklet process</dt>
                <dd>{health ? `均值 ${health.tick.processMeanMs.toFixed(3)} ms / 最大 ${health.tick.processMaxMs.toFixed(3)} ms` : "等待 tick"}</dd>
                <dt>回调间隙</dt>
                <dd>{health
                  ? `累计 ${health.callbackGaps} 次；当前 ${health.tick.callbackGaps} 次（>25 ms ${health.tick.callbackGapsOver25Ms}）/ 最大 ${health.tick.callbackGapMaxMs.toFixed(1)} ms；2.5 秒 burst ${health.callbackGapWindowEvents} 次 / ${health.callbackGapWindowTicks} tick；5 秒持续 ${health.callbackGapDistributedEvents} 次 / ${health.callbackGapDistributedTicks} tick；升级 ${health.callbackGapEscalation}`
                  : "等待 tick"}</dd>
                <dt>断供样本</dt>
                <dd>{health
                  ? `累计 ${health.underrunSamples}；当前 ${health.tick.underrunSamples}（拒绝 ${health.tick.rejectedBatches} frame / ${health.tick.rejectedSources} source）`
                  : "等待 tick"}</dd>
                <dt>诊断</dt>
                <dd>{debug || "—"}</dd>
              </dl>
            ) : (
              <p className="dim">拖入 .mkv / .mp4 / .bwf / .wav / .thd / .ec3 / .dts 文件开始</p>
            )}
          </div>
        )}
        {floatPanel === "binaural" && (
          <div className="panel float-panel">
            <h2>双耳元数据</h2>
            <dl>
              <dt>来源</dt>
              <dd>{binauralMetadata?.available ? `BWF dbmd ${binauralMetadata.version ?? ""}` : "当前输入未携带可读取的 Binaural Render Mode"}</dd>
              <dt>模式表</dt>
              <dd>{binauralMetadata?.available ? `${binauralMetadata.modeTable.length} 个未绑定 ordinal（${binauralMetadata.modeTable.join(", ")}）` : "—"}</dd>
              <dt>元素映射</dt>
              <dd>{binauralMetadata?.available
                ? "公开 DBMD supplemental 解析结果未提供 ordinal 到 surround-bed 子声道或 3D object 的身份映射"
                : "—"}</dd>
              {binauralMetadata?.error && <><dt>状态</dt><dd>{binauralMetadata.error}</dd></>}
            </dl>
          </div>
        )}
        {floatPanel === "headphone" && selectedHeadphoneProfile && (
          <div className="panel float-panel">
            <h2>耳机补偿</h2>
            <dl>
              <dt>模式</dt>
              <dd>{selectedHeadphoneProfile.measurementMode === "average-dual-mono" ? "平均测量，L/R 同一曲线" : "独立 L/R 测量"}</dd>
              <dt>来源</dt>
              <dd>{selectedHeadphoneProfile.source}</dd>
              {selectedHeadphoneProfile.channelClaim && <><dt>限制</dt><dd>{selectedHeadphoneProfile.channelClaim}</dd></>}
              {selectedHeadphoneProfile.measurementMode === "average-dual-mono" && <><dt>电平参考</dt><dd>1 kHz 频响参考，不与无补偿响度匹配；A/B 比较请用主音量匹配。</dd></>}
            </dl>
          </div>
        )}
        {floatPanel === "pinna" && (
          <div className="panel float-panel">
            <h2>完整 HRTF 测量</h2>
            <p className="settings-description">
              每个档案使用单一人头或受试者的完整 HRIR/BRIR（头部、耳道、耳廓与房间响应来自同一测量系统）。不再将 KU100 与另一套耳廓做频段拼接。播放中切换实时生效。
            </p>
            <div className="pinna-list">
              {BINAURAL_HEADS.map((head) => (
                <button
                  key={head.id}
                  className={`pinna-option ${binauralHead === head.id ? "active" : ""}`}
                  disabled={denseBinauralBusy}
                  onClick={() => changeBinauralHead(head.id)}
                >
                  <b>{head.label}</b>
                  <small>{head.description}</small>
                  {binauralHead === head.id && <span className="pinna-current">使用中</span>}
                </button>
              ))}
            </div>
            <label className="settings-switch" title={binauralHead === "ku100"
              ? "采用 KU100 的 61 向测量 HRTF。原生播放保留当前扬声器布局，同时用于声床和对象的双耳滤波；独立对象渲染由对象 HRTF 开关控制。"
              : "高解析对象集目前仅有完整 KU100 的 61 向测量。为避免将 KU100 对象 HRTF 混入当前完整 subject HRTF，此模式已关闭。"}>
              <span>高解析 HRTF（仅 KU100）</span>
              <input
                type="checkbox"
                role="switch"
                disabled={binauralHead !== "ku100" || denseBinauralBusy || nativeRendererBusy}
                checked={denseBinauralObjects}
                onChange={(event) => changeDenseBinauralObjects(event.target.checked)}
              />
            </label>
          </div>
        )}
        {floatPanel === "playlist" && (          <div className="panel float-panel playlist-panel" aria-label="播放列表">
            <div className="playlist-head">
              <h2>播放列表 <span>{playlistCurrentId ? `${Math.max(1, playlist.findIndex((item) => item.id === playlistCurrentId) + 1)}/${playlist.length}` : `${playlist.length}`}</span></h2>
              <button disabled={playlist.length === 0} onClick={clearPlaylist}>清空</button>
            </div>
            {playlist.length === 0 ? <p className="dim">打开文件或添加文件夹后，曲目会出现在这里。</p> : (
              <ol className="playlist-items">
                {playlist.map((item) => {
                  const current = item.id === playlistCurrentId;
                  return <li key={item.id} className={current ? "current" : ""} aria-current={current ? "true" : undefined}>
                    <button className="playlist-select" onClick={() => playPlaylistItem(item.id)} title={`播放 ${item.title}`}>
                      <span>{current ? (paused ? "暂停" : "播放") : ""}</span><b>{item.title}</b>
                    </button>
                    <button className="playlist-remove" onClick={() => removePlaylistItem(item.id)} title={`移除 ${item.title}`}>×</button>
                  </li>;
                })}
              </ol>
            )}
          </div>
        )}
        {floatPanel === "channels" && (
          <div className="panel obj-panel float-panel" aria-label="输出音箱声道">
            <div className="obj-head">
              <h2>声道 <span className="obj-count">{layoutId === "auto" ? detectedLayout ?? "7.1.4" : layoutId} · {outputSpeakers.length}</span></h2>
              {activeSpeakerFocus.size > 0 && <button className="obj-clear-solo" onClick={() => setFocusedSpeakers(new Set())}>取消聚焦 ×{activeSpeakerFocus.size}</button>}
              {soloSpeakerNames.size > 0 && <button className="obj-clear-solo" disabled={speakerMixLocked} onClick={() => toggleSpeakerSoloGroup([], true)}>取消独奏 ×{soloSpeakerNames.size}</button>}
            </div>
            <div className="speaker-group-tabs" role="group" aria-label="声道分组">
              {([ ["all", "全部"], ["top", "顶部"], ["front", "前方平面"], ["rear", "后方平面"] ] as const).map(([group, label]) => (
                <button key={group} aria-pressed={speakerGroup === group} onClick={() => setSpeakerGroup(group)}>{label}</button>
              ))}
            </div>
            {filteredSpeakers.some(speaker => speaker.name === "WideLeft") && <div className="speaker-pair-controls">
              <span className="obj-info"><strong>前宽声道 Lw / Rw</strong><small>9.1 新增 · 左右 60°</small></span>
              <span className="obj-ms">
                <button disabled={speakerMixLocked} className={`obj-ms-btn${WIDE_SPEAKERS.every(name => mutedSpeakerNames.has(name)) ? " m-on" : ""}`} aria-label="前宽声道 静音" aria-pressed={WIDE_SPEAKERS.every(name => mutedSpeakerNames.has(name))} title={speakerMixLocked ? "请先取消音箱聚焦" : "同时切换左前宽与右前宽静音"} onClick={() => toggleSpeakerMuteGroup(WIDE_SPEAKERS)}>M</button>
                <button disabled={speakerMixLocked} className={`obj-ms-btn${WIDE_SPEAKERS.every(name => soloSpeakerNames.has(name)) ? " s-on" : ""}`} aria-label="前宽声道 独奏" aria-pressed={WIDE_SPEAKERS.every(name => soloSpeakerNames.has(name))} title={speakerMixLocked ? "请先取消音箱聚焦" : "同时切换左前宽与右前宽独奏"} onClick={event => toggleSpeakerSoloGroup(WIDE_SPEAKERS, event.ctrlKey || event.metaKey)}>S</button>
              </span>
            </div>}
            <ul className="objects">
              {filteredSpeakers.length === 0 && <li className="speaker-group-empty">当前布局无此类声道</li>}
              {filteredSpeakers.map(speaker => {
                const label = speaker.name;
                const muted = mutedSpeakerNames.has(label);
                const soloed = soloSpeakerNames.has(label);
                const silenced = muted || (activeSpeakerSolo && !soloed);
                return <li key={label} className={`obj-row${soloed ? " obj-solo" : ""}${silenced ? " obj-muted" : ""}`}>
                  <span className="obj-info speaker-channel-label" title={label}><strong>{speakerLabel(label)}</strong><small>{speakerPosition(speaker)}</small></span>
                  <span className="obj-ms">
                    <button disabled={speakerMixLocked} className={`obj-ms-btn${muted ? " m-on" : ""}`} aria-label={`${label} 静音`} aria-pressed={muted} title={speakerMixLocked ? "请先取消音箱聚焦" : `${muted ? "取消静音" : "静音"} ${speakerLabel(label)}`} onClick={() => toggleSpeakerMuteGroup([label])}>M</button>
                    <button disabled={speakerMixLocked} className={`obj-ms-btn${soloed ? " s-on" : ""}`} aria-label={`${label} 独奏`} aria-pressed={soloed} title={speakerMixLocked ? "请先取消音箱聚焦" : "独奏此声道（Ctrl/Cmd+点击取消全部独奏）"} onClick={(event) => {
                      toggleSpeakerSoloGroup([label], event.ctrlKey || event.metaKey);
                    }}>S</button>
                  </span>
                </li>;
              })}
            </ul>
          </div>
        )}
        {floatPanel === "objects" && (
          <ObjectPanel
            className="float-panel"
            objects={diagnosticObjects}
            mutedIds={mutedIds}
            soundingIds={soundingObjectIds}
            soloIds={soloIds}
            binauralMetadata={binauralMetadata}
            onToggleMute={toggleMute}
            onToggleSolo={toggleSolo}
          />
        )}
        <div className="float-buttons">
          {headTrackingStatus?.running && (
            <button
              className={`head-tracking-toggle ${floatPanel === "head-tracking" ? "active" : ""}`}
              title="查看头部追踪实时数据"
              aria-label="查看头部追踪实时数据"
              aria-expanded={floatPanel === "head-tracking"}
              onClick={() => setFloatPanel(floatPanel === "head-tracking" ? null : "head-tracking")}
            >头追</button>
          )}
          <button
            className={floatPanel === "stream" ? "active" : ""}
            title="码流信息"
            onClick={() => setFloatPanel(floatPanel === "stream" ? null : "stream")}
          >码流</button>
          <button
            className={floatPanel === "binaural" ? "active" : ""}
            title="双耳元数据"
            onClick={() => setFloatPanel(floatPanel === "binaural" ? null : "binaural")}
          >双耳</button>
          {selectedHeadphoneProfile && (
            <button
              className={floatPanel === "headphone" ? "active" : ""}
              title="耳机补偿详情"
              onClick={() => setFloatPanel(floatPanel === "headphone" ? null : "headphone")}
            >耳机</button>
          )}
          <button
            className={floatPanel === "playlist" ? "active" : ""}
            title={`播放列表 (${playlist.length})`}
            onClick={() => setFloatPanel(floatPanel === "playlist" ? null : "playlist")}
          >列表</button>
          <button
            className={floatPanel === "objects" ? "active" : ""}
            title={`对象 (${diagnosticObjects.length})`}
            onClick={() => setFloatPanel(floatPanel === "objects" ? null : "objects")}
          >对象</button>
          <button
            className={floatPanel === "channels" ? "active" : ""}
            title="声道静音与独奏"
            aria-expanded={floatPanel === "channels"}
            onClick={() => setFloatPanel(floatPanel === "channels" ? null : "channels")}
          >声道</button>
          {mode === "binaural" && (
            <button
              className={floatPanel === "pinna" ? "active" : ""}
              title="选择人头麦/耳廓（HRTF）"
              onClick={() => setFloatPanel(floatPanel === "pinna" ? null : "pinna")}
            >耳廓</button>
          )}
        </div>
      </div>
    </div>
  );
}
