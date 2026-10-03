import React, { useEffect, useRef, useState } from "react";
import { Alert, Image, PanResponder, Pressable, ScrollView, StatusBar, StyleSheet, Switch, Text, View, useColorScheme, useWindowDimensions } from "react-native";
import { SafeAreaProvider, SafeAreaView, initialWindowMetrics } from "react-native-safe-area-context";
import { followingPlaybackMode, PLAYBACK_MODE_LABELS } from "../../web/src/playbackOrder";
import renderingPresets from "../rendering-presets.json";
import type { PlayerProps } from "./RemotePlayer";
import { MobileObjectScene } from "./MobileObjectScene";
import { hasNativeIOSChrome, IOSAction, IOSDistanceStepper, IOSGlassTabs, IOSIconButton, IOSMaterialSurface, IOSVolumeSlider } from "./IOSNativeChrome";
import { hasSystemIOSTabs, IOSSystemTabs } from "./IOSSystemTabs";
import { IOSSettingsNavigation, IOSSettingsButton, hasNativeIOSSettings } from "./IOSNativeSettings";
import { compatibleRooms, IOS_TABS, isPresetSelected, playbackStatus, trackTitle, iosPlayerLayout } from "./ios-ui-model";

import { IOSVolumeSymbol } from "./IOSVolumeSymbol";

const palettes = {
  light: { bg: "#f0f3f0", panel: "#fafcf9", ink: "#24302a", muted: "#66776c", line: "#d5dfd6", field: "#eef2ed", accent: "#28754a" },
  dark: { bg: "#171a19", panel: "#202523", ink: "#e9eeeb", muted: "#a0ada6", line: "#35413a", field: "#2a322e", accent: "#9fdcb9" },
};
const time = (ms: number) => Math.floor(ms / 60000) + ":" + String(Math.floor(ms / 1000) % 60).padStart(2, "0");

// The approved remote-inspired presentation. All audio actions remain owned by
// App/SdaPlayer; navigating between pages never stops or restarts a track.
export function IOSPlayer(p: PlayerProps) {
  const systemTheme = useColorScheme();
  const { width, height } = useWindowDimensions();
  const [sceneSmoke] = useState(() => (globalThis as any).expo?.modules?.SdaEngine?.sceneSmokeEnabled?.() === true);
  const [chromeSmoke] = useState(() => (globalThis as any).expo?.modules?.SdaGlassButton?.smokeStage?.() || "");
  const [page, setPage] = useState(sceneSmoke ? 2 : chromeSmoke === "library" ? 1 : 0);
  const [settings, setSettings] = useState(chromeSmoke.startsWith("settings"));
  const [sceneVisited, setSceneVisited] = useState(sceneSmoke);
  const [librarySize, setLibrarySize] = useState({ viewport: 0, content: 0 });
  const libraryCanScroll = librarySize.viewport > 0 && librarySize.content > librarySize.viewport + 1;
  const settingsScroll = useRef<ScrollView>(null);
  const volumeWidth = useRef(1);
  const volumeOrigin = useRef(0);
  const isLight = systemTheme !== "dark";
  const c = palettes[isLight ? "light" : "dark"];
  const title = p.metadata.title || p.fileName || "等待选择歌曲";
  const artist = p.metadata.artist || p.metadata.albumArtist || (p.selectedUri ? "未知艺人" : "打开音频，开始聆听");
  const status = playbackStatus(p);
  const engineUnavailable = p.error?.includes("SdaEngine native module is not registered") === true;
  const progress = p.durationMs > 0 ? Math.max(0, Math.min(1, p.positionMs / p.durationMs)) : 0;
  const [playerHeight, setPlayerHeight] = useState(Math.max(300, height - 230));
  const [playerBlocks, setPlayerBlocks] = useState({ status: 20, info: 66, controls: 107, volume: 44 });
  const { coverSize, gap: playerGap } = iosPlayerLayout(width, playerHeight, Object.values(playerBlocks).reduce((sum, value) => sum + value, 0));
  const measurePlayerBlock = (key: keyof typeof playerBlocks, value: number) => {
    setPlayerBlocks(previous => Math.abs(previous[key] - value) < 0.5 ? previous : { ...previous, [key]: value });
  };
  const format = p.layout === "360RA-13" || p.systemSpatial360RAActive ? "360 Reality Audio" : /atmos/i.test(p.renderingStatus) ? "Dolby Atmos" : "空间音频";
  const audioOptionsDisabled = p.systemSpatial360RAActive || p.busy;
  const presetDisabled = audioOptionsDisabled || p.roomBusy || p.nearFieldBusy;
  const canChooseRoom = !(p.systemSpatial360RAActive && p.systemSpatial360RA) && !p.busy && !p.roomBusy;
  const rooms = compatibleRooms(p);
  const currentPreset = renderingPresets.find(profile => isPresetSelected(p, profile));
  const playing = p.playing && !p.paused;
  const togglePlayback = () => p.playing ? p.togglePause() : p.play();
  const navigate = (index: number) => { setPage(index); if (index === 2) setSceneVisited(true); };
  useEffect(() => {
    if (p.playbackPageRequest) { setSettings(false); navigate(0); }
  }, [p.playbackPageRequest]);

  const volumeGesture = React.useMemo(() => PanResponder.create({
    onStartShouldSetPanResponder: () => true,
    onStartShouldSetPanResponderCapture: () => true,
    onPanResponderTerminationRequest: () => false,
    onPanResponderGrant: event => {
      volumeOrigin.current = event.nativeEvent.pageX - event.nativeEvent.locationX;
      p.setVolume(Math.max(0, Math.min(1, event.nativeEvent.locationX / volumeWidth.current)));
    },
    onPanResponderMove: (_, gesture) => p.setVolume(Math.max(0, Math.min(1, (gesture.moveX - volumeOrigin.current) / volumeWidth.current))),
  }), [p.setVolume]);
  const label = (value: string, muted = false, style: object = {}) => <Text style={[{ color: muted ? c.muted : c.ink }, style]}>{value}</Text>;
  const icon = (symbol: string, name: string, action: () => void, disabled = false, size = 44, fallback = "•") => hasNativeIOSChrome
    ? <IOSIconButton symbol={symbol} label={name} onPress={action} disabled={disabled} size={size} symbolSize={size > 50 ? 25 : 19} />
    : <Pressable accessibilityRole="button" accessibilityLabel={name} disabled={disabled} onPress={action} style={[s.icon, { width: size, height: size, backgroundColor: c.panel, opacity: disabled ? .35 : 1 }]}>{label(fallback, false, { fontSize: 22 })}</Pressable>;
  const divider = () => <View style={[s.divider, { backgroundColor: c.line }]} />;
  const toggle = (name: string, description: string, value: boolean, onValueChange: (enabled: boolean) => void, disabled = false) => <View style={s.settingRow}>
    <View style={s.settingCopy}>{label(name, false, s.settingTitle)}{!!description && label(description, true, s.settingHint)}</View>
    <Switch accessibilityLabel={name} value={value} onValueChange={onValueChange} disabled={disabled} trackColor={{ false: c.line, true: c.accent }} />
  </View>;
  const actionRow = (symbol: string, name: string, description: string, action: () => void, disabled = false, selected = false) => hasNativeIOSChrome
    ? <IOSAction row symbol={symbol} title={name} subtitle={description} selected={selected} disabled={disabled} onPress={action} />
    : <Pressable accessibilityRole="button" accessibilityLabel={name} accessibilityState={{ disabled, selected }} disabled={disabled} onPress={action} style={[s.settingRow, { opacity: disabled ? .4 : 1 }]}><View style={s.settingCopy}>{label(name, false, s.settingTitle)}{!!description && label(description, true, s.settingHint)}</View>{label(selected ? "✓" : "›", true)}</Pressable>;
  const cover = (size: number) => p.metadata.coverUri
    ? <Image source={{ uri: p.metadata.coverUri }} resizeMode="cover" accessibilityLabel={(p.metadata.album || title) + " 封面"} style={{ width: size, height: size, borderRadius: size > 100 ? 18 : 8 }} />
    : <View accessibilityLabel="无专辑封面" style={[s.emptyCover, { width: size, height: size, borderRadius: size > 100 ? 18 : 8, backgroundColor: c.field }]}>{label("♫", true, { fontSize: size > 100 ? 56 : 23 })}</View>;
  const miniPlayer = () => !!p.selectedUri && <View style={[s.miniPlayer, { backgroundColor: c.panel, borderColor: c.line }]}>
    <Pressable accessibilityRole="button" accessibilityLabel="展开正在播放" onPress={() => navigate(0)} style={s.miniSong}>{cover(40)}<View style={{ flex: 1 }}><Text numberOfLines={1} style={[s.miniTitle, { color: c.ink }]}>{title}</Text><Text numberOfLines={1} style={[s.miniArtist, { color: c.muted }]}>{artist}</Text></View></Pressable>
    {icon(playing ? "pause.fill" : "play.fill", playing ? "暂停" : "播放", togglePlayback, p.busy || !p.selectedUri, 44, playing ? "Ⅱ" : "▷")}
  </View>;
  const heading = (name: string, stage = "") => <View onLayout={event => {
    if (stage && chromeSmoke === stage) { const y = event.nativeEvent.layout.y; requestAnimationFrame(() => settingsScroll.current?.scrollTo({ y, animated: false })); }
  }}>{label(name, true, s.groupTitle)}</View>;

  return <SafeAreaProvider initialMetrics={initialWindowMetrics} style={{ flex: 1, backgroundColor: c.bg }}>
  <IOSSettingsNavigation settings={settings} onSettingsChange={setSettings} title={page === 0 ? "正在播放" : IOS_TABS[page] || "正在播放"} player={p} accent={c.accent} theme={isLight ? "light" : "dark"}>
  {/* Native tabs own the bottom inset. Reserve only the header/side insets
      here, so the tab bar's background reaches the home indicator. */}
  <SafeAreaView edges={hasSystemIOSTabs ? ["top", "left", "right"] : ["top", "bottom", "left", "right"]} style={[s.safe, { backgroundColor: c.bg }]}>
    <StatusBar barStyle={isLight ? "dark-content" : "light-content"} backgroundColor={c.bg} />
    <View style={[s.root, hasSystemIOSTabs && { paddingBottom: 0 }]}>
      <View style={{ flex: 1 }} accessibilityElementsHidden={settings} importantForAccessibility={settings ? "no-hide-descendants" : "auto"}>
      <View style={s.header}>
        <View style={{ flex: 1 }}>{label("SDA", true, s.eyebrow)}{label(page === 0 ? "正在播放" : IOS_TABS[page] || "正在播放", false, s.pageTitle)}</View>
        {hasNativeIOSSettings ? <IOSSettingsButton onPress={() => setSettings(true)} /> : icon("gearshape", "更多设置", () => setSettings(true), false, 44, "⚙")}
      </View>
      {/* Status belongs above the native tabs, never below their full-screen
          host, where it steals the home-indicator inset from the tab bar. */}
      {!!p.error && !engineUnavailable && <Text accessibilityRole="alert" style={s.error}>{p.error}</Text>}
      <IOSSystemTabs selected={page} onChange={navigate} accessory={miniPlayer()} backgroundColor={c.bg} accent={c.accent} theme={isLight ? "light" : "dark"} fallback={<View style={s.tabs}>{hasNativeIOSChrome ? <IOSGlassTabs selected={page} onChange={navigate} /> : <View style={[s.fallbackTabs, { backgroundColor: c.panel, borderColor: c.line }]}>{IOS_TABS.map((name, index) => <Pressable key={name} accessibilityRole="tab" accessibilityState={{ selected: page === index }} onPress={() => navigate(index)} style={[s.fallbackTab, { backgroundColor: page === index ? c.field : "transparent" }]}>{label(name, page !== index, s.small)}</Pressable>)}</View>}</View>}>
        <View style={[s.page, s.playerContent, { gap: playerGap }, !hasSystemIOSTabs && page !== 0 && s.hidden]}
          onLayout={event => { const available = event.nativeEvent.layout.height; if (available > 0) setPlayerHeight(previous => Math.abs(previous - available) < 0.5 ? previous : available); }}>
          <View style={s.playerBlock} onLayout={event => measurePlayerBlock("status", event.nativeEvent.layout.height)}>
          {engineUnavailable && <Text accessibilityRole="alert" style={[s.engineNotice, { color: c.muted }]}>当前运行环境未包含 SDA 音频引擎；可查看界面，播放需要完整安装包。</Text>}
          <View style={s.row}>{label(format + " · 48 kHz", true, s.small)}<Text accessibilityLiveRegion="polite" style={[s.small, { color: c.accent, maxWidth: "58%", textAlign: "right" }]}>{status}</Text></View>
          </View>
          <View style={[s.album, { width: coverSize, height: coverSize }]}>{cover(coverSize)}</View>
          <View style={s.playerBlock} onLayout={event => measurePlayerBlock("info", event.nativeEvent.layout.height)}>
          <Text numberOfLines={2} style={[s.trackTitle, { color: c.ink }]}>{title}</Text>
          <Text numberOfLines={2} style={[s.trackSubtitle, { color: c.muted }]}>{artist}{p.metadata.album ? " · " + p.metadata.album : ""}</Text>
          </View>
          <View style={s.playerBlock} onLayout={event => measurePlayerBlock("controls", event.nativeEvent.layout.height)}>
          <View accessibilityRole="progressbar" accessibilityLabel="播放进度" accessibilityValue={p.durationMs > 0 ? { min: 0, max: p.durationMs, now: Math.min(p.durationMs, p.positionMs), text: time(p.positionMs) + " / " + time(p.durationMs) } : { text: "总时长未知" }} style={[s.progress, { backgroundColor: c.line }]}><View style={{ width: String(progress * 100) + "%" as any, height: 5, borderRadius: 8, backgroundColor: c.accent }} /></View>
          <View style={s.times}>{label(time(p.positionMs), true, s.small)}{label(p.durationMs > 0 ? time(p.durationMs) : "--:--", true, s.small)}</View>
          <View style={s.transport}>
            {icon(p.playbackMode === "repeat-one" ? "repeat.1" : p.playbackMode === "repeat-all" ? "repeat" : "list.bullet", "播放模式：" + PLAYBACK_MODE_LABELS[p.playbackMode], () => p.setPlaybackMode(followingPlaybackMode(p.playbackMode)), false, 44, "↻")}
            {icon("backward.end.fill", "上一曲", p.previous, p.busy || !p.queue.length, 44, "|◁")}
            {icon(playing ? "pause.fill" : "play.fill", playing ? "暂停" : "播放", togglePlayback, p.busy || !p.selectedUri, 58, playing ? "Ⅱ" : "▷")}
            {icon("forward.end.fill", "下一曲", p.next, p.busy || !p.queue.length, 44, "▷|")}
            {icon("arrow.counterclockwise", "重新播放", p.play, p.busy || !p.selectedUri, 44, "⟲")}
          </View>
          </View>
          <View style={[s.volume, s.playerBlock]} onLayout={event => measurePlayerBlock("volume", event.nativeEvent.layout.height)}><IOSVolumeSymbol volume={p.volume} color={c.muted} />{hasNativeIOSChrome ? <IOSVolumeSlider value={p.volume} onChange={p.setVolume} onTracking={() => { /* Fixed page has no parent scrolling to suspend. */ }} /> : <View onLayout={event => { volumeWidth.current = Math.max(1, event.nativeEvent.layout.width); }} {...volumeGesture.panHandlers} style={s.volumeTouch}><View style={[s.progress, { marginTop: 0, backgroundColor: c.line }]}><View style={{ height: 5, width: String(p.volume * 100) + "%" as any, backgroundColor: c.accent }} /></View></View>}{label(Math.round(p.volume * 100) + "%", true, s.volumeValue)}</View>
        </View>
        <ScrollView style={[s.page, !hasSystemIOSTabs && page !== 1 && s.hidden]} contentContainerStyle={s.otherContent}
          scrollEnabled={libraryCanScroll} bounces={false} alwaysBounceVertical={false} showsVerticalScrollIndicator={false}
          onLayout={event => { const viewport = event.nativeEvent.layout.height; setLibrarySize(previous => previous.viewport === viewport ? previous : { ...previous, viewport }); }}
          onContentSizeChange={(_, content) => setLibrarySize(previous => previous.content === content ? previous : { ...previous, content })}>
          {label("音乐留在你的设备上", true, s.lead)}
          <View style={[s.group, { backgroundColor: c.panel, borderColor: c.line }]}>{actionRow("folder.open", "打开本机文件", "从「文件」选择音乐", p.chooseFile, p.busy)}</View>
          <View style={[s.row, { marginTop: 28, marginBottom: 8 }]}>{label("播放列表", false, s.sectionTitle)}{label(p.queue.length + " 首", true, s.small)}</View>
          {p.queue.map((track, index) => <Pressable key={track.contentHash} accessibilityRole="button" accessibilityState={{ selected: index === p.queueIndex, disabled: p.busy }} disabled={p.busy} onPress={() => { p.selectTrack(index); navigate(0); }} style={[s.queueRow, { borderBottomColor: c.line }]}>
            {track.metadata.coverUri ? <Image source={{ uri: track.metadata.coverUri }} resizeMode="cover" style={s.queueCover} /> : <View style={[s.queueCover, s.emptyCover, { backgroundColor: c.field }]}>{label("♫", true, { fontSize: 23 })}</View>}
            <View style={{ flex: 1 }}><Text numberOfLines={2} style={[s.queueTitle, { color: c.ink }]}>{trackTitle(track)}</Text><Text numberOfLines={1} style={[s.queueArtist, { color: c.muted }]}>{track.metadata.artist || track.metadata.albumArtist || "本机文件"}{track.metadata.album ? " · " + track.metadata.album : ""}</Text></View>
            {label(index === p.queueIndex ? "●" : "›", index !== p.queueIndex, { color: index === p.queueIndex ? c.accent : c.muted })}
          </Pressable>)}
          {!p.queue.length && <View style={s.emptyLibrary}>{label("还没有歌曲", false, s.sectionTitle)}{label("打开本机文件，添加到播放列表", true, s.lead)}</View>}
          {label("本机播放，不上传音乐，也不改变源文件。", true, s.libraryHint)}
        </ScrollView>
        <View style={[s.page, s.otherContent, !hasSystemIOSTabs && page !== 2 && s.hidden]}>
          <View style={s.row}>{label(format, true, s.small)}{label(p.layout === "360RA-13" ? "360° 球形声场" : "7.1.4", true, s.small)}</View>
          <View style={[s.scene, { height: Math.max(270, Math.min(410, height * .42)) }]}>{sceneVisited && <MobileObjectScene layout={p.layout} objects={p.objects} active={page === 2 && !settings} />}</View>
          <View style={[s.row, s.sceneInfo]}><View>{label(p.systemSpatial360RAActive ? "系统空间音频" : "KU100", false, s.settingTitle)}{label(p.systemSpatial360RAActive ? "7.1.4 系统输出" : currentPreset?.label || "SDA 空间渲染", true, s.small)}</View><View style={{ alignItems: "flex-end" }}>{label(p.objects.length + " 个对象", false, s.settingTitle)}{label("实时位置", true, s.small)}</View></View>
          {label("单指旋转 · 双指缩放", true, s.sceneHint)}
        </View>
      </IOSSystemTabs>

      </View>
      {settings && !hasNativeIOSSettings && <View style={[s.settingsPage, { backgroundColor: c.bg }]} accessibilityViewIsModal>
        <View style={[s.settingsHeader, { backgroundColor: c.panel }]}>
          {hasNativeIOSChrome && <IOSMaterialSurface style={StyleSheet.absoluteFill} />}
          {icon("chevron.left", "返回", () => setSettings(false), false, 44, "‹")}
          {label("设置", false, s.settingsTitle)}<View style={{ width: 44 }} />
        </View>
        <ScrollView ref={settingsScroll} showsVerticalScrollIndicator={false} contentContainerStyle={s.settingsContent}>
          <View style={s.settingsActions}>{icon("arrow.counterclockwise", "重新播放", () => { setSettings(false); p.play(); }, p.busy || !p.selectedUri, 46, "⟲")}{icon("stop.fill", "停止播放", () => { setSettings(false); p.stop(); }, p.busy || !p.playing, 46, "□")}</View>
          {heading("播放")}
          <View style={[s.group, { backgroundColor: c.panel, borderColor: c.line }]}>
            {toggle("音量平衡", "双声道 / 360RA 响度平衡，只衰减不增益", p.volumeBalanceEnabled, p.setVolumeBalance, p.busy)}
            {divider()}
            {actionRow("repeat", "播放模式", PLAYBACK_MODE_LABELS[p.playbackMode], () => p.setPlaybackMode(followingPlaybackMode(p.playbackMode)))}
          </View>
          {heading("360 Reality Audio")}
          <View style={[s.group, { backgroundColor: c.panel, borderColor: c.line }]}>{toggle("系统空间音频 · 7.1.4", "仅 360RA：12 声道交给系统，旁路 KU100、近场与房间", p.systemSpatial360RA, p.setSystemSpatial360RA, p.busy)}</View>
          {label("修改后下一次播放生效，不中断当前歌曲。关闭后恢复 SDA / KU100 空间渲染，并非普通立体声下混。", true, s.groupHint)}
          {heading("空间渲染", "settings-spatial")}
          <View style={[s.group, { backgroundColor: c.panel, borderColor: c.line }]}>
            {renderingPresets.map(profile => <View key={profile.id}>{actionRow(isPresetSelected(p, profile) ? "checkmark.circle.fill" : "circle", profile.label, profile.description, () => p.setRenderingPreset(profile.id), presetDisabled, isPresetSelected(p, profile))}{divider()}</View>)}
            {toggle("逐对象渲染", "每个对象独立生成双耳声音", p.directObjects, value => p.setRendering(value, p.directionalObjects), audioOptionsDisabled)}
            {divider()}
            {toggle("实际方向", "按对象真实位置定位声音", p.directionalObjects, value => p.setRendering(p.directObjects, value), audioOptionsDisabled)}
          </View>
          {label("切换预设保留播放进度与播放 / 暂停状态；加载时可能短暂缓冲。", true, s.groupHint)}
          {heading("近场与距离")}
          <View style={[s.group, { backgroundColor: c.panel, borderColor: c.line }]}>
            {toggle("近场渲染", "按对象距离计算近场声学效果", p.nearField, value => p.setNearField(value, p.metresPerUnit), audioOptionsDisabled || p.nearFieldBusy)}
            {divider()}
            <View style={s.settingRow}><View style={s.settingCopy}>{label("距离映射", false, s.settingTitle)}{label(p.metresPerUnit.toFixed(2) + " m / 单位", true, s.settingHint)}</View>{hasNativeIOSChrome ? <IOSDistanceStepper value={p.metresPerUnit} disabled={audioOptionsDisabled || p.nearFieldBusy} onChange={value => p.setNearField(p.nearField, value)} /> : <View style={s.row}>{icon("minus", "减小距离", () => p.setNearField(p.nearField, Math.max(.25, p.metresPerUnit - .05)), audioOptionsDisabled || p.nearFieldBusy, 44, "−")}{icon("plus", "增大距离", () => p.setNearField(p.nearField, Math.min(4, p.metresPerUnit + .05)), audioOptionsDisabled || p.nearFieldBusy, 44, "+")}</View>}</View>
          </View>
          {heading("房间仿真", "settings-room")}
          <View style={[s.group, { backgroundColor: c.panel, borderColor: c.line }]}>
            {[{ id: "", name: "关闭", layout: "" }, ...rooms].map(room => <View key={room.id}>{actionRow(room.id === p.roomId ? "checkmark.circle.fill" : "circle", room.name.startsWith("SDA Near-field Control Room") ? "近场录音棚" : room.name, room.id ? room.layout + " · 房间资产" : "使用直接双耳渲染", () => p.setRoom(room.id), !canChooseRoom, room.id === p.roomId)}{divider()}</View>)}
          </View>
          {label(p.systemSpatial360RAActive && p.systemSpatial360RA ? "当前系统输出旁路房间仿真。" : "早期反射 −6 dB，直达声与混响尾部保持不变。", true, s.groupHint)}
          {heading("应用与输出")}
          <View style={[s.group, { backgroundColor: c.panel, borderColor: c.line }]}>
            <View style={s.settingRow}>{label("外观", false, s.settingTitle)}{label("跟随系统", true, s.small)}</View>{divider()}
            <View style={s.settingRow}><View style={s.settingCopy}>{label("音频输出", false, s.settingTitle)}{label(p.playing ? p.renderingStatus : "等待播放", true, s.settingHint)}{label(p.systemSpatial360RAActive ? "48 kHz · 浮点 PCM · 7.1.4" : "48 kHz · 浮点 PCM · 双声道", true, s.settingHint)}</View></View>{divider()}
            {actionRow("info.circle", "关于 SDA", "Spatial Decoder App", () => Alert.alert("SDA", "Spatial Decoder App\n本机空间音频播放器"))}
          </View>

        </ScrollView>
      </View>}
    </View>
  </SafeAreaView>
  </IOSSettingsNavigation>
  </SafeAreaProvider>;
}

const s = StyleSheet.create({
  engineNotice: { fontSize: 12, lineHeight: 18, marginBottom: 12 },
  safe: { flex: 1 }, root: { flex: 1, paddingHorizontal: 18, paddingBottom: 8 },
  header: { flexDirection: "row", alignItems: "center", paddingVertical: 13, paddingHorizontal: 3 },
  eyebrow: { fontSize: 11, letterSpacing: 3 }, pageTitle: { fontSize: 24, fontWeight: "600", marginTop: 5 },
  pages: { flex: 1 }, page: { flex: 1 }, hidden: { display: "none" },
  playerContent: { paddingHorizontal: 7, paddingTop: 8, paddingBottom: 14, justifyContent: "space-between" }, playerBlock: { flexShrink: 0 }, otherContent: { paddingTop: 8, paddingBottom: 20 },
  row: { flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }, small: { fontSize: 11, lineHeight: 17 },
  album: { alignSelf: "center", flexShrink: 0, shadowColor: "#000", shadowOpacity: .15, shadowRadius: 18, shadowOffset: { width: 0, height: 12 } },
  emptyCover: { alignItems: "center", justifyContent: "center" }, trackTitle: { fontSize: 22, fontWeight: "600", textAlign: "center", lineHeight: 29 },
  trackSubtitle: { fontSize: 13, textAlign: "center", marginTop: 8, lineHeight: 20 },
  progress: { height: 5, borderRadius: 8, overflow: "hidden", marginTop: 4 }, times: { flexDirection: "row", justifyContent: "space-between", marginTop: 8 },
  transport: { flexDirection: "row", justifyContent: "center", alignItems: "center", gap: 9, marginTop: 20 }, icon: { borderRadius: 32, alignItems: "center", justifyContent: "center" },
  volume: { flexDirection: "row", alignItems: "center", gap: 10 }, volumeValue: { fontSize: 11, minWidth: 31, textAlign: "right", fontVariant: ["tabular-nums"] }, volumeTouch: { flex: 1, height: 44, justifyContent: "center" },
  lead: { fontSize: 13, lineHeight: 21, marginBottom: 22 }, group: { borderRadius: 15, borderWidth: StyleSheet.hairlineWidth, paddingHorizontal: 12, overflow: "hidden" },
  sectionTitle: { fontSize: 18, fontWeight: "600" }, queueRow: { flexDirection: "row", alignItems: "center", gap: 12, paddingVertical: 15, borderBottomWidth: StyleSheet.hairlineWidth },
  queueCover: { width: 49, height: 49, borderRadius: 8 }, queueTitle: { fontSize: 15, lineHeight: 21 }, queueArtist: { fontSize: 12, marginTop: 5 },
  emptyLibrary: { paddingVertical: 35, gap: 10 }, libraryHint: { fontSize: 12, lineHeight: 20, marginTop: 28 },
  miniPlayer: { flexDirection: "row", alignItems: "center", gap: 4, borderRadius: 16, borderWidth: StyleSheet.hairlineWidth, padding: 8 },
  miniSong: { flex: 1, flexDirection: "row", alignItems: "center", gap: 10 }, miniTitle: { fontSize: 13, lineHeight: 18 }, miniArtist: { fontSize: 11, marginTop: 3 },
  tabs: { marginTop: 10 }, fallbackTabs: { flexDirection: "row", padding: 5, borderWidth: StyleSheet.hairlineWidth, borderRadius: 26 }, fallbackTab: { flex: 1, minHeight: 50, alignItems: "center", justifyContent: "center", borderRadius: 22 },
  scene: { marginTop: 17, borderRadius: 18, overflow: "hidden", backgroundColor: "#171a19" }, sceneInfo: { marginTop: 18, paddingHorizontal: 4 }, sceneHint: { textAlign: "center", fontSize: 12, marginTop: 18 },
  settingsPage: { position: "absolute", top: 0, right: 0, bottom: 0, left: 0, zIndex: 10 }, settingsHeader: { flexDirection: "row", alignItems: "center", justifyContent: "space-between", paddingHorizontal: 8, paddingVertical: 8, overflow: "hidden" }, settingsTitle: { fontSize: 18, fontWeight: "600" },
  settingsContent: { paddingHorizontal: 3, paddingBottom: 28 }, groupTitle: { fontSize: 12, marginTop: 25, marginBottom: 8, marginLeft: 12 },
  settingRow: { flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 12, paddingVertical: 13, minHeight: 54 }, settingCopy: { flex: 1 }, settingTitle: { fontSize: 15, lineHeight: 22 }, settingHint: { fontSize: 12, lineHeight: 19, marginTop: 4 }, groupHint: { fontSize: 12, lineHeight: 19, marginTop: 9, paddingHorizontal: 12 },
  divider: { height: StyleSheet.hairlineWidth }, settingsActions: { flexDirection: "row", justifyContent: "center", gap: 24, marginTop: 25 }, error: { color: "#bc483a", fontSize: 12, lineHeight: 18, marginTop: 7 },
});
