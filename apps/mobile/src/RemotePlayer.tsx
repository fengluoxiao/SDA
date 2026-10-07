import { followingPlaybackMode, PLAYBACK_MODE_LABELS, type PlaybackMode } from "../../web/src/playbackOrder";
import renderingPresets from "../rendering-presets.json";
import React, { useMemo, useRef, useState } from "react";
import { Animated, Image, Modal, PanResponder, Pressable, SafeAreaView, Platform, ScrollView, StatusBar, StyleSheet, Switch, Text, View, useColorScheme, useWindowDimensions } from "react-native";
import { hasNativeIOSChrome, IOSIconButton, IOSGlassTabs, IOSMaterialSurface, IOSAction, IOSVolumeSlider } from "./IOSNativeChrome";
import { MobileObjectScene, type MobileObjectPoint } from "./MobileObjectScene";
import { IOSPlayer } from "./IOSPlayer";

export interface TrackMetadata {
  title?: string; artist?: string; album?: string; albumArtist?: string;
  year?: string; track?: string; coverUri?: string; durationMs?: number;
}
export interface QueueTrack { contentHash: string; uri: string; name: string; metadata: TrackMetadata }
export interface PlayerProps {
  alacStereoUpmix: boolean; systemSpatialStereo: boolean; sourceCodec: string; alacUpmixActive: boolean; outputChannels: number;
  setAlacStereoUpmix(enabled: boolean): void; setSystemSpatialStereo(enabled: boolean): void;
  spatialCueDb: number; spatialCueBusy: boolean; setSpatialCueDb(db: number): void;
  preparingAudio?: boolean;
  playbackPageRequest?: number;
  systemSpatial360RA: boolean; systemSpatial360RAActive: boolean;
  setSystemSpatial360RA(enabled: boolean): void;
  layout: "2.0" | "7.1.4" | "360RA-13";
  playbackMode: PlaybackMode; setPlaybackMode(mode: PlaybackMode): void;
  queue: QueueTrack[]; queueIndex: number;
  selectTrack(index: number): void; previous(): void; next(): void;
  metadata: TrackMetadata;
  busy: boolean; playing: boolean; paused: boolean; ended: boolean; selectedUri: string;
  fileName: string; positionMs: number; decodedMs: number; durationMs: number; objects: MobileObjectPoint[];
  headYaw: number; error: string | null; directObjects: boolean; directionalObjects: boolean;
  renderingStatus: string; volume: number;
  volumeBalanceEnabled: boolean; setVolumeBalance(enabled: boolean): void;
  chooseFile(): void; play(): void; togglePause(): void; stop(): void;
  adjustYaw(delta: number): void; resetYaw(): void; setVolume(value: number): void;
  hrtfSet: "standard" | "dense" | "dense-raw"; hrtfWetWeight: number; setRenderingPreset(id: string): void;
  setRendering(direct: boolean, directional: boolean): void;
  rooms: { id: string; name: string; layout: string }[]; roomId: string; roomBusy: boolean;
  setRoom(id: string): void;
  nearField: boolean; metresPerUnit: number; nearFieldBusy: boolean;
  setNearField(enabled: boolean, metresPerUnit: number): void;
}
const dark = { bg: "#141619", panel: "#202328", ink: "#f5f6f7", muted: "#a8adb4", line: "#42464d", accent: "#e0e6ed", field: "#2a2e34", soft: "#343b44" };
const light = { bg: "#f3f4f6", panel: "#ffffff", ink: "#19212b", muted: "#687381", line: "#e5e8ed", accent: "#167d72", field: "#f0f2f5", soft: "#e1efeb" };
const time = (ms: number) => `${Math.floor(ms / 60000)}:${String(Math.floor(ms / 1000) % 60).padStart(2, "0")}`;

// Native counterpart of desktop/remote-web: same palette, cards and three-page navigation.
export function RemotePlayer(p: PlayerProps) {
  return Platform.OS === "ios" ? <IOSPlayer {...p} /> : <LegacyRemotePlayer {...p} />;
}

function LegacyRemotePlayer(p: PlayerProps) {
  const { width, height } = useWindowDimensions();
  const [sceneSmoke] = useState(() => (globalThis as any).expo?.modules?.SdaEngine?.sceneSmokeEnabled?.() === true);
  const [chromeSmoke] = useState(() => (globalThis as any).expo?.modules?.SdaGlassButton?.smokeStage?.() || "");
  const [page, setPage] = useState(sceneSmoke ? 2 : chromeSmoke === "library" ? 1 : 0);
  const [settings, setSettings] = useState(chromeSmoke.startsWith("settings"));
  const sheetDrag = useRef(new Animated.Value(0)).current;
  const sheetHeight = useRef(height);
  const showSettings = () => { sheetDrag.setValue(0); setSettings(true); };
  const sheetGesture = useMemo(() => {
    const returnToTop = () => Animated.spring(sheetDrag, { toValue: 0, tension: 90, friction: 14, useNativeDriver: true }).start();
    return PanResponder.create({
      onStartShouldSetPanResponder: () => true,
      onStartShouldSetPanResponderCapture: () => true,
      onMoveShouldSetPanResponder: (_, gesture) => gesture.numberActiveTouches === 1 && gesture.dy > 5 && gesture.dy > Math.abs(gesture.dx),
      onPanResponderGrant: () => sheetDrag.stopAnimation(),
      onPanResponderMove: (_, gesture) => sheetDrag.setValue(Math.max(0, gesture.dy)),
      onPanResponderRelease: (_, gesture) => {
        const threshold = Math.min(96, sheetHeight.current * .18);
        if (gesture.dy >= threshold || (gesture.dy > 16 && gesture.vy > .8)) {
          Animated.timing(sheetDrag, { toValue: height, duration: 180, useNativeDriver: true }).start(({ finished }) => {
            if (finished) setSettings(false);
          });
        } else returnToTop();
      },
      onPanResponderTerminate: returnToTop,
      onPanResponderTerminationRequest: () => false,
    });
  }, [height, sheetDrag]);
  const isLight = useColorScheme() === "light";
  const [volumeWidth, setVolumeWidth] = useState(1);
  const pager = useRef<ScrollView>(null);
  const playerScroll = useRef<ScrollView>(null);
  const sceneScroll = useRef<ScrollView>(null);
  const settingsScroll = useRef<ScrollView>(null);
  // CI-only anchors capture the entire long settings sheet, not just its top.
  const settingsHeading = (text: string, stage: string) => <View onLayout={event => {
    if (chromeSmoke === stage) settingsScroll.current?.scrollTo({ y: event.nativeEvent.layout.y, animated: false });
  }}>{label(text, true, s.groupTitle)}</View>;
  const pagerLayoutWidth = useRef(0);
  const [adjustingVolume, setAdjustingVolume] = useState(false);
  const [interactingScene, setInteractingScene] = useState(false);
  const scrollLocks = useRef({ volume: false, scene: false });
  const sceneInteraction = (active: boolean) => {
    scrollLocks.current.scene = active;
    const enabled = !scrollLocks.current.scene && !scrollLocks.current.volume;
    pager.current?.setNativeProps({ scrollEnabled: enabled });
    playerScroll.current?.setNativeProps({ scrollEnabled: enabled });
    sceneScroll.current?.setNativeProps({ scrollEnabled: enabled });
    setInteractingScene(active);
  };
  const volumeTracking = (locked: boolean) => {
    scrollLocks.current.volume = locked;
    const enabled = !scrollLocks.current.scene && !locked;
    pager.current?.setNativeProps({ scrollEnabled: enabled });
    playerScroll.current?.setNativeProps({ scrollEnabled: enabled });
    sceneScroll.current?.setNativeProps({ scrollEnabled: enabled });
    setAdjustingVolume(locked);
  };
  const volumeGesture = useMemo(() => {
    const lockScrolling = (locked: boolean) => {
      // Block native scrolling immediately, then keep the rendered props in sync.
      scrollLocks.current.volume = locked;
      const enabled = !scrollLocks.current.scene && !scrollLocks.current.volume;
      pager.current?.setNativeProps({ scrollEnabled: enabled });
      playerScroll.current?.setNativeProps({ scrollEnabled: enabled });
      sceneScroll.current?.setNativeProps({ scrollEnabled: enabled });
      setAdjustingVolume(locked);
    };
    let sliderLeft = 0;
    const updateVolume = (x: number) => p.setVolume(Math.max(0, Math.min(1, x / volumeWidth)));
    return PanResponder.create({
      onStartShouldSetPanResponder: () => true,
      onStartShouldSetPanResponderCapture: () => true,
      // A swipe that starts outside the slider remains a page gesture.
      onMoveShouldSetPanResponder: () => false,
      onShouldBlockNativeResponder: () => true,
      onPanResponderTerminationRequest: () => false,
      onPanResponderGrant: event => {
        sliderLeft = event.nativeEvent.pageX - event.nativeEvent.locationX;
        lockScrolling(true);
        updateVolume(event.nativeEvent.pageX - sliderLeft);
      },
      // Keep a fixed screen-space origin even when the finger leaves the view.
      onPanResponderMove: (_, gesture) => updateVolume(gesture.moveX - sliderLeft),
      onPanResponderRelease: () => lockScrolling(false),
      onPanResponderTerminate: () => lockScrolling(false),
    });
  }, [p.setVolume, volumeWidth]);
  const c = Platform.OS === "ios" ? { ...(isLight ? light : dark), bg: isLight ? "#f2f2f7" : "#000000", panel: isLight ? "#ffffff" : "#1c1c1e", ink: isLight ? "#1c1c1e" : "#ffffff", muted: isLight ? "#6c6c70" : "#aeaeb2", accent: "#30b0a6" } : isLight ? light : dark;
  const pageWidth = width - 32;
  const cardHeight = Math.max(440, height - 235);
  const coverSize = Math.min(Math.max(160, width - 104), 460);
  const label = (value: string, muted = false, extra: object = {}) => <Text style={[{ color: muted ? c.muted : c.ink }, extra]}>{value}</Text>;
  const button = (glyph: string, name: string, action: () => void, disabled = false, large = false) => hasNativeIOSChrome ? (
    <IOSIconButton symbol={glyph === "＋" ? "plus" : glyph === "···" ? "ellipsis" : "arrow.counterclockwise"} label={name} onPress={action} disabled={disabled} size={large ? 64 : 46} />
  ) : (
    <Pressable accessibilityRole="button" accessibilityLabel={name} disabled={disabled} onPress={action}
      style={({ pressed }) => [s.circle, { backgroundColor: c.field, borderColor: c.line, opacity: disabled ? .35 : pressed ? .65 : 1 }, large && s.play]}>
      {label(glyph, false, { fontSize: large ? 27 : 22 })}
    </Pressable>
  );
  React.useEffect(() => {
    if (p.playbackPageRequest) { setPage(0); pager.current?.scrollTo({ x: 0, animated: false }); setSettings(false); }
  }, [p.playbackPageRequest]);
  const [sceneVisited, setSceneVisited] = useState(sceneSmoke);
  React.useEffect(() => { if (page === 2) setSceneVisited(true); }, [page]);
  const navigate = (index: number) => { setPage(index); pager.current?.scrollTo({ x: index * pageWidth, animated: true }); };
  const progress = p.durationMs > 0 ? Math.max(0, Math.min(1, p.positionMs / p.durationMs)) : 0;
  const title = p.metadata.title || p.fileName || "等待选择歌曲";
  const artist = p.metadata.artist || p.metadata.albumArtist;
  const playback = p.busy ? "正在准备音频…" : p.playing ? p.paused ? "已暂停" : "正在播放" : p.ended ? "播放结束" : p.selectedUri ? "准备就绪" : "选择文件，开始聆听";
  const SafeContainer = Platform.OS === "ios" ? SafeAreaView : View;
  return <SafeContainer style={{ flex: 1, backgroundColor: c.bg }}><View style={[s.root, { backgroundColor: c.bg }]}>
    {!isLight && p.metadata.coverUri && <View pointerEvents="none" style={StyleSheet.absoluteFill}>
      <Image source={{ uri: p.metadata.coverUri }} blurRadius={65} style={[StyleSheet.absoluteFill, { opacity: .2 }]} />
      <View style={[StyleSheet.absoluteFill, { backgroundColor: "#10131888" }]} />
    </View>}
    <StatusBar barStyle={isLight ? "dark-content" : "light-content"} backgroundColor={c.bg} />
    <View style={s.header}>
      {button("＋", "打开本机媒体", p.chooseFile, p.busy)}
      <View style={{ flex: 1, alignItems: "center" }}>{label(page === 0 ? (p.busy || p.preparingAudio ? "正在准备音频…" : "正在播放") : page === 1 ? "音乐资料库" : "空间音频", true, s.eyebrow)}
        <Text numberOfLines={1} style={{ color: c.ink, fontSize: 12, marginTop: 5 }}>{p.metadata.album || "SDA · 本地音乐"}</Text>
      </View>
      {button("···", "更多设置", showSettings)}
    </View>
    <ScrollView ref={pager} horizontal pagingEnabled snapToInterval={pageWidth} snapToAlignment="start"
      decelerationRate="fast" disableIntervalMomentum bounces={false} overScrollMode="never"
      scrollEnabled={!adjustingVolume && !interactingScene} showsHorizontalScrollIndicator={false}
      onMomentumScrollEnd={event => setPage(Math.max(0, Math.min(2, Math.round(event.nativeEvent.contentOffset.x / pageWidth))))}
      onLayout={event => {
        const layoutWidth = event.nativeEvent.layout.width;
        if (pagerLayoutWidth.current === layoutWidth) return;
        pagerLayoutWidth.current = layoutWidth;
        pager.current?.scrollTo({ x: page * pageWidth, animated: false });
      }}>
      <ScrollView ref={playerScroll} scrollEnabled={!adjustingVolume && !interactingScene} style={{ width: pageWidth }} contentContainerStyle={{ paddingBottom: 6 }}>
        <View style={[s.player, { width: coverSize, alignSelf: "center" }]}>
          <View style={[s.art, { height: coverSize, width: coverSize, alignSelf: "center", backgroundColor: c.panel }]}>
            {p.metadata.coverUri ? <Image source={{ uri: p.metadata.coverUri }} accessibilityLabel={`${p.metadata.album || title} 封面`}
              resizeMode="cover" style={{ width: coverSize, height: coverSize, borderRadius: 12 }} /> : <View style={[s.globe, { borderColor: c.accent }]}>
              <View style={[s.longitude, { borderColor: c.accent }]} /><View style={[s.latitude, { borderColor: c.accent }]} />
              <View style={[s.equator, { backgroundColor: c.accent }]} />
            </View>}
          </View>
          <View style={s.trackInfo}>
            <Text numberOfLines={2} style={[s.trackTitle, { color: c.ink }]}>{title}</Text>
            <Text numberOfLines={1} style={[s.trackSubtitle, { color: c.muted }]}>
              {artist || (p.selectedUri ? "未知艺人" : "打开音频，开始聆听")}
            </Text>
          </View>
          <View accessibilityRole="progressbar" accessibilityLabel="播放进度"
            accessibilityValue={p.durationMs > 0 ? { min: 0, max: p.durationMs, now: Math.min(p.positionMs, p.durationMs), text: `${time(p.positionMs)} / ${time(p.durationMs)}` } : { text: "总时长未知" }}
            style={[s.progress, { backgroundColor: c.line }]}>
            <View style={{ height: 5, borderRadius: 8, width: `${progress * 100}%`, backgroundColor: c.accent }} />
          </View>
          <View style={[s.row, { marginTop: 9, marginBottom: 12 }]}>{label(time(p.positionMs), true, s.small)}{label(p.durationMs > 0 ? time(p.durationMs) : "--:--", true, s.small)}</View>
          <View style={[s.transport, { gap: Math.max(0, Math.min(44, (coverSize - 244) / 2)) }]}>
            {hasNativeIOSChrome ? <IOSIconButton symbol="backward.end.fill" label="上一曲" disabled={p.busy || !p.queue.length} onPress={p.previous} /> : <Pressable accessibilityRole="button" accessibilityLabel="上一曲" disabled={p.busy || !p.queue.length} onPress={p.previous} style={[s.headerAction, { opacity: p.busy || !p.queue.length ? .3 : 1 }]}>
              <View style={{ flexDirection: "row", alignItems: "center", gap: 3 }}><View style={{ width: 3, height: 23, borderRadius: 1, backgroundColor: c.ink }} /><View style={{ width: 0, height: 0, borderTopWidth: 12, borderBottomWidth: 12, borderRightWidth: 19, borderTopColor: "transparent", borderBottomColor: "transparent", borderRightColor: c.ink }} /></View>
            </Pressable>}
            {hasNativeIOSChrome ? <IOSIconButton symbol={p.playing && !p.paused ? "pause.fill" : "play.fill"} label={p.playing && !p.paused ? "暂停" : "播放"} disabled={p.busy || !p.selectedUri} onPress={p.playing ? p.togglePause : p.play} prominent size={64} symbolSize={25} /> : <Pressable accessibilityRole="button" accessibilityLabel={p.playing && !p.paused ? "暂停" : "播放"}
              disabled={p.busy || !p.selectedUri} onPress={p.playing ? p.togglePause : p.play}
              style={({ pressed }) => [s.mainPlay, { backgroundColor: c.ink, opacity: p.busy || !p.selectedUri ? .35 : pressed ? .7 : 1 }]}>
              {p.playing && !p.paused ? <View style={{ flexDirection: "row", gap: 6 }}><View style={[s.pauseBar, { backgroundColor: c.bg }]} /><View style={[s.pauseBar, { backgroundColor: c.bg }]} /></View>
                : <View style={{ marginLeft: 5, width: 0, height: 0, borderTopWidth: 12, borderBottomWidth: 12, borderLeftWidth: 20, borderTopColor: "transparent", borderBottomColor: "transparent", borderLeftColor: c.bg }} />}
            </Pressable>}
            {hasNativeIOSChrome ? <IOSIconButton symbol="forward.end.fill" label="下一曲" disabled={p.busy || !p.queue.length} onPress={p.next} /> : <Pressable accessibilityRole="button" accessibilityLabel="下一曲" disabled={p.busy || !p.queue.length} onPress={p.next} style={[s.headerAction, { opacity: p.busy || !p.queue.length ? .3 : 1 }]}>
              <View style={{ flexDirection: "row", alignItems: "center", gap: 3 }}><View style={{ width: 0, height: 0, borderTopWidth: 12, borderBottomWidth: 12, borderLeftWidth: 19, borderTopColor: "transparent", borderBottomColor: "transparent", borderLeftColor: c.ink }} /><View style={{ width: 3, height: 23, borderRadius: 1, backgroundColor: c.ink }} /></View>
            </Pressable>}
            {hasNativeIOSChrome ? <IOSAction symbol={p.playbackMode === "repeat-one" ? "repeat.1" : p.playbackMode === "sequence" ? "list.bullet" : "repeat"} title="" label={`播放模式：${PLAYBACK_MODE_LABELS[p.playbackMode]}`} onPress={() => {}} choices={["sequence", "repeat-one", "repeat-all"].map(mode => PLAYBACK_MODE_LABELS[mode as PlaybackMode])} choiceIndex={["sequence", "repeat-one", "repeat-all"].indexOf(p.playbackMode)} onChoice={index => { const mode = (["sequence", "repeat-one", "repeat-all"] as PlaybackMode[])[index]; if (mode) p.setPlaybackMode(mode); }} style={{ width: 44, height: 44 }} /> : (            <Pressable accessibilityRole="button" accessibilityLabel={`播放模式：${PLAYBACK_MODE_LABELS[p.playbackMode]}，点击切换为${PLAYBACK_MODE_LABELS[followingPlaybackMode(p.playbackMode)]}`}
              onPress={() => p.setPlaybackMode(followingPlaybackMode(p.playbackMode))}
              style={({ pressed }) => ({ position: hasNativeIOSChrome ? "relative" : "absolute", right: 0, width: 44, height: 44, justifyContent: "center", alignItems: "center", borderRadius: 22, backgroundColor: p.playbackMode === "sequence" ? "transparent" : c.soft, opacity: pressed ? .65 : 1 })}>
              {label(p.playbackMode === "sequence" ? "≡" : p.playbackMode === "repeat-one" ? "↻₁" : "↻", p.playbackMode === "sequence", { fontSize: 20 })}
            </Pressable>)}

          </View>
          <View style={[s.row, { gap: 12, marginTop: 8 }]}>
            {hasNativeIOSChrome ? <IOSVolumeSlider value={p.volume} onChange={p.setVolume} onTracking={volumeTracking} /> : <>
            <View accessibilityRole="image" accessibilityLabel="音量" style={{ width: 24, height: 24 }}>
              <View style={{ position: "absolute", left: 2, top: 9, width: 5, height: 7, borderRadius: 1, backgroundColor: c.muted }} />
              <View style={{ position: "absolute", left: 6, top: 5, width: 0, height: 0, borderTopWidth: 7, borderBottomWidth: 7, borderRightWidth: 8, borderTopColor: "transparent", borderBottomColor: "transparent", borderRightColor: c.muted }} />
              <View style={{ position: "absolute", left: 15, top: 8, width: 4, height: 9, borderRightWidth: 1.5, borderRightColor: c.muted, borderRadius: 6 }} />
              <View style={{ position: "absolute", left: 18, top: 5, width: 5, height: 15, borderRightWidth: 1.5, borderRightColor: c.muted, borderRadius: 8 }} />
            </View>
            <View accessibilityRole="adjustable" accessibilityLabel="音量" accessibilityValue={{ min: 0, max: 100, now: Math.round(p.volume * 100) }}
              accessibilityActions={[{ name: "increment" }, { name: "decrement" }]}
              onAccessibilityAction={event => p.setVolume(Math.max(0, Math.min(1, p.volume + (event.nativeEvent.actionName === "increment" ? .05 : -.05))))}
              onLayout={event => setVolumeWidth(event.nativeEvent.layout.width)}
              {...volumeGesture.panHandlers} style={s.volumeTouch}>
              <View pointerEvents="none" style={[s.volumeTrack, { backgroundColor: c.line }]}><View style={{ width: `${p.volume * 100}%`, height: 5, borderRadius: 8, backgroundColor: c.accent }} /></View>
            </View></>}{label(`${Math.round(p.volume * 100)}%`, true, { ...s.small, width: 35 })}
          </View>
        </View>
      </ScrollView>
      <ScrollView style={{ width: pageWidth }} contentContainerStyle={{ paddingBottom: 6 }}>
        <View style={[s.card, hasNativeIOSChrome && { borderWidth: 0, borderRadius: 22, padding: 18 }, { minHeight: cardHeight, backgroundColor: c.panel, borderColor: c.line }]}>
          <View style={s.row}>{label("播放列表", false, s.sectionTitle)}{label(`${p.queue.length} 首`, true, s.small)}</View>
          {hasNativeIOSChrome ? p.queue.map((track, index) => <IOSAction key={`${track.uri}-${index}`} row selected={index === p.queueIndex} symbol={index === p.queueIndex && p.playing && !p.paused ? "waveform" : "music.note"} title={track.metadata.title || track.name} subtitle={track.metadata.artist || track.metadata.albumArtist || "未知艺人"} label={`播放 ${track.metadata.title || track.name}`} disabled={p.busy} onPress={() => index === p.queueIndex && p.playing ? p.togglePause() : p.selectTrack(index)} style={{ marginTop: 8 }} />) : <>
          {p.queue.map((track, index) => <Pressable key={`${track.uri}-${index}`} accessibilityRole="button" accessibilityLabel={`播放 ${track.metadata.title || track.name}`} accessibilityState={{ selected: index === p.queueIndex }} onPress={() => index === p.queueIndex && p.playing ? p.togglePause() : p.selectTrack(index)} disabled={p.busy} style={[s.queueItem, { backgroundColor: index === p.queueIndex ? c.soft : "transparent" }]}>
            {label(String(index + 1).padStart(2, "0"), true, s.small)}<View style={{ flex: 1 }}><Text numberOfLines={2} style={{ color: c.ink }}>{track.metadata.title || track.name}</Text>{!!(track.metadata.artist || track.metadata.albumArtist) && label(track.metadata.artist || track.metadata.albumArtist || "", true, { ...s.small, marginTop: 5 })}</View>{label(index === p.queueIndex && p.playing && !p.paused ? "Ⅱ" : "▶")}
          </Pressable>)}
          </>}
          {!p.queue.length && label("还没有添加歌曲。", true, { marginVertical: 28 })}
          {hasNativeIOSChrome ? <IOSAction symbol="folder.badge.plus" title="打开本机媒体" onPress={p.chooseFile} disabled={p.busy} style={{ marginTop: 20 }} /> : (          <Pressable accessibilityRole="button" onPress={p.chooseFile} disabled={p.busy} style={[s.choose, { borderColor: c.line }]}>{label("＋  打开本机媒体", false, { color: c.accent })}</Pressable>) }
          {label(`可多选文件加入列表。当前：${PLAYBACK_MODE_LABELS[p.playbackMode]}。`, true, s.help)}
        </View>
      </ScrollView>
      <ScrollView ref={sceneScroll} scrollEnabled={!adjustingVolume && !interactingScene} style={{ width: pageWidth }} contentContainerStyle={{ paddingBottom: 6 }}>
        <View style={[s.card, hasNativeIOSChrome && { borderWidth: 0, borderRadius: 22, padding: 18 }, { minHeight: cardHeight, backgroundColor: c.panel, borderColor: c.line }]}>
          <View style={s.row}>{label(p.layout === "360RA-13" ? "360° 球形声场" : "空间视图", false, s.sectionTitle)}{label(`${p.objects.length} 个对象`, true, s.small)}</View>
          {label(`${p.layout} · 对象实时位置`, true, { ...s.small, marginTop: 12 })}
          <View style={[s.scene, { height: Math.max(260, height * .40) }]}>{sceneVisited && <MobileObjectScene layout={p.layout} objects={p.objects} active={page === 2} onInteractionChange={sceneInteraction} />}</View>
          {label("单指旋转 · 双指缩放", true, s.help)}
        </View>
      </ScrollView>
    </ScrollView>
    {hasNativeIOSChrome ? <View style={{ paddingTop: 12 }}><IOSGlassTabs selected={page} onChange={navigate} /></View> : <View style={s.tabs}>{["播放", "列表", "空间"].map((name, index) => <Pressable key={name} accessibilityRole="tab" accessibilityState={{ selected: page === index }} onPress={() => navigate(index)} style={s.tab}>
      <View style={{ paddingHorizontal: 20, paddingVertical: 10, borderRadius: 20, backgroundColor: page === index ? c.field : "transparent" }}>{label(name, page !== index, { fontSize: 12, fontWeight: page === index ? "600" : "400" })}</View></Pressable>)}</View>}
    {p.error && <Text accessibilityRole="alert" style={s.error}>{p.error}</Text>}
    <Modal transparent visible={settings} animationType="slide" onRequestClose={() => setSettings(false)}>
      <StatusBar barStyle={isLight ? "dark-content" : "light-content"} backgroundColor={c.bg} />
      <Animated.View style={[s.backdrop, { opacity: sheetDrag.interpolate({ inputRange: [0, height], outputRange: [1, 0], extrapolate: "clamp" }) }]}>
        <Pressable accessibilityRole="button" accessibilityLabel="关闭设置" style={StyleSheet.absoluteFill} onPress={() => setSettings(false)} />
      </Animated.View>
      <Animated.View onLayout={event => { sheetHeight.current = event.nativeEvent.layout.height; }} style={[s.sheet, { backgroundColor: hasNativeIOSChrome ? "transparent" : c.bg, maxHeight: height - 64, transform: [{ translateY: sheetDrag }] }]}>
        {hasNativeIOSChrome && <IOSMaterialSurface style={StyleSheet.absoluteFill} />}
        <View collapsable={false} {...sheetGesture.panHandlers} accessible accessibilityLabel="播放设置，向下滑动关闭" accessibilityActions={[{ name: "dismiss", label: "关闭设置" }]} onAccessibilityAction={event => { if (event.nativeEvent.actionName === "dismiss") setSettings(false); }}>
        <View style={[s.sheetHandle, { backgroundColor: c.line }]} />
        <View style={[s.sheetHeader, hasNativeIOSChrome && { paddingRight: 74 }]}>
          <View style={{ flex: 1 }}>{label("播放设置", false, s.sheetTitle)}{label("调整你的空间聆听体验", true, s.sheetSubtitle)}</View>
        </View>
        </View>
        {hasNativeIOSChrome && <View style={{ position: "absolute", right: 18, top: 22 }}><IOSIconButton symbol="xmark" label="关闭设置" onPress={() => setSettings(false)} size={40} symbolSize={16} /></View>}
        <ScrollView ref={settingsScroll} showsVerticalScrollIndicator={false} contentContainerStyle={s.sheetContent}>
          <View style={s.quickActions}>
            {[{ name: "重新播放", action: p.play, disabled: p.busy || !p.selectedUri }, { name: "停止播放", action: p.stop, disabled: p.busy || !p.playing }].map(action =>
              hasNativeIOSChrome ? <IOSAction key={action.name} symbol={action.name === "重新播放" ? "arrow.counterclockwise" : "stop.fill"} title={action.name} disabled={action.disabled} onPress={() => { setSettings(false); action.action(); }} style={{ flex: 1 }} /> : <Pressable key={action.name} accessibilityRole="button" disabled={action.disabled} onPress={() => { setSettings(false); action.action(); }}
                style={({ pressed }) => [s.quickAction, { backgroundColor: c.panel, opacity: action.disabled ? .35 : pressed ? .65 : 1 }]}>{label(action.name, false, s.settingTitle)}</Pressable>)}
          </View>
          {hasNativeIOSChrome ? <IOSAction symbol="folder.badge.plus" title="打开本机媒体" subtitle="添加歌曲到播放列表" disabled={p.busy} onPress={() => { setSettings(false); p.chooseFile(); }} /> : (<Pressable accessibilityRole="button" disabled={p.busy} onPress={() => { setSettings(false); p.chooseFile(); }} style={[s.settingsCard, s.settingRow, { backgroundColor: c.panel }]}>
            <View style={s.settingCopy}>{label("打开本机媒体", false, s.settingTitle)}{label("添加歌曲到播放列表", true, s.settingDescription)}</View>{label("›", true, { fontSize: 26 })}
          </Pressable>)}
          {label("音量", true, s.groupTitle)}
          <View style={[s.settingsCard, { backgroundColor: c.panel }]}>
            <View style={s.settingRow}><View style={s.settingCopy}>{label("音量平衡", false, s.settingTitle)}{label(p.systemSpatial360RAActive ? "按 7.1.4 PCM 测量响度，12 声道统一衰减，不改变声场；起播前预读测量，避免开头先响后轻" : "与 Windows 相同：双声道 / 360RA 响度平衡，只衰减不增益", true, s.settingDescription)}</View><Switch accessibilityLabel="音量平衡" trackColor={{ false: c.line, true: "#167d72" }} thumbColor="#ffffff" value={p.volumeBalanceEnabled} disabled={p.busy} onValueChange={p.setVolumeBalance} /></View>
          </View>
          {Platform.OS === "ios" && <>
            {label("360 Reality Audio", true, s.groupTitle)}
            <View style={[s.settingsCard, { backgroundColor: c.panel }]}>
              <View style={s.settingRow}><View style={s.settingCopy}>{label("系统空间音频 · 7.1.4", false, s.settingTitle)}{label("仅 360RA：渲染成 12 声道交给苹果系统，旁路 KU100 直达渲染；支持统一音量平衡，不改变声道方向。关闭后恢复 SDA／KU100 双耳空间渲染，并非普通立体声下混。", true, s.settingDescription)}</View><Switch accessibilityLabel="360RA 系统空间音频 7.1.4" value={p.systemSpatial360RA} disabled={p.busy} onValueChange={p.setSystemSpatial360RA} trackColor={{false:c.line,true:"#167d72"}} thumbColor="#ffffff" /></View>
              {label("修改后下一次播放生效，不中断当前歌曲。实际空间化／头部跟踪由兼容耳机及系统设置决定。", true, s.groupHint)}
              {label(p.systemSpatial360RAActive ? p.systemSpatial360RA ? "当前：系统 7.1.4 输出（SDA 空间选项已旁路）" : "当前仍是系统 7.1.4；下次播放恢复 360RA-13／KU100 直达渲染" : "当前：SDA 渲染／等待播放", true, s.groupHint)}
            </View>
          </>}
          {Platform.OS === "ios" && <View style={[s.settingsCard, { backgroundColor: c.panel }]}>
            {label("空间线索强度", false, s.settingTitle)}
            {[0, -3, -6, -9, -12].map(db => <Pressable key={db} accessibilityRole="radio" accessibilityState={{ checked: p.spatialCueDb === db, disabled: p.spatialCueBusy || p.busy }} disabled={p.spatialCueBusy || p.busy} onPress={() => p.setSpatialCueDb(db)} style={s.settingRow}>
              {label(`${p.spatialCueDb === db ? "●" : "○"} ${db} dB${db === -6 ? " · 默认" : ""}`, false, s.settingTitle)}
            </Pressable>)}
            {label("播放中平滑切换，不暂停或重新播放；缓冲音频播放完后生效。系统空间音频旁路时仅保存设置。", true, s.groupHint)}
          </View>}
          {settingsHeading("空间渲染", "settings-spatial")}
          <View style={[s.settingsCard, { backgroundColor: c.panel }]}>
            <View style={s.profileHeader}><View style={s.settingCopy}>{label("KU100 双耳音频", false, s.profileTitle)}{label("128 方向 HRIR · 对象与声道纯直达", true, s.settingDescription)}</View><View style={[s.profileBadge, { backgroundColor: c.soft }]}>{label("耳廓", false, { fontSize: 11 })}</View></View>
            <View style={[s.cardDivider, { backgroundColor: c.line }]} />
            {renderingPresets.map(profile => {
              const selected = p.hrtfSet === profile.hrtfSet && p.directObjects === profile.direct && p.directionalObjects === profile.directional && p.nearField === profile.nearField && p.roomId === profile.roomId && Math.abs(p.hrtfWetWeight - profile.hrtfWetWeight) < 1e-6;
              const disabled = p.systemSpatial360RAActive || p.busy || p.roomBusy || p.nearFieldBusy;
              return hasNativeIOSChrome ? <IOSAction key={profile.id} row selected={selected} symbol={selected ? "checkmark.circle.fill" : "circle"} title={profile.label} subtitle={profile.description} label={`空间渲染预设：${profile.label}。${profile.description}`} disabled={disabled} onPress={() => p.setRenderingPreset(profile.id)} /> : <Pressable key={profile.id} accessibilityRole="radio" accessibilityLabel={`空间渲染预设：${profile.label}`} accessibilityState={{ checked: selected, disabled }} disabled={disabled} onPress={() => p.setRenderingPreset(profile.id)} style={[s.roomOption, { borderColor: selected ? "#167d72" : c.line, backgroundColor: selected ? c.soft : "transparent" }]}>
                <View style={s.settingCopy}>{label(profile.label, false, s.settingTitle)}{label(profile.description, true, s.settingDescription)}</View>{label(selected ? "●" : "○", !selected, { fontSize: 20 })}
              </Pressable>;
            })}
            {label("切换预设会保留播放进度和播放／暂停状态，并恢复默认对象选项；加载时可能短暂缓冲。", true, s.groupHint)}
            <View style={[s.cardDivider, { backgroundColor: c.line }]} />
            <View style={s.settingRow}><View style={s.settingCopy}>{label("逐对象渲染", false, s.settingTitle)}{label("为每个对象独立生成双耳声音", true, s.settingDescription)}</View><Switch accessibilityLabel="基础逐对象双耳渲染" trackColor={{ false: c.line, true: "#167d72" }} thumbColor="#ffffff" value={p.directObjects} disabled={p.systemSpatial360RAActive || p.busy} onValueChange={value => p.setRendering(value, p.directionalObjects)} /></View>
            <View style={[s.cardDivider, { backgroundColor: c.line }]} />
            <View style={s.settingRow}><View style={s.settingCopy}>{label("实际方向", false, s.settingTitle)}{label("按对象的真实位置定位声音", true, s.settingDescription)}</View><Switch accessibilityLabel="按对象实际方向渲染" trackColor={{ false: c.line, true: "#167d72" }} thumbColor="#ffffff" value={p.directionalObjects} disabled={p.systemSpatial360RAActive || p.busy} onValueChange={value => p.setRendering(p.directObjects, value)} /></View>
          </View>
          {label("实际方向会自动启用逐对象处理；两项均关闭时使用虚拟扬声器。", true, s.groupHint)}
{label("外观与输出", true, s.groupTitle)}
          <View style={[s.settingsCard, { backgroundColor: c.panel }]}>
            <View style={s.settingRow}><View style={s.settingCopy}>{label("外观", false, s.settingTitle)}{label("自动跟随系统", true, s.settingDescription)}</View>{label(isLight ? "浅色" : "深色", true, s.settingDescription)}</View>
            <View style={[s.cardDivider, { backgroundColor: c.line }]} />
            <View style={s.outputDetails}>{label("音频输出", false, s.settingTitle)}{label(p.playing ? p.renderingStatus : "等待播放", true, s.settingDescription)}{label(p.systemSpatial360RAActive ? "48 kHz · 浮点 PCM · 7.1.4（12 声道）" : "48 kHz · 浮点 PCM · 双声道", true, s.settingDescription)}</View>
          </View>
        </ScrollView>
      </Animated.View>
    </Modal>
  </View></SafeContainer>;
}
const s = StyleSheet.create({
  root: { flex: 1, paddingTop: 12, paddingHorizontal: 16, paddingBottom: 12 },
  header: { flexDirection: "row", alignItems: "center", gap: 16, marginBottom: 12 },
  headerAction: { width: 46, height: 46, justifyContent: "center", alignItems: "center" },
  player: { paddingBottom: 8 },
  mainPlay: { width: 64, height: 64, borderRadius: 32, justifyContent: "center", alignItems: "center" },
  pauseBar: { width: 6, height: 23, borderRadius: 1 },
  eyebrow: { fontSize: 10, letterSpacing: 2.2 }, title: { fontSize: 22, fontWeight: "600", marginTop: 8 },
  card: { borderWidth: 1, borderRadius: 24, padding: 22 }, row: { flexDirection: "row", alignItems: "center", justifyContent: "space-between" },
  progress: { height: 5, borderRadius: 8, marginTop: 18, overflow: "hidden" },
  small: { fontSize: 11 }, sectionTitle: { fontSize: 19, fontWeight: "600" },
  art: { alignItems: "center", justifyContent: "center", borderRadius: 12, elevation: 12, shadowColor: "#000", shadowOpacity: .3, shadowRadius: 18, shadowOffset: { width: 0, height: 12 } }, globe: { width: 100, height: 100, borderRadius: 50, borderWidth: 1, alignItems: "center", justifyContent: "center", opacity: .7 },
  longitude: { position: "absolute", width: 46, height: 100, borderRadius: 50, borderWidth: 1 }, latitude: { position: "absolute", width: 100, height: 40, borderRadius: 50, borderWidth: 1 }, equator: { width: 98, height: 1 },
  trackInfo: { marginTop: 38, marginBottom: 0 },
  trackTitle: { fontSize: 23, lineHeight: 32, textAlign: "left", fontWeight: "500", letterSpacing: 0, includeFontPadding: false },
  trackSubtitle: { fontSize: 14, lineHeight: 22, marginTop: 8, fontWeight: "400", includeFontPadding: false },
  centerHint: { fontSize: 12, textAlign: "center", marginTop: 12, lineHeight: 20 },
  circle: { width: 46, height: 46, borderRadius: 30, borderWidth: 1, alignItems: "center", justifyContent: "center" }, play: { width: 64, height: 64, borderRadius: 32 },
  transport: { flexDirection: "row", alignItems: "center", justifyContent: "center", gap: 25 }, volumeTouch: { flex: 1, height: 44, justifyContent: "center" }, volumeTrack: { height: 5, borderRadius: 8 },
  tabs: { flexDirection: "row", justifyContent: "center", gap: 8, paddingTop: 8 }, tab: { minWidth: 48, alignItems: "center", padding: 2 },
  footer: { flexDirection: "row", justifyContent: "space-between", marginTop: 12 },
  queueItem: { flexDirection: "row", alignItems: "center", gap: 16, padding: 18, borderRadius: 14, marginTop: 26 }, choose: { padding: 16, alignItems: "center", borderRadius: 14, borderWidth: 1, marginTop: 20 },
  help: { fontSize: 12, lineHeight: 20, marginTop: 12, textAlign: "center" }, scene: { borderRadius: 16, overflow: "hidden", marginTop: 16, backgroundColor: "#171a19" },
  backdrop: { position: "absolute", top: 0, right: 0, bottom: 0, left: 0, backgroundColor: "#0008" }, sheet: { position: "absolute", bottom: 0, right: 0, left: 0, borderTopLeftRadius: 28, borderTopRightRadius: 28, overflow: "hidden" },
  sheetHandle: { width: 36, height: 4, borderRadius: 2, alignSelf: "center", marginTop: 10 },
  sheetHeader: { flexDirection: "row", alignItems: "center", paddingHorizontal: 22, paddingTop: 18, paddingBottom: 20 },
  sheetTitle: { fontSize: 23, fontWeight: "600", lineHeight: 30, includeFontPadding: false },
  sheetSubtitle: { fontSize: 13, lineHeight: 20, marginTop: 4 },
  sheetContent: { paddingHorizontal: 18, paddingBottom: 32 },
  quickActions: { flexDirection: "row", gap: 12, marginBottom: 12 },
  quickAction: { flex: 1, borderRadius: 16, minHeight: 52, justifyContent: "center", alignItems: "center" },
  settingsCard: { borderRadius: 18, paddingHorizontal: 16, overflow: "hidden" },
  groupTitle: { fontSize: 12, fontWeight: "500", marginTop: 24, marginBottom: 10, marginLeft: 4 },
  groupHint: { fontSize: 12, lineHeight: 19, marginTop: 10, paddingHorizontal: 4 },
  settingRow: { flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 12, minHeight: 76, paddingVertical: 14 },
  settingCopy: { flex: 1 },
  settingTitle: { fontSize: 15, lineHeight: 22, fontWeight: "500", includeFontPadding: false },
  settingDescription: { fontSize: 12, lineHeight: 19, marginTop: 4 },
  profileHeader: { flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 12, paddingVertical: 18 },
  profileTitle: { fontSize: 17, lineHeight: 24, fontWeight: "600" },
  profileBadge: { paddingHorizontal: 10, paddingVertical: 5, borderRadius: 8 },
  cardDivider: { height: StyleSheet.hairlineWidth },
  stepper: { flexDirection: "row", alignItems: "center", borderRadius: 12 },
  stepperButton: { width: 34, height: 44, alignItems: "center", justifyContent: "center" },
  roomOption: { flexDirection: "row", alignItems: "center", gap: 12, borderWidth: 1, borderRadius: 12, padding: 14, marginBottom: 12 },
  outputDetails: { paddingVertical: 16 },
  setting: { flexDirection: "row", alignItems: "center", justifyContent: "space-between", minHeight: 52, gap: 8 }, divider: { height: 1 }, error: { color: "#dd7c73", fontSize: 12, paddingTop: 8 },
});
