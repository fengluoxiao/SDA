import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { SdaPlayer, type BinauralRenderMetadata, type PlayerHealthSnapshot, type ProgramLoudnessMetadata, type VisualObject } from "@sda/player";
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
} from "@sda/renderer";
// @ts-ignore — plain JS asset served by Vite
import workletUrl from "@sda/renderer/worklet/sda-renderer.worklet.js?url";
import { ObjectView, type Theme } from "./components/ObjectView";
import { MiniPlayer, type TrackInfo } from "./components/MiniPlayer";
import { ObjectPanel } from "./components/ObjectPanel";

type PlaybackSource = { kind: "file"; file: File } | { kind: "path"; path: string };
type HeadTrackingPlayer = {
  setHeadPose?: (pose: HeadPose) => void;
  clearHeadPose?: () => void;
  recenterHeadPose?: () => void;
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

const FILE_CHUNK_SIZE = 1 << 20;
const OUTPUT_LATENCY_STORAGE_KEY = "sda-output-latency-seconds";
type OutputLatencySeconds = 0.1 | 0.2 | 0.3;
const DEFAULT_OUTPUT_LATENCY_SECONDS: OutputLatencySeconds = 0.1;
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
  const [playerReady, setPlayerReady] = useState<SdaPlayer | null>(null);
  const [mode, setMode] = useState<OutputMode>("binaural");
  /** "auto" = 按码流内容自动检测（床标签 + 是否有动态对象）。 */
  const [layoutId, setLayoutId] = useState<LayoutId | "auto">("auto");
  /** 自动模式下首帧检测出的布局（用于界面回显 + 3D 视图）。 */
  const [detectedLayout, setDetectedLayout] = useState<LayoutId | null>(null);
  const [theme, setTheme] = useState<Theme>("dark");
  const [track, setTrack] = useState<TrackInfo | null>(null);
  const [binauralMetadata, setBinauralMetadata] = useState<BinauralRenderMetadata | null>(null);
  const [objects, setObjects] = useState<VisualObject[]>([]);
  const [diagnosticObjects, setDiagnosticObjects] = useState<VisualObject[]>([]);
  const lastDiagnosticUpdateRef = useRef(0);
  /** 被静音的对象 id（Omniphony Studio 语义：mute 独立切换；
   *  solo = mute 其他全部对象，独奏态由"只剩一个未静音"导出）。 */
  const [mutedIds, setMutedIds] = useState<ReadonlySet<number>>(new Set());
  const [soloIds, setSoloIds] = useState<ReadonlySet<number>>(new Set());
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
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [headTrackingStatus, setHeadTrackingStatus] = useState<HeadTrackingStatus | null>(null);
  const [headTrackingBusy, setHeadTrackingBusy] = useState(false);
  const [floatPanel, setFloatPanel] = useState<"stream" | "binaural" | "objects" | null>(null);
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
  /** A monotonically increasing token makes the most recent play request win. */
  const playRequestRef = useRef(0);
  const playRef = useRef<(source: PlaybackSource) => Promise<void>>(async () => {});
  const volumeBalanceRef = useRef(volumeBalanceEnabled);
  const binauralEqBandsRef = useRef(binauralEqBands);
  volumeBalanceRef.current = volumeBalanceEnabled;
  binauralEqBandsRef.current = binauralEqBands;

  const createPlayer = useCallback(
    async (m: OutputMode, lid: LayoutId | "auto", isCurrent: () => boolean) => {
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
        },
        onDecodedFormat: ({ rawBedLabels, bedLabels, objectChannels }) => {
          if (isCurrent()) setTrack((current) => current && { ...current, rawBedLabels, bedLabels, objectChannels });
        },
        onBinauralMetadata: (metadata) => {
          if (isCurrent()) setBinauralMetadata(metadata);
        },
        onVisualState: (objs, t) => {
          if (!isCurrent()) return;
          objectsRef.current = objs;
          setObjects(objs);
          if (t === 0 || t - lastDiagnosticUpdateRef.current >= 0.2) {
            lastDiagnosticUpdateRef.current = t;
            setDiagnosticObjects(objs);
            setProgramLoudness(playerRef.current?.programLoudnessInfo() ?? null);
          }
          setPosition(t);
          const p = playerRef.current;
          setDuration(p?.durationSeconds() ?? 0);
          setDebug(p ? `#${p.id} 已解码 ${p.durationSeconds().toFixed(1)}s / 播放头 ${t.toFixed(1)}s` : "");
        },
        onHealth: (snapshot) => {
          if (isCurrent()) setHealth(snapshot);
        },
        onError: (message) => {
          if (!isCurrent()) return;
          console.warn(`[SDA] ${message}`);
          setErrors((prev) => [...prev.slice(-19), message]);
        },
        onEnded: () => {
          if (isCurrent()) setPlaying(false);
        },
      }, { initialOutputLatencySeconds: outputLatencySecondsRef.current });
      const fallbackLayout = lid === "auto" ? LAYOUTS["7.1.4"] : LAYOUTS[lid];
      const resolver = lid === "auto"
        ? (labels: readonly string[], hasDynamics: boolean) => {
            const id = detectLayoutId(labels, hasDynamics);
            setDetectedLayout(id);
            return LAYOUTS[id];
          }
        : undefined;
      await player.init(m, workletUrl, fallbackLayout, assetUrl("hrtf"), resolver);
      if (!isCurrent()) {
        await player.dispose();
        return null;
      }
      player.setVolumeBalance(volumeBalanceRef.current);
      player.setBinauralEqBands(binauralEqBandsRef.current);
      return player;
    },
    [],
  );

  useEffect(
    () => () => {
      if (coverUrlRef.current) URL.revokeObjectURL(coverUrlRef.current);
      void playerRef.current?.dispose();
      setPlayerReady(null);
    },
    [],
  );

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  useEffect(() => {
    const desktop = window.sdaDesktop;
    if (!desktop?.getHeadTrackingStatus) return;
    void desktop.getHeadTrackingStatus().then(setHeadTrackingStatus).catch((error) => {
      console.warn("[SDA] 读取头部追踪状态失败:", error);
    });
    const stopStatus = desktop.onHeadTrackingStatus?.(setHeadTrackingStatus);
    const applyPose = (pose: HeadTrackingPose) => {
      const player = playerRef.current as (SdaPlayer & HeadTrackingPlayer) | null;
      player?.setHeadPose?.(rendererHeadPose(pose));
    };
    const stopPose = desktop.onHeadTrackingPose?.(applyPose);
    const stopRecenter = desktop.onHeadTrackingRecenter?.((pose) => {
      const player = playerRef.current as (SdaPlayer & HeadTrackingPlayer) | null;
      player?.setHeadPose?.(rendererHeadPose(pose));
      player?.recenterHeadPose?.();
    });
    return () => {
      stopStatus?.();
      stopPose?.();
      stopRecenter?.();
    };
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
      }
    } catch (error) {
      console.warn("[SDA] 头部追踪切换失败:", error);
      setErrors((prev) => [...prev, `头部追踪切换失败: ${String(error)}`]);
    } finally {
      setHeadTrackingBusy(false);
    }
  }, []);

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

  /** 动态对象全部静音时，独立的 LFE 床声道也必须一起静音。 */
  const allObjectsMuted = objects.length > 0 && objects.every((object) => effectiveMutedIds.has(object.id));
  useEffect(() => {
    playerReady?.setLfeMuted(allObjectsMuted);
  }, [playerReady, allObjectsMuted]);

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
      const isCurrent = () => playRequestRef.current === request;
      // A new request invalidates and tears down the audible player immediately.
      // Its async reader may still unwind, but its callbacks are token-gated and it
      // cannot overlap the incoming session while that session initializes.
      const previous = playerRef.current;
      playerRef.current = null;
      setPlayerReady(null);
      if (previous) await previous.dispose();
      if (!isCurrent()) return;
      setErrors([]);
      setTrack(null);
      setBinauralMetadata(null);
      objectsRef.current = [];
      setObjects([]);
      setDiagnosticObjects([]);
      lastDiagnosticUpdateRef.current = 0;
      setPosition(0);
      setDuration(0);
      setHealth(null);
      setDetectedLayout(null);
      setPlaying(true);
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
        const player = await createPlayer(mode, layoutId, isCurrent);
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
        player.setHeadphoneCompensation(headphoneProfileId);
        applyMutes(effectiveMutedIds); // 恢复静音/solo 状态（新播放器默认全不静音）
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
        setErrors((prev) => [...prev, String(e)]);
        setPlaying(false);
      }
    },
    [createPlayer, mode, layoutId, volume, volumeBalanceEnabled, headphoneProfileId, applyMutes, effectiveMutedIds],
  );
  playRef.current = play;

  useEffect(() => window.sdaDesktop?.onOpenFile?.((path) => {
    void playRef.current({ kind: "path", path });
  }), []);

  const onDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      setDragOver(false);
      const file = e.dataTransfer.files[0];
      if (file) void play({ kind: "file", file });
    },
    [play],
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
    setMode(next);
  }, []);

  const changeLayout = useCallback((next: LayoutId | "auto") => {
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

  const selectedHeadphoneProfile = headphoneProfiles.find((profile) => profile.id === headphoneProfileId) ?? null;

  const stopPlayback = useCallback(() => {
    playerRef.current?.stop();
  }, []);

  const replay = useCallback(() => {
    const source = lastSourceRef.current;
    if (source) void play(source);
  }, [play]);

  const openFile = useCallback(async () => {
    const desktop = window.sdaDesktop;
    if (desktop?.pickFile) {
      const path = await desktop.pickFile();
      if (path) void play({ kind: "path", path });
      return;
    }
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".mkv,.mka,.mp4,.m4a,.thd,.mlp,.ec3,.eac3,.ac3,.dts";
    input.onchange = () => {
      const file = input.files?.[0];
      if (file) void play({ kind: "file", file });
    };
    input.click();
  }, [play]);

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
          <select value={mode} onChange={(e) => changeOutputMode(e.target.value as OutputMode)}>
            <option value="binaural">双耳 (耳机 HRTF)</option>
            <option value="stereo">立体声</option>
            <option value="multichannel">多声道</option>
          </select>
          <select value={layoutId} onChange={(e) => changeLayout(e.target.value as LayoutId | "auto")}>
            <option value="auto">自动{detectedLayout ? `（${detectedLayout}）` : ""}</option>
            {(Object.keys(LAYOUTS) as LayoutId[]).map((id) => (
              <option key={id} value={id}>
                Dolby {id}
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
          <button disabled={!playing} onClick={() => playerRef.current?.stop()}>
            停止
          </button>
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
              <label className="settings-switch" title="Dolby 对话归一化（dialnorm，ETSI TS 102 366 / ATSC A/52）：把节目对白响度对齐到 -31 LUFS 参考，只衰减过响的节目（dialnorm ≤ 31 故增益恒 ≤ 0 dB），不启用 DRC 动态范围压缩。作用于双耳与立体声输出；无响度元数据的码流不受影响。">
                <span>音量平衡</span>
                <input
                  type="checkbox"
                  role="switch"
                  checked={volumeBalanceEnabled}
                  onChange={(event) => changeVolumeBalance(event.target.checked)}
                />
              </label>
            </fieldset>
            <fieldset className="settings-group settings-section" disabled={mode !== "binaural"}>
              <legend>耳机 EQ</legend>
              <p className="settings-description">最终双耳输出的三段连续调整，不改变空间渲染或耳机补偿档案。</p>
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
                <legend>实验性头部追踪</legend>
                <p className="settings-description">
                  仅用于验证 SDA 双耳渲染接口。目前使用内置模拟姿态；未来 Windows AirPods 将通过外部 helper 接入，不包含驱动或蓝牙协议实现。
                </p>
                <p className="settings-description">
                  状态：{headTrackingStatus?.running ? `运行中（${headTrackingStatus.source}；${headTrackingStatus.detail}）` : headTrackingStatus?.detail ?? "正在读取"}
                </p>
                <div className="settings-switch">
                  <span>启用模拟追踪</span>
                  <input
                    type="checkbox"
                    role="switch"
                    checked={headTrackingStatus?.running ?? false}
                    onChange={(event) => void setHeadTrackingRunning(event.target.checked)}
                  />
                </div>
                <div className="settings-switch">
                  <span>面向前方</span>
                  <button
                    disabled={!headTrackingStatus?.running}
                    onClick={() => void recenterHeadTracking()}
                    title="将当前实验性姿态设为前方"
                  >重置</button>
                </div>
              </fieldset>
            )}
            {mode === "multichannel" && <p className="settings-disabled">音量平衡仅用于双耳和立体声输出。</p>}
            {mode !== "binaural" && <p className="settings-disabled">切换至双耳输出后可启用耳机 EQ 和头部追踪。</p>}
          </section>
        </div>
      )}

      <main>
        <section className="view">
          <ObjectView objects={objects} layout={LAYOUTS[layoutId === "auto" ? detectedLayout ?? "7.1.4" : layoutId]} theme={theme} mutedIds={effectiveMutedIds} />
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
            onStop={stopPlayback}
            onReplay={replay}
            onVolume={changeVolume}
          />
        </section>
        {selectedHeadphoneProfile && (
        <aside>
          <div className="panel">
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
        </aside>
        )}
      </main>

      <div className="float-dock">
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
                  ? `物理 ${layoutId === "auto" ? detectedLayout ?? "7.1.4" : layoutId} → 系统声卡`
                  : `虚拟 ${layoutId === "auto" ? detectedLayout ?? "7.1.4" : layoutId} → ${mode === "binaural" ? "耳机 L/R" : "立体声 L/R"}`}</dd>
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
                  ? `当前 ${Math.round(health.requestedOutputLatencySeconds * 1000)} ms / 下次 ${Math.round(health.nextRecommendedOutputLatencySeconds * 1000)} ms${health.nextRecommendedOutputLatencySeconds !== health.requestedOutputLatencySeconds ? "（下次生效）" : ""}`
                  : "等待数据"}</dd>
                <dt>解码实时倍率</dt>
                <dd>{health ? `${health.decodeRealtimeMultiplier.toFixed(2)}×（5 秒滑窗，前瞻填满后会被节流）` : "等待数据"}</dd>
                <dt>Worklet process</dt>
                <dd>{health ? `均值 ${health.tick.processMeanMs.toFixed(3)} ms / 最大 ${health.tick.processMaxMs.toFixed(3)} ms` : "等待 tick"}</dd>
                <dt>回调间隙</dt>
                <dd>{health
                  ? `累计 ${health.callbackGaps} 次；当前 ${health.tick.callbackGaps} 次 / 最大 ${health.tick.callbackGapMaxMs.toFixed(1)} ms`
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
        {floatPanel === "objects" && (
          <ObjectPanel
            className="float-panel"
            objects={diagnosticObjects}
            mutedIds={mutedIds}
            soloIds={soloIds}
            binauralMetadata={binauralMetadata}
            onToggleMute={toggleMute}
            onToggleSolo={toggleSolo}
          />
        )}
        <div className="float-buttons">
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
          <button
            className={floatPanel === "objects" ? "active" : ""}
            title={`对象 (${diagnosticObjects.length})`}
            onClick={() => setFloatPanel(floatPanel === "objects" ? null : "objects")}
          >对象</button>
        </div>
      </div>
    </div>
  );
}
