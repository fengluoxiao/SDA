import React from "react";
import { Platform, Linking, AppState, type NativeEventSubscription } from "react-native";
import renderingPresets from "./rendering-presets.json";
import { prepare360RaMp4, type MpeghMp4Host } from "./src/mpeghMp4";
import { nextPlaylistItemId, adjacentPlaylistItemId, type PlaybackMode } from "../web/src/playbackOrder";
import * as DocumentPicker from "expo-document-picker";
import { RemotePlayer, type TrackMetadata, type QueueTrack } from "./src/RemotePlayer";
import { type MobileObjectPoint } from "./src/MobileObjectScene";

interface PlaybackStatus {
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
  contentHash(uri: string): Promise<string>;
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
  feedError(): string | null;
  feedDone(): boolean;
  setVolume(volume: number): void;
  setVolumeBalance(enabled: boolean): void;
  hrtfStatus(): string;
  renderingSettings(): string;
  set360RaSystemSpatialAudio?(enabled: boolean): boolean;
  setNowPlayingMetadata?(contentHash: string, metadataJson: string): void;
  setObjectRendering(direct: boolean, directional: boolean): void;
  setRenderingPreset(id: string): Promise<void>;
  rooms(): string;
  setRoom(id: string): Promise<void>;
  setNearField(enabled: boolean, metresPerUnit: number): Promise<void>;
}
interface State {
  systemSpatial360RA: boolean;
  systemSpatial360RAActive: boolean;
  layout: "7.1.4" | "360RA-13";
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
  rooms: { id: string; name: string; layout: string }[];
  roomId: string;
  roomBusy: boolean;
  nearField: boolean;
  metresPerUnit: number;
  nearFieldBusy: boolean;
}

export default class App extends React.Component<Record<string, never>, State> {
  state: State = {
    systemSpatial360RA: false, systemSpatial360RAActive: false,
    layout: "7.1.4",
    playbackMode: "sequence",
    queue: [],
    queueIndex: -1,
    volume: 1,
    volumeBalanceEnabled: false,
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
  private changingTrack = false;
  private poller?: ReturnType<typeof setInterval>;

  private foreground = AppState.currentState !== "background" && AppState.currentState !== "inactive";

  private restartStatusPolling() {
    if (this.poller) clearInterval(this.poller);
    // Native audio feeds independently of JS. Background JS only handles
    // end/error while runnable; never poll objects or rebuild hidden UI.
    this.poller = setInterval(() => this.pollStatus(), Platform.OS === "ios" ? (this.foreground ? 125 : 1000) : 80);
  }

  componentDidMount() {
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
      const settings = JSON.parse(this.getEngine().renderingSettings());
      this.setState({ hrtfSet: settings.hrtfSet === "standard" ? "standard" : settings.hrtfSet === "dense-raw" ? "dense-raw" : "dense",
        hrtfWetWeight: settings.hrtfWetWeight ?? 0, directObjects: settings.direct, directionalObjects: settings.directional,
        volumeBalanceEnabled: settings.volumeBalanceEnabled === true,
        systemSpatial360RA: Platform.OS === "ios" && settings.systemSpatial360RA === true,
        roomId: settings.roomId || "", rooms: JSON.parse(this.getEngine().rooms()),
        nearField: settings.nearField === true, metresPerUnit: settings.metresPerUnit ?? 1 });
    } catch (error) {
      this.setState({ error: error instanceof Error ? error.message : String(error) });
    }
  }

  private setSystemSpatial360RA = (enabled: boolean) => {
    if (Platform.OS !== "ios") return;
    try {
      const setter = this.getEngine().set360RaSystemSpatialAudio;
      if (!setter) throw new Error("当前 iOS 原生模块不支持系统空间音频开关");
      setter(enabled);
      this.setState({ systemSpatial360RA: enabled, error: null });
    } catch (error) { this.setState({error: error instanceof Error ? error.message : String(error)}); }
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
      const additions: QueueTrack[] = [];
      const knownHashes = new Set(this.state.queue.map(track => track.contentHash));
      for (const asset of result.assets) {
        const extension = asset.name.split(".").pop()?.toLowerCase();
        if (!extension || !["eac3", "ec3", "m4a", "mp4", "mp3", "mhas"].includes(extension)) {
          throw new Error("请选择 .eac3/.ec3、.mp3、.mhas，或包含 Atmos/360RA 音轨的 .m4a/.mp4 文件");
        }
        const contentHash = await this.getEngine().contentHash(asset.uri);
        if (knownHashes.has(contentHash)) continue;
        const metadata = JSON.parse(await this.getEngine().metadata(asset.uri)) as TrackMetadata;
        additions.push({ contentHash, uri: asset.uri, name: asset.name, metadata });
        knownHashes.add(contentHash);
      }
      if (!additions.length) return;
      const queue = [...this.state.queue, ...additions];
      if (this.state.queueIndex < 0) {
        const first = queue[0]!;
        this.setState({ queue, queueIndex: 0, selectedUri: first.uri, fileName: first.name, metadata: first.metadata, durationMs: first.metadata.durationMs ?? 0 });
      } else this.setState({ queue });
    } catch (error) {
      this.setState({ error: error instanceof Error ? error.message : String(error) });
    } finally {
      this.changingTrack = false;
      this.setState({ busy: false });
    }
  };

  private setPlaybackMode = (playbackMode: PlaybackMode) => this.setState({ playbackMode });

  private skipTrack = (direction: 1 | -1) => {
    const items = this.state.queue.map(track => ({ id: track.contentHash }));
    const currentId = this.state.queue[this.state.queueIndex]?.contentHash ?? null;
    const nextId = adjacentPlaylistItemId(items, currentId, direction);
    if (nextId !== null) void this.selectTrack(this.state.queue.findIndex(track => track.contentHash === nextId));
  };

  private playSelected = () => this.selectTrack(this.state.queueIndex);

  private selectTrack = async (index: number) => {
    const track = this.state.queue[index];
    if (this.changingTrack || this.state.busy || !track) return;
    this.changingTrack = true;
    this.setState({ queueIndex: index, selectedUri: track.uri, fileName: track.name, metadata: track.metadata,
      durationMs: track.metadata.durationMs ?? 0, busy: true, preparingAudio: false, playing: false, error: null, ended: false, paused: false, positionMs: 0, decodedMs: 0, fifoFrames: 0, objects: [] });
    try {
      const engine = this.getEngine();
      if (!this.poller) this.restartStatusPolling();
      this.setState({ hrtfStatus: engine.hrtfStatus() });
      engine.stop();
      const imported = await prepare360RaMp4(engine, track.uri, track.name);
      try {
        await engine.playUri(imported?.uri ?? track.uri, imported?.name ?? track.name, this.state.headYaw, track.contentHash);
        engine.setNowPlayingMetadata?.(track.contentHash, JSON.stringify({ ...track.metadata, durationMs: imported?.durationMs || track.metadata.durationMs }));
        if (imported?.durationMs) this.setState({ durationMs: imported.durationMs });
        const settings = JSON.parse(engine.renderingSettings());
        this.setState({ volumeBalanceEnabled: settings.volumeBalanceEnabled === true,
          roomId: settings.roomId || "", layout: settings.layout, systemSpatial360RAActive: settings.systemSpatial360RAActive === true });
      } finally {
        // playUri has opened its InputStream. Android keeps that descriptor valid
        // after unlinking the temporary extraction, including while paused.
        await imported?.release();
      }
      engine.setVolume(this.state.volume);
      this.setState({ playing: true, ended: false, paused: false, error: null });
    } catch (error) {
      this.setState({ playing: false, error: error instanceof Error ? error.message : String(error) });
    } finally {
      this.changingTrack = false;
      this.setState({ busy: false });
    }
  };

  private pollStatus() {
    try {
      const engine = this.engine;
      if (!engine || !this.state.playing || this.state.busy || this.changingTrack) return;
      const feedError = engine.feedError();
      const feedDone = engine.feedDone();
      if (Platform.OS === "ios" && !this.foreground) {
        if (feedDone || feedError) this.setState({ playing: !feedDone, ended: feedDone, error: feedError ?? this.state.error }, () => {
          if (feedDone && !feedError) this.advancePlaylist();
        });
        return;
      }
      const value = JSON.parse(engine.status()) as Partial<PlaybackStatus>;
      const objects = JSON.parse(engine.objects()) as Record<string, ObjectPoint>;
      this.setState({
        preparingAudio: (value as Partial<PlaybackStatus> & { preparingAudio?: boolean }).preparingAudio === true,
        positionMs: value.positionMs ?? 0,
        decodedMs: ((value.decodedSamplePos ?? 0) * 1000) / 48000,
        fifoFrames: value.fifoFrames ?? 0,
        systemSpatial360RAActive: value.systemSpatial360RAActive === true,
        objects: feedDone ? [] : Object.values(objects).filter((object) => object.hasPos && object.pos.every(Number.isFinite)),
        paused: value.paused ?? this.state.paused,
        playing: feedDone ? false : this.state.playing,
        ended: feedDone,
        error: feedError ?? this.state.error,
        hrtfStatus: engine.hrtfStatus(),
        renderingStatus: value.systemSpatial360RAActive ? "360RA → 7.1.4 · 苹果系统输出 · KU100 已旁路" : feedDone ? "KU100 · 等待播放" : !value.hrtfReady ? "KU100 · 等待引擎加载"
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
    const items = this.state.queue.map(track => ({ id: track.contentHash }));
    const currentId = this.state.queue[this.state.queueIndex]?.contentHash ?? null;
    const nextId = nextPlaylistItemId(items, currentId, this.state.playbackMode);
    if (nextId !== null) void this.selectTrack(this.state.queue.findIndex(track => track.contentHash === nextId));
  }

  private togglePause = () => {
    try {
      const engine = this.getEngine();
      const paused = !this.state.paused;
      if (paused) engine.pause(); else engine.resume();
      this.setState({ paused });
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
      this.setState({ systemSpatial360RAActive: false, playing: false, ended: false, paused: false, positionMs: 0, decodedMs: 0, fifoFrames: 0, objects: [] });
    } catch (error) {
      this.setState({ error: error instanceof Error ? error.message : String(error) });
    }
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
    return <RemotePlayer {...this.state} setSystemSpatial360RA={this.setSystemSpatial360RA} chooseFile={this.chooseFile} play={this.playSelected}
      selectTrack={this.selectTrack} previous={() => this.skipTrack(-1)} next={() => this.skipTrack(1)} setPlaybackMode={this.setPlaybackMode}
      togglePause={this.togglePause} stop={this.stop} adjustYaw={this.adjustYaw}
      resetYaw={this.resetYaw} setVolume={this.setVolume} setVolumeBalance={this.setVolumeBalance} setRenderingPreset={this.setRenderingPreset} setRendering={this.setObjectRendering} setRoom={this.setRoom} setNearField={this.setNearField} />;
  }
}
