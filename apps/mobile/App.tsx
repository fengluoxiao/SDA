import { primeArtworkColor } from "./src/artwork-color";
import React from "react";
import { Platform, Linking, AppState, type NativeEventSubscription } from "react-native";
import renderingPresets from "./rendering-presets.json";
import { prepare360RaMp4, type MpeghMp4Host } from "./src/mpeghMp4";
import { nextPlaylistItemId, adjacentPlaylistItemId, type PlaybackMode } from "../web/src/playbackOrder";
import * as DocumentPicker from "expo-document-picker";
import { RemotePlayer, type TrackMetadata, type QueueTrack } from "./src/RemotePlayer";
import { type MobileObjectPoint } from "./src/MobileObjectScene";

interface PlaybackStatus {
  binauralEnabled?: boolean; objectRendering?: boolean; listenerYawDegrees?: number;
  outputLayout: "2.0" | "7.1.4" | "360RA-13";
  sourceCodec: string;
  alacUpmixActive: boolean;
  outputChannels: number;
  systemSpatial360RAActive: boolean;
  consumedSamplePos: number;
  decodedSamplePos: number;
  positionMs: number;
  fifoFrames: number;
  pendingBatches: number;
  paused: boolean;
  hrtfReady: boolean;
  hrtfDirections: number;
  directObjectHrtf: boolean;
  directionalHrtf: boolean;
  objectConvolverCount: number;
  continuousObjectCount: number;
  nearFieldEnabled: boolean;
  roomEnabled: boolean;
}
interface ObjectPoint extends MobileObjectPoint { samplePos: number; hasPos: boolean }
interface SdaEngineModule extends MpeghMp4Host {
  addListener?(name: "onMediaCommand", callback: (event: { command: string; hash: string; paused?: boolean }) => void): NativeEventSubscription;
  configurePlaybackQueue?(json: string, mode: string): void;
  currentTrackHash?(): string;
  contentHash(uri: string): Promise<string>;
  preparePlayback?(displayName: string): Promise<void>;
  discardPickerCopy?(uri: string): Promise<boolean>;
  metadata(uri: string): Promise<string>;
  durationMs(uri: string): Promise<number>;
  playUri(uri: string, displayName: string, headYawDegrees: number, contentHash: string): Promise<string>;
  pause(): boolean;
  resume(): boolean;
  stop(): boolean;
  status(): string;
  objects(): string;
  setHeadYaw(degrees: number): void;
  resetHeadPose(): void;
  setHeadTracking?(enabled: boolean): void;
  setBinauralEnabled?(enabled: boolean): void;
  headTrackingStatus?(): string;
  feedError(): string | null;
  feedDone(): boolean;
  setVolume(volume: number): void;
  setVolumeBalance(enabled: boolean): void;
  setSpatialEnhancement?(enabled: boolean): void;
  hrtfStatus(): string;
  renderingSettings(): string;
  set360RaSystemSpatialAudio?(enabled: boolean): boolean;
  setAlacStereoUpmix?(enabled: boolean): boolean;
  setStereoSystemSpatialAudio?(enabled: boolean): boolean;
  setNowPlayingMetadata?(contentHash: string, metadataJson: string): void;
  setObjectRendering(direct: boolean, directional: boolean): void;
  setRenderingPreset(id: string): Promise<void>;
  setSpatialCueDb?(db: number): Promise<void>;
  rooms(): string;
  setRoom(id: string): Promise<void>;
  setNearField(enabled: boolean, metresPerUnit: number): Promise<void>;
}
interface State {
  binauralEnabled: boolean;
  headTrackingEnabled: boolean;
  headTrackingStatus: string;
  alacStereoUpmix: boolean; systemSpatialStereo: boolean;
  sourceCodec: string; alacUpmixActive: boolean; outputChannels: number;
  spatialCueDb: number;
  spatialCueBusy: boolean;
  systemSpatial360RA: boolean;
  systemSpatial360RAActive: boolean;
  layout: "2.0" | "7.1.4" | "360RA-13";
  playbackMode: PlaybackMode;
  queue: QueueTrack[];
  queueIndex: number;
  busy: boolean;
  preparingAudio: boolean;
  playbackPageRequest: number;
  playing: boolean;
  ended: boolean;
  paused: boolean;
  selectedUri: string;
  fileName: string;
  positionMs: number;
  decodedMs: number;
  durationMs: number;
  metadata: TrackMetadata;
  fifoFrames: number;
  objects: ObjectPoint[];
  hrtfStatus: string;
  headYaw: number;
  error: string | null;
  hrtfSet: "standard" | "dense" | "dense-raw";
  hrtfWetWeight: number;
  directObjects: boolean;
  directionalObjects: boolean;
  renderingStatus: string;
  volume: number;
  volumeBalanceEnabled: boolean;
  spatialEnhancementEnabled: boolean;
  rooms: { id: string; name: string; layout: string }[];
  roomId: string;
  roomBusy: boolean;
  nearField: boolean;
  metresPerUnit: number;
  nearFieldBusy: boolean;
}

export default class App extends React.Component<Record<string, never>, State> {
  state: State = {
    binauralEnabled: true,
    headTrackingEnabled: false, headTrackingStatus: "未启用",
    alacStereoUpmix: false, systemSpatialStereo: false, sourceCodec: "", alacUpmixActive: false, outputChannels: 2,
    spatialCueDb: -6, spatialCueBusy: false,
    systemSpatial360RA: false, systemSpatial360RAActive: false,
    layout: "7.1.4",
    playbackMode: "sequence",
    queue: [],
    queueIndex: -1,
    volume: 1,
    volumeBalanceEnabled: false,
    spatialEnhancementEnabled: false,
    rooms: [], roomId: "", roomBusy: false,
    nearField: false, metresPerUnit: 1, nearFieldBusy: false,
    busy: false,
    preparingAudio: false,
    playbackPageRequest: 0,
    playing: false,
    ended: false,
    paused: false,
    selectedUri: "",
    fileName: "",
    positionMs: 0,
    decodedMs: 0,
    durationMs: 0,
    metadata: {},
    fifoFrames: 0,
    objects: [],
    hrtfStatus: "KU100 尚未加载",
    headYaw: 0,
    error: null,
    hrtfSet: "dense",
    hrtfWetWeight: 0,
    directObjects: true,
    directionalObjects: true,
    renderingStatus: "KU100 · 等待播放",
  };
  private lifecycleSubscriptions: NativeEventSubscription[] = [];
  private engine?: SdaEngineModule;
  private artworkPreview = false;
  private changingTrack = false;
  private poller?: ReturnType<typeof setInterval>;

  private foreground = AppState.currentState !== "background" && AppState.currentState !== "inactive";

  private restartStatusPolling() {
    if (this.poller) clearInterval(this.poller);
    // Native audio feeds independently of JS. Background JS only handles
    // end/error while runnable; never poll objects or rebuild hidden UI.
    this.poller = setInterval(() => this.pollStatus(), this.foreground ? (Platform.OS === "ios" ? 125 : 80) : (Platform.OS === "ios" ? 1000 : 250));
  }

  componentDidMount() {
    if (__DEV__ && !(globalThis as any).expo?.modules?.SdaEngine &&
        require("expo-constants").default.executionEnvironment === "storeClient") {
      this.artworkPreview = true;
      this.setState(require("./dev-preview/fixture").createArtworkPreview());
      return;
    }
    const showPlayback = () => this.setState(previous => ({ playbackPageRequest: previous.playbackPageRequest + 1 }));
    this.lifecycleSubscriptions.push(Linking.addEventListener("url", ({ url }) => {
      if (/^(sda|app\.sda\.mobile):\/\/now-playing(?:[/?#]|$)/.test(url)) showPlayback();
    }));
    this.lifecycleSubscriptions.push(AppState.addEventListener("change", state => {
      this.foreground = state === "active";
      if (this.poller) this.restartStatusPolling();
      if (state === "active" && (this.state.playing || this.state.ended)) { showPlayback(); this.pollStatus(); }
    }));
    // UI-only simulator fixture: never start a decoder/audio session. The
    // native smokeStage export returns an empty string in ordinary launches.
    if ((globalThis as any).expo?.modules?.SdaGlassButton?.smokeStage?.() === "mini-player") {
      this.setState({ selectedUri: "ci://mini-player", fileName: "Mini-player", metadata: { title: "正在播放的歌曲", artist: "SDA UI smoke" } });
    }
    try {
      const engine = this.getEngine();
      if (Platform.OS === "android" && engine.addListener) {
        this.lifecycleSubscriptions.push(engine.addListener("onMediaCommand", event => {
          if (event.hash !== this.state.queue[this.state.queueIndex]?.contentHash) return;
          if (event.command === "next") this.skipTrack(1);
          else if (event.command === "previous") this.skipTrack(-1);
          else if (event.command === "state") this.setState({ paused: event.paused === true });
          else if (event.command === "stop") this.setState({ playing: false, paused: false, ended: false, positionMs: 0, objects: [] });
        }));
      }
      const settings = JSON.parse(engine.renderingSettings());
      this.setState({ hrtfSet: settings.hrtfSet === "standard" ? "standard" : settings.hrtfSet === "dense-raw" ? "dense-raw" : "dense",
        binauralEnabled: settings.binauralEnabled !== false,
        headTrackingEnabled: settings.binauralEnabled !== false && settings.headTrackingEnabled === true,
        alacStereoUpmix: settings.alacStereoUpmix === true, systemSpatialStereo: settings.systemSpatialStereo === true,
        spatialCueDb: settings.spatialCueDb ?? -6,
        hrtfWetWeight: settings.hrtfWetWeight ?? 0, directObjects: settings.direct, directionalObjects: settings.directional,
        volumeBalanceEnabled: settings.volumeBalanceEnabled === true,
        spatialEnhancementEnabled: settings.spatialEnhancementEnabled === true,
        systemSpatial360RA: Platform.OS === "ios" && settings.systemSpatial360RA === true,
        roomId: settings.roomId || "", rooms: JSON.parse(this.getEngine().rooms()),
        nearField: settings.nearField === true, metresPerUnit: settings.metresPerUnit ?? 1 });
    } catch (error) {
      this.setState({ error: error instanceof Error ? error.message : String(error) });
    }
  }

  private setBinauralEnabled = (enabled: boolean) => {
    try {
      const engine = this.getEngine();
      if (!engine.setBinauralEnabled) throw new Error("请更新原生模块以切换空间音频");
      engine.setBinauralEnabled(enabled);
      this.setState(previous => ({ binauralEnabled: enabled, headTrackingEnabled: enabled && previous.headTrackingEnabled, headYaw: enabled ? previous.headYaw : 0, error: null }));
    } catch (error) { this.setState({ error: error instanceof Error ? error.message : String(error) }); }
  };

  private setHeadTracking = (enabled: boolean) => {
    if (Platform.OS !== "ios") return;
    try {
      const engine = this.getEngine();
      if (!engine.setHeadTracking) throw new Error("请更新 iOS 原生模块以启用头部追踪");
      if (enabled && !this.state.binauralEnabled) { engine.setBinauralEnabled?.(true); this.setState({ binauralEnabled: true }); }
      engine.setHeadTracking(enabled);
      this.setState({ headTrackingEnabled: enabled, headYaw: 0,
        headTrackingStatus: engine.headTrackingStatus?.() ?? "", error: null });
    } catch (error) { this.setState({ error: error instanceof Error ? error.message : String(error) }); }
  };

  private setSystemSpatial360RA = (enabled: boolean) => {
    if (Platform.OS !== "ios") return;
    try {
      const setter = this.getEngine().set360RaSystemSpatialAudio;
      if (!setter) throw new Error("当前 iOS 原生模块不支持系统空间音频开关");
      setter(enabled);
      this.setState({ systemSpatial360RA: enabled, error: null });
    } catch (error) { this.setState({error: error instanceof Error ? error.message : String(error)}); }
  };

  private setAlacStereoUpmix = (enabled: boolean) => {
    try {
      const setter = this.getEngine().setAlacStereoUpmix;
      if (!setter) throw new Error("请更新原生模块以启用 ALAC 上混");
      setter(enabled); this.setState({ alacStereoUpmix: enabled, error: null });
    } catch (error) { this.setState({ error: error instanceof Error ? error.message : String(error) }); }
  };
  private setSystemSpatialStereo = (enabled: boolean) => {
    if (Platform.OS !== "ios") return;
    try {
      const setter = this.getEngine().setStereoSystemSpatialAudio;
      if (!setter) throw new Error("请更新 iOS 原生模块以启用立体声系统空间音频");
      setter(enabled); this.setState({ systemSpatialStereo: enabled, error: null });
    } catch (error) { this.setState({ error: error instanceof Error ? error.message : String(error) }); }
  };

  private cueUpdatePending = false;
  private setSpatialCueDb = async (db: number) => {
    if (this.changingTrack || this.state.busy || this.cueUpdatePending || ![0, -3, -6, -9, -12].includes(db)) return;
    this.cueUpdatePending = true;
    this.setState({ spatialCueBusy: true, error: null });
    try {
      const engine = this.getEngine();
      if (!engine.setSpatialCueDb) throw new Error("请更新原生模块以调整空间线索");
      await engine.setSpatialCueDb(db);
      this.setState({ spatialCueDb: db });
    } catch (error) { this.setState({ error: error instanceof Error ? error.message : String(error) }); }
    finally { this.cueUpdatePending = false; this.setState({ spatialCueBusy: false }); }
  };

  private setRenderingPreset = async (id: string) => {
    if (this.changingTrack || this.state.busy || this.state.roomBusy || this.state.nearFieldBusy) return;
    if (!renderingPresets.some(profile => profile.id === id)) return;
    this.changingTrack = true;
    this.setState({ busy: true, error: null });
    try {
      const engine = this.getEngine();
      // Replace only the live DSP graph; never stop/reopen or reset playback.
      await engine.setRenderingPreset(id);
      const settings = JSON.parse(engine.renderingSettings());
      this.setState({ hrtfSet: settings.hrtfSet === "standard" ? "standard" : settings.hrtfSet === "dense-raw" ? "dense-raw" : "dense",
        hrtfWetWeight: settings.hrtfWetWeight ?? 0,
        directObjects: settings.direct, directionalObjects: settings.directional,
        nearField: settings.nearField, roomId: settings.roomId,
        hrtfStatus: engine.hrtfStatus() });
    } catch (error) {
      this.setState({ error: error instanceof Error ? error.message : String(error) });
    } finally {
      this.changingTrack = false;
      this.setState({ busy: false });
    }
  };

  private setObjectRendering = (direct: boolean, directional: boolean) => {
    try {
      this.getEngine().setObjectRendering(direct, directional);
      this.setState({ directObjects: direct, directionalObjects: directional, error: null });
    } catch (error) {
      this.setState({ error: error instanceof Error ? error.message : String(error) });
    }
  };

  private setRoom = async (roomId: string) => {
    if (this.state.roomBusy) return;
    this.setState({ roomBusy: true, error: null });
    try {
      await this.getEngine().setRoom(roomId);
      this.setState({ roomId });
    } catch (error) {
      this.setState({ error: error instanceof Error ? error.message : String(error) });
    } finally { this.setState({ roomBusy: false }); }
  };

  private setNearField = async (nearField: boolean, metresPerUnit: number) => {
    if (this.state.nearFieldBusy) return;
    this.setState({ nearFieldBusy: true, error: null });
    try {
      await this.getEngine().setNearField(nearField, metresPerUnit);
      this.setState({ nearField, metresPerUnit });
    } catch (error) {
      this.setState({ error: error instanceof Error ? error.message : String(error) });
    } finally { this.setState({ nearFieldBusy: false }); }
  };

  componentWillUnmount() {
    for (const subscription of this.lifecycleSubscriptions) subscription.remove();
    this.lifecycleSubscriptions = [];
    if (this.poller) clearInterval(this.poller);
  }

  private getEngine(): SdaEngineModule {
    const module = (globalThis as any).expo?.modules?.SdaEngine;
    if (!module) throw new Error("SdaEngine native module is not registered");
    this.engine = module as SdaEngineModule;
    return this.engine;
  }

  private chooseFile = async () => {
    if (this.changingTrack || this.state.busy) return;
    this.changingTrack = true;
    this.setState({ busy: true, error: null });
    try {
      const result = await DocumentPicker.getDocumentAsync({
        type: "*/*",
        copyToCacheDirectory: true,
        multiple: true,
      });
      if (result.canceled) return;
      const knownHashes = new Set(this.state.queue.filter(track => !track.preparation).map(track => track.contentHash));
      const batch = Date.now();
      const pending: QueueTrack[] = result.assets.map((asset, i) => ({
        contentHash: `pending:${batch}:${i}`, uri: asset.uri, name: asset.name, metadata: {},
        preparation: { state: "loading", progress: 0, stage: "等待处理" },
      }));
      await new Promise<void>(resolve => this.setState(state => ({ queue: [...state.queue, ...pending] }), resolve));
      let importError: string | null = null;
      for (const [assetIndex, asset] of result.assets.entries()) {
        const pendingId = pending[assetIndex]!.contentHash;
        const patch = (update: Partial<QueueTrack>) => this.setState(state => ({
          queue: state.queue.map(track => track.contentHash === pendingId ? { ...track, ...update } : track),
        }));
        const progress = (value: number, stage: string) => patch({ preparation: { state: "loading", progress: value, stage } });
        try {
        const extension = asset.name.split(".").pop()?.toLowerCase();
        if (!extension || !["eac3", "ec3", "ac4", "m4a", "mp4", "mp3", "mhas", "av3a"].includes(extension)) {
          throw new Error("请选择 .eac3/.ec3、.ac4、.av3a、.mp3、.mhas，或包含立体声 ALAC、Atmos/360RA/Audio Vivid 音轨的 .m4a/.mp4 文件");
        }
        progress(.02, "校验文件");
        const contentHash = await this.getEngine().contentHash(asset.uri);
        if (knownHashes.has(contentHash)) {
          this.setState(state => {
            const selected = state.queue[state.queueIndex]?.contentHash;
            const queue = state.queue.filter(track => track.contentHash !== pendingId);
            return { queue, queueIndex: selected ? queue.findIndex(track => track.contentHash === selected) : -1 };
          });
          if (Platform.OS === "android") await this.getEngine().discardPickerCopy?.(asset.uri);
          continue;
        }
        progress(.08, "读取歌曲信息");
        const metadata = JSON.parse(await this.getEngine().metadata(asset.uri)) as TrackMetadata;
        if (metadata.coverUri) void primeArtworkColor(metadata.coverUri, metadata.artworkColor).catch(() => {});
        patch({ metadata });
        const track = { contentHash, uri: asset.uri, name: asset.name, metadata };
        // Both platforms finish compressed-track preparation before displaying ready.
        const prepared = await this.prepareTrack(track, p => progress(.12 + p.fraction * .86, p.stage));
        if (Platform.OS === "android") {
          progress(.985, "准备音频引擎");
          await this.getEngine().preparePlayback?.(prepared?.name ?? track.name);
          if (prepared) {
            track.uri = prepared.uri;
            track.metadata = { ...track.metadata, durationMs: prepared.durationMs || track.metadata.durationMs };
            await prepared.adopt?.();
          }
        }
        progress(.99, "完成准备");
        await new Promise<void>(resolve => this.setState(state => {
          const queue = state.queue.map(item => item.contentHash === pendingId ? track : item);
          if (state.queueIndex < 0) return { queue, queueIndex: queue.findIndex(item => item.contentHash === contentHash),
            selectedUri: track.uri, fileName: track.name, metadata: track.metadata, durationMs: track.metadata.durationMs ?? 0 };
          return { queue, queueIndex: state.queueIndex, selectedUri: state.selectedUri, fileName: state.fileName,
            metadata: state.metadata, durationMs: state.durationMs };
        }, () => { this.syncNativeQueue(); resolve(); }));
        knownHashes.add(contentHash);
        } catch (error) {
          importError = error instanceof Error ? error.message : String(error);
          patch({ preparation: { state: "failed", progress: 0, stage: "准备失败", error: importError } });
        }
      }
      if (importError) this.setState({ error: importError });
    } catch (error) {
      this.setState({ error: error instanceof Error ? error.message : String(error) });
    } finally {
      this.changingTrack = false;
      this.setState({ busy: false });
    }
  };

  // Prepared compressed tracks are queue-owned. Android adopts the raw track instead of retaining a second picker copy.
  private preparedTracks = new Map<string, Awaited<ReturnType<typeof prepare360RaMp4>>>();
  private prepareTrack = async (track: QueueTrack, onProgress?: (p: import("./src/mpeghMp4").ImportProgress) => void) => {
    if (!this.preparedTracks.has(track.contentHash)) {
      this.preparedTracks.set(track.contentHash, await prepare360RaMp4(this.getEngine(), track.uri, track.name, onProgress));
    }
    return this.preparedTracks.get(track.contentHash) ?? null;
  };
  private syncNativeQueue = () => {
    this.engine?.configurePlaybackQueue?.(JSON.stringify(this.state.queue.filter(track => !track.preparation).map(track => {
      const prepared = this.preparedTracks.get(track.contentHash);
      return { hash: track.contentHash, uri: prepared?.uri ?? track.uri, name: prepared?.name ?? track.name,
        metadata: { ...track.metadata, durationMs: prepared?.durationMs || track.metadata.durationMs } };
    })), this.state.playbackMode);
  };
  private setPlaybackMode = (playbackMode: PlaybackMode) => this.setState({ playbackMode }, this.syncNativeQueue);

  private skipTrack = (direction: 1 | -1) => {
    const items = this.state.queue.filter(track => !track.preparation).map(track => ({ id: track.contentHash }));
    const currentId = this.state.queue[this.state.queueIndex]?.contentHash ?? null;
    const nextId = adjacentPlaylistItemId(items, currentId, direction);
    if (nextId !== null) void this.selectTrack(this.state.queue.findIndex(track => track.contentHash === nextId));
  };

  private playSelected = () => this.selectTrack(this.state.queueIndex);

  private selectTrack = async (index: number) => {
    const track = this.state.queue[index];
    if (this.changingTrack || this.state.busy || !track || track.preparation) return;
    this.changingTrack = true;
    this.setState({ queueIndex: index, selectedUri: track.uri, fileName: track.name, metadata: track.metadata,
      durationMs: track.metadata.durationMs ?? 0, busy: true, preparingAudio: false, playing: false, error: null, ended: false, paused: false, positionMs: 0, decodedMs: 0, fifoFrames: 0, objects: [] });
    try {
      const engine = this.getEngine();
      if (!this.poller) this.restartStatusPolling();
      this.setState({ hrtfStatus: engine.hrtfStatus() });
      const imported = await this.prepareTrack(track);
      this.syncNativeQueue();
      // playUri owns stop/start atomically; do not stop audio before asynchronous preparation.
      await engine.playUri(imported?.uri ?? track.uri, imported?.name ?? track.name, this.state.headYaw, track.contentHash);
      engine.setNowPlayingMetadata?.(track.contentHash, JSON.stringify({ ...track.metadata, durationMs: imported?.durationMs || track.metadata.durationMs }));
      if (imported?.durationMs) this.setState({ durationMs: imported.durationMs });
      const settings = JSON.parse(engine.renderingSettings());
      this.setState({ volumeBalanceEnabled: settings.volumeBalanceEnabled === true,
          roomId: settings.roomId || "", layout: settings.layout, systemSpatial360RAActive: settings.systemSpatial360RAActive === true });
      // Retain the queue-owned compressed track for repeat/next. Interruptions use resume, never reimport.
      engine.setVolume(this.state.volume);
      const nativePaused = Platform.OS === "android" && JSON.parse(engine.status()).paused === true;
      this.setState({ playing: true, ended: false, paused: nativePaused, error: null });
    } catch (error) {
      this.setState({ playing: false, error: error instanceof Error ? error.message : String(error) });
    } finally {
      this.changingTrack = false;
      this.setState({ busy: false });
    }
  };

  private pollStatus() {
    try {
    if (Platform.OS === "ios" && this.foreground) {
      const detail = this.engine?.headTrackingStatus?.();
      if (detail !== undefined && detail !== this.state.headTrackingStatus) this.setState({ headTrackingStatus: detail });
    }

      const engine = this.engine;
      if (!engine || !this.state.playing || this.state.busy || this.changingTrack) return;
      const feedError = engine.feedError();
      const feedDone = engine.feedDone();
      if (!this.foreground) {
        if (feedDone || feedError) this.setState({ playing: !feedDone, ended: feedDone, error: feedError ?? this.state.error }, () => {
          if (feedDone && !feedError) this.advancePlaylist();
        });
        return;
      }
      const hash = engine.currentTrackHash?.();
      const index = this.state.queue.findIndex(track => track.contentHash === hash);
      if (index >= 0 && index !== this.state.queueIndex) {
        const track = this.state.queue[index]!;
        const prepared = this.preparedTracks.get(track.contentHash);
        this.setState({ queueIndex: index, selectedUri: track.uri, fileName: track.name, metadata: track.metadata,
          durationMs: prepared?.durationMs || track.metadata.durationMs || 0 });
      }
      const value = JSON.parse(engine.status()) as Partial<PlaybackStatus>;
      const objects = JSON.parse(engine.objects()) as Record<string, ObjectPoint>;
      this.setState({
        preparingAudio: (value as Partial<PlaybackStatus> & { preparingAudio?: boolean }).preparingAudio === true,
        positionMs: value.positionMs ?? 0,
        headYaw: this.state.headTrackingEnabled && Number.isFinite(value.listenerYawDegrees) ? value.listenerYawDegrees! : this.state.headYaw,
        decodedMs: ((value.decodedSamplePos ?? 0) * 1000) / 48000,
        fifoFrames: value.fifoFrames ?? 0,
        sourceCodec: value.sourceCodec ?? "", alacUpmixActive: value.alacUpmixActive === true, outputChannels: value.outputChannels ?? 2,
        systemSpatial360RAActive: value.systemSpatial360RAActive === true,
        layout: value.outputLayout ?? this.state.layout,
        objects: feedDone ? [] : Object.values(objects).filter((object) => object.hasPos && object.pos.every(Number.isFinite)),
        paused: value.paused ?? this.state.paused,
        playing: feedDone ? false : this.state.playing,
        ended: feedDone,
        error: feedError ?? this.state.error,
        hrtfStatus: engine.hrtfStatus(),
        renderingStatus: value.systemSpatial360RAActive ? `${value.sourceCodec === "alac" ? `ALAC 立体声 → ${value.alacUpmixActive ? "7.1.4 上混" : "2.0"}` : value.sourceCodec === "av3a" ? "Audio Vivid → 7.1.4" : "360RA → 7.1.4"} · 苹果系统输出 · KU100 已旁路` : value.binauralEnabled === false ? "直接下混 · HRTF 已关闭" : value.objectRendering === false ? "床声道兼容 · HRTF" : feedDone ? "KU100 · 等待播放" : !value.hrtfReady ? "KU100 · 等待引擎加载"
          : `KU100${value.hrtfDirections === 128 ? " 高解析" : ""} · ${value.hrtfDirections} 方向 · ${value.directionalHrtf ? "实际方向" : value.directObjectHrtf || value.nearFieldEnabled ? "逐对象" : "虚拟扬声器"} · 纯直达 · ${value.objectConvolverCount ?? 0} 个独立卷积`,
      }, () => {
        if (feedDone && !feedError) {
          this.advancePlaylist();
        }
      });
    } catch (error) {
      this.setState({ error: error instanceof Error ? error.message : String(error) });
    }
  }

  private advancePlaylist() {
    // Native EOF owns iOS advancement even when JS is suspended. Never advance twice.
    if (Platform.OS === "ios" && this.engine?.configurePlaybackQueue) return;
    const items = this.state.queue.filter(track => !track.preparation).map(track => ({ id: track.contentHash }));
    const currentId = this.state.queue[this.state.queueIndex]?.contentHash ?? null;
    const nextId = nextPlaylistItemId(items, currentId, this.state.playbackMode);
    if (nextId !== null) void this.selectTrack(this.state.queue.findIndex(track => track.contentHash === nextId));
  }

  private togglePause = () => {
    try {
      const engine = this.getEngine();
      const paused = !this.state.paused;
      const applied = paused ? engine.pause() : engine.resume();
      if (applied !== false) this.setState({ paused });
      else this.pollStatus();
    } catch (error) {
      this.setState({ error: error instanceof Error ? error.message : String(error) });
    }
  };

  private adjustYaw = (delta: number) => {
    try {
      const headYaw = Math.max(-180, Math.min(180, this.state.headYaw + delta));
      if (this.state.playing && !this.state.ended) this.getEngine().setHeadYaw(headYaw);
      this.setState({ headYaw });
    } catch (error) {
      this.setState({ error: error instanceof Error ? error.message : String(error) });
    }
  };

  private resetYaw = () => {
    try {
      if (this.state.playing && !this.state.ended) this.getEngine().resetHeadPose();
      this.setState({ headYaw: 0 });
    } catch (error) {
      this.setState({ error: error instanceof Error ? error.message : String(error) });
    }
  };

  private stop = () => {
    try {
      this.engine?.stop();
      this.setState({ sourceCodec: "", alacUpmixActive: false, outputChannels: 2, systemSpatial360RAActive: false, playing: false, ended: false, paused: false, positionMs: 0, decodedMs: 0, fifoFrames: 0, objects: [] });
    } catch (error) {
      this.setState({ error: error instanceof Error ? error.message : String(error) });
    }
  };

  private setSpatialEnhancement = (spatialEnhancementEnabled: boolean) => {
    if (this.changingTrack || this.state.busy || this.state.systemSpatial360RAActive) return;
    try {
      const setter = this.getEngine().setSpatialEnhancement;
      if (!setter) throw new Error("请更新原生模块以启用空间层增强");
      setter(spatialEnhancementEnabled);
      this.setState({ spatialEnhancementEnabled, error: null });
    } catch (error) { this.setState({ error: error instanceof Error ? error.message : String(error) }); }
  };

  private setVolumeBalance = (volumeBalanceEnabled: boolean) => {
    try {
      this.getEngine().setVolumeBalance(volumeBalanceEnabled);
      this.setState({ volumeBalanceEnabled, error: null });
    } catch (error) {
      this.setState({ error: error instanceof Error ? error.message : String(error) });
    }
  };

  private setVolume = (volume: number) => {
    try {
      this.getEngine().setVolume(volume);
      this.setState({ volume });
    } catch (error) {
      this.setState({ error: error instanceof Error ? error.message : String(error) });
    }
  };

  render() {
    const previewProps = __DEV__ && this.artworkPreview ? {
      selectTrack: (index: number) => {
        const track = this.state.queue[index];
        if (track) this.setState({ queueIndex: index, selectedUri: track.uri, fileName: track.name, metadata: track.metadata, positionMs: 0 });
      },
      play: () => this.setState({ playing: true, paused: false }),
      togglePause: () => this.setState(state => ({ playing: true, paused: !state.paused })),
      stop: () => this.setState({ playing: false, paused: true, positionMs: 0 }),
      setVolume: (volume: number) => this.setState({ volume }),
      setBinauralEnabled: (binauralEnabled: boolean) => this.setState(previous => ({ binauralEnabled, headTrackingEnabled: binauralEnabled && previous.headTrackingEnabled })),
      setHeadTracking: (headTrackingEnabled: boolean) => this.setState({ headTrackingEnabled }),
      setRendering: (directObjects: boolean, directionalObjects: boolean) => this.setState({ directObjects, directionalObjects }),
    } : {};
    return <RemotePlayer {...this.state} setHeadTracking={this.setHeadTracking} setBinauralEnabled={this.setBinauralEnabled} setSystemSpatial360RA={this.setSystemSpatial360RA} setAlacStereoUpmix={this.setAlacStereoUpmix} setSystemSpatialStereo={this.setSystemSpatialStereo} chooseFile={this.chooseFile} play={this.playSelected}
      selectTrack={this.selectTrack} previous={() => this.skipTrack(-1)} next={() => this.skipTrack(1)} setPlaybackMode={this.setPlaybackMode}
      togglePause={this.togglePause} stop={this.stop} adjustYaw={this.adjustYaw}
      resetYaw={this.resetYaw} setVolume={this.setVolume} setVolumeBalance={this.setVolumeBalance} setSpatialEnhancement={this.setSpatialEnhancement} setSpatialCueDb={this.setSpatialCueDb} setRenderingPreset={this.setRenderingPreset} setRendering={this.setObjectRendering} setRoom={this.setRoom} setNearField={this.setNearField} {...previewProps} />;
  }
}
