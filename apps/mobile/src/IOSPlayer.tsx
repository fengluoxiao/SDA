import { androidMaterialTheme } from "./android-material-theme";
import { AppThemeProvider, CHARACTER_THEMES, useAppTheme } from "./app-theme";
import React, { useEffect, useRef, useState } from "react";
import { Alert, BackHandler, Image, Platform, PlatformColor, PanResponder, Pressable, ScrollView, StatusBar, StyleSheet, Switch, Text, View, useColorScheme, useWindowDimensions } from "react-native";
import { SafeAreaProvider, SafeAreaView, initialWindowMetrics } from "react-native-safe-area-context";
import { followingPlaybackMode, PLAYBACK_MODE_LABELS } from "../../web/src/playbackOrder";
import renderingPresets from "../rendering-presets.json";
import type { PlayerProps } from "./RemotePlayer";
import { MobileObjectScene } from "./MobileObjectScene";
import { hasNativeIOSChrome, IOSAction, IOSGlassTabs, IOSMaterialSurface } from "./IOSNativeChrome";
import { hasNativePlayerTabs as hasSystemIOSTabs, PlatformPlayerTabs as IOSSystemTabs } from "./PlatformPlayerTabs";
import { AndroidSymbol } from "./AndroidSymbol";
import { AndroidGlassSettingsButton } from "./AndroidGlassSettingsButton";
import { AndroidSettings } from "./AndroidSettings";
import { AndroidAudioInfo } from "./AndroidAudioInfo";
import { IOSSettingsNavigation, IOSSettingsButton, IOSHeadTrackingButton, hasNativeIOSSettings } from "./IOSNativeSettings";
import { IOS_TABS, isPresetSelected, trackTitle, iosPlayerLayout, iosLandscapeControlLayout, formatPlaybackTime } from "./ios-ui-model";

import { IOSMetadataDoneButton, IOSMetadataSheet } from "./IOSMetadataSheet";
import { artworkColor, readArtworkColor } from "./artwork-color";
import { swiftUI } from "./IOSSystemTabs";
import { IOSPlaybackSymbol, IOSSkipSymbol } from "./IOSPlaybackSymbol";
import { QueuePreparationOverlay } from "./QueuePreparationOverlay";

function Symbol({ name, fallback, size = 22, color = "#fff", theme = "dark" }: { name: string; fallback: string; size?: number; color?: string; theme?: "light" | "dark" }) {
  if (Platform.OS === "android") return <AndroidSymbol name={name} size={size} color={color} />;
  const native = !!swiftUI && !!(globalThis as any).expo?.getViewConfig?.("ExpoUI", "ImageView");
  return <View pointerEvents="none" accessible={false} accessibilityElementsHidden style={{ width: size + 4, height: size + 4, alignItems: "center", justifyContent: "center" }}>
    {native && swiftUI ? <swiftUI.Host style={{ width: size + 4, height: size + 4 }} colorScheme={theme}><swiftUI.Image systemName={name as React.ComponentProps<typeof swiftUI.Image>["systemName"]} size={size} color={color} /></swiftUI.Host> : <Text style={{ fontSize: size, color }}>{fallback}</Text>}
  </View>;
}


const palettes = {
  light: { bg: "#f2f2f7", panel: "#ffffff", ink: "#1c1c1e", muted: "#74747c", line: "#dcdce3", field: "#e8e8ef", accent: "#7799CC" },
  dark: { bg: "#111113", panel: "#242427", ink: "#f5f5f7", muted: "#a0a0a8", line: "#3c3c43", field: "#29292e", accent: "#7799CC" },
};
const time = formatPlaybackTime;

// The approved remote-inspired presentation. All audio actions remain owned by
// App/SdaPlayer; navigating between pages never stops or restarts a track.
export function IOSPlayer(p: PlayerProps) { return <AppThemeProvider><ThemedPlayer {...p} /></AppThemeProvider>; }
function ThemedPlayer(p: PlayerProps) {
  const appTheme = useAppTheme();
  const previewMetadata = false;
  const systemTheme = useColorScheme();
  const initialWindow = useWindowDimensions();
  // Native rotation can publish window metrics before this navigation surface
  // has resized. Artwork and controls share the measured surface in one commit.
  const [surfaceSize, setSurfaceSize] = useState<{ width: number; height: number } | null>(null);
  const { width, height } = surfaceSize || initialWindow;
  const landscapePlayer = width > height && (Platform.OS === "ios" || (Platform.OS === "android" && height >= 600));
  const compactLandscape = landscapePlayer && height < 600;
  const androidTabletPortrait = Platform.OS === "android" && width >= 600 && height > width;
  const [sceneSmoke] = useState(() => (globalThis as any).expo?.modules?.SdaEngine?.sceneSmokeEnabled?.() === true);
  const [chromeSmoke] = useState(() => (globalThis as any).expo?.modules?.SdaGlassButton?.smokeStage?.() || "");
  const [page, setPage] = useState(sceneSmoke ? 2 : (chromeSmoke === "library" || chromeSmoke === "mini-player") ? 1 : 0);
  const [metadataOpen, setMetadataOpen] = useState(false);
  const [settings, setSettings] = useState(chromeSmoke.startsWith("settings"));
  useEffect(() => {
    if (Platform.OS !== "android" || !settings) return;
    const subscription = BackHandler.addEventListener("hardwareBackPress", () => { setSettings(false); return true; });
    return () => subscription.remove();
  }, [settings]);
  const [sceneVisited, setSceneVisited] = useState(sceneSmoke);
  const settingsScroll = useRef<ScrollView>(null);
  const volumeWidth = useRef(1);
  const volumeOrigin = useRef(0);
  const isLight = systemTheme !== "dark";
  const md3 = androidMaterialTheme(appTheme.id, !isLight);
  const c = Platform.OS === "android" ? { ...palettes[isLight ? "light" : "dark"], bg: md3.bg, panel: md3.panel, ink: md3.ink, muted: md3.muted, line: md3.line, field: md3.panel, accent: md3.primary } : { ...palettes[isLight ? "light" : "dark"], accent: appTheme.accent };
  const artworkSize = landscapePlayer ? height : width + 48; // Slight uniform zoom, never stretch the square.
  const pc = { ink: "#ffffff", muted: "#c9c9ce", line: "rgba(255,255,255,.24)", field: "rgba(255,255,255,.10)" };
  const [failedArtwork, setFailedArtwork] = useState("");
  const artworkUri = p.metadata.coverUri || "";
  const hasArtwork = !!artworkUri && failedArtwork !== artworkUri;
  const [artworkTone, setArtworkTone] = useState({ uri: "", color: "#303034" });
  const preparedArtworkColor = readArtworkColor(artworkUri, p.metadata.artworkColor);
  const fadeColor = preparedArtworkColor || (artworkTone.uri === artworkUri ? artworkTone.color : "#303034");
  useEffect(() => {
    let active = true;
    if (hasArtwork && !preparedArtworkColor) artworkColor(artworkUri).then(color => { if (active) setArtworkTone({ uri: artworkUri, color }); }).catch(() => { if (active) setArtworkTone({ uri: artworkUri, color: "#303034" }); });
    return () => { active = false; };
  }, [artworkUri, hasArtwork, preparedArtworkColor]);
  // Preserve the underlying player across the settings push/pop animation.
  const immersiveHome = page === 0;
  const homeBackground = immersiveHome ? "transparent" : c.bg;
  const homeTitle = page === 0 ? (p.busy || p.preparingAudio ? "正在准备音频…" : "正在播放") : IOS_TABS[page] || "正在播放";
  const title = p.metadata.title || p.fileName || "等待选择歌曲";
  const artist = p.metadata.artist || p.metadata.albumArtist || (p.selectedUri ? "未知艺人" : "打开音频，开始聆听");
  const engineUnavailable = p.error?.includes("SdaEngine native module is not registered") === true;
  const progress = p.durationMs > 0 ? Math.max(0, Math.min(1, p.positionMs / p.durationMs)) : 0;
  const layoutKey = landscapePlayer ? "landscape" : "portrait";
  const initialBlocks = { status: 0, info: 62, controls: 106, volume: 44 };
  const [playerMeasurements, setPlayerMeasurements] = useState<Partial<Record<typeof layoutKey, number>>>({});
  const [blockMeasurements, setBlockMeasurements] = useState<Partial<Record<typeof layoutKey, typeof initialBlocks>>>({});
  const playerHeight = playerMeasurements[layoutKey] ?? Math.max(0, height - (compactLandscape ? 140 : 230));
  const playerBlocks = blockMeasurements[layoutKey] || initialBlocks;
  const measuredBlockHeight = Object.values(playerBlocks).reduce((sum, value) => sum + value, 0);
  const { coverSize, gap: portraitGap } = iosPlayerLayout(width, playerHeight, measuredBlockHeight);
  const playerGap = landscapePlayer ? Math.max(0, Math.min(compactLandscape ? 12 : 32, (playerHeight - measuredBlockHeight - (compactLandscape ? 24 : 88)) / 4)) : portraitGap;
  const transportInset = compactLandscape ? 8 : 22;
  const iosLandscape = Platform.OS === "ios" && landscapePlayer;
  const landscapeSpacing = iosLandscape ? iosLandscapeControlLayout(Math.max(0, playerHeight - (compactLandscape ? 0 : 22)), measuredBlockHeight) : null;
  const transportMargin = landscapeSpacing?.transportGap ?? playerGap + transportInset;
  const measurePlayerBlock = (key: keyof typeof playerBlocks, value: number) => {
    setBlockMeasurements(previous => Math.abs((previous[layoutKey] || initialBlocks)[key] - value) < 0.5
      ? previous : { ...previous, [layoutKey]: { ...(previous[layoutKey] || initialBlocks), [key]: value } });
  };
  const format = p.sourceCodec === "alac" ? "立体声" : p.layout === "360RA-13" || p.systemSpatial360RAActive ? "360 Reality Audio" : /atmos/i.test(p.renderingStatus) || /^(eac3|e-ac-3|ac4|ac-4|truehd)$/i.test(p.sourceCodec) ? "杜比全景声" : previewMetadata ? "杜比全景声" : "音频信息";
  const infoRows: [string, string][] = __DEV__ && previewMetadata ? [
    ["数据来源", "Expo Go 模拟数据 · 仅开发预览"], ["文件名", "Preview.m4a"], ["标题", "示例歌曲"], ["艺人", "示例艺人"], ["专辑", "示例专辑"], ["容器", "M4A"], ["编码", "E-AC-3 JOC"], ["采样率", "48,000 Hz（模拟）"], ["音频格式", "杜比全景声"],
  ] : [
    ["文件名", p.fileName || "—"], ["标题", p.metadata.title || "未写入"], ["艺人", p.metadata.artist || p.metadata.albumArtist || "未写入"], ["专辑", p.metadata.album || "未写入"],
    ["年份", p.metadata.year || "暂未读取"], ["曲目", p.metadata.track || "暂未读取"], ["容器（文件扩展名）", p.fileName.split(".").length > 1 ? p.fileName.split(".").pop()!.toUpperCase() : "未知"],
    ["编码", p.sourceCodec || "未知"], ["时长", p.durationMs > 0 ? time(p.durationMs) : "未知"], ["双耳／系统输出声道数（非源声道）", String(p.outputChannels)], ["播放器渲染布局（非源声道）", p.layout], ["当前可视对象数（非全曲总数）", String(p.objects.length)],
    ["播放路径", p.systemSpatial360RAActive ? "系统空间音频" : "SDA / HRTFHQ"],
  ];
  const audioOptionsDisabled = p.systemSpatial360RAActive || p.busy;
  const presetDisabled = audioOptionsDisabled || p.roomBusy || p.nearFieldBusy;
  const currentPreset = renderingPresets.find(profile => isPresetSelected(p, profile));
  const playing = p.playing && !p.paused;
  const togglePlayback = () => p.playing ? p.togglePause() : p.play();
  const navigate = (index: number) => { setPage(index); if (index === 2) setSceneVisited(true); };
  useEffect(() => {
    if (p.playbackPageRequest) { setSettings(false); navigate(0); }
  }, [p.playbackPageRequest]);

  const button = (name: string, symbol: string, fallback: string, action: () => void, disabled = false, small = false) =>
    <Pressable accessibilityRole="button" accessibilityLabel={name} accessibilityState={{ disabled }} disabled={disabled} onPress={action} style={({ pressed }) => [small ? np.smallAction : np.action, { backgroundColor: small ? pc.field : "transparent", opacity: disabled ? .3 : pressed ? .55 : 1 }]}><Symbol name={symbol} fallback={fallback} size={small ? 18 : 22} color={pc.ink} /></Pressable>;
  const more = () => Alert.alert(title, "歌曲选项", [
    { text: "重新播放", onPress: p.play },
    { text: `播放模式：${PLAYBACK_MODE_LABELS[p.playbackMode]}`, onPress: () => p.setPlaybackMode(followingPlaybackMode(p.playbackMode)) },
    { text: "SDA 设置", onPress: () => setSettings(true) }, { text: "取消", style: "cancel" },
  ]);

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
  const label = (value: string, muted = false, style: object = {}) => <Text style={[{ color: immersiveHome && !settings ? (muted ? pc.muted : pc.ink) : (muted ? c.muted : c.ink) }, style]}>{value}</Text>;
  // The approved Expo Go transport controls are shared with the release app.
  // The presence of SdaEngine/SdaGlass must never select a different design.
  const icon = (symbol: string, name: string, action: () => void, disabled = false, size = 44, fallback = "•") => <Pressable accessibilityRole="button" accessibilityLabel={name} disabled={disabled} onPress={action} style={[s.icon, { width: size, height: size, backgroundColor: Platform.OS === "android" && immersiveHome && !settings ? "rgba(255,255,255,.14)" : c.panel, opacity: disabled ? .35 : 1 }]}>{Platform.OS === "android" ? <Symbol name={symbol} fallback={fallback} size={22} color={immersiveHome && !settings ? pc.ink : c.ink} /> : label(fallback, false, { fontSize: 22 })}</Pressable>;
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
  const miniPlayer = (environment?: "regular" | "inline") => !!p.selectedUri && <View style={[s.miniPlayer, environment ? s.nativeMiniPlayer : { backgroundColor: c.panel, borderColor: c.line }]}>
    <Pressable accessibilityRole="button" accessibilityLabel="展开正在播放" onPress={() => navigate(0)} style={s.miniSong}>{cover(environment === "inline" ? 24 : 32)}<View style={{ flex: 1 }}><Text numberOfLines={1} style={[s.miniTitle, { color: c.ink }]}>{title}</Text>{environment !== "inline" && <Text numberOfLines={1} style={[s.miniArtist, { color: c.muted }]}>{artist}</Text>}</View></Pressable>
    <Pressable accessibilityRole="button" accessibilityLabel={playing ? "暂停" : "播放"} accessibilityState={{ disabled: p.busy || !p.selectedUri }} disabled={p.busy || !p.selectedUri} onPress={togglePlayback} style={[s.miniPlayback, { opacity: p.busy ? .35 : 1 }]}>
      <IOSPlaybackSymbol playing={playing} color={c.accent} />
    </Pressable>
  </View>;
  const heading = (name: string, stage = "") => <View onLayout={event => {
    if (stage && chromeSmoke === stage) { const y = event.nativeEvent.layout.y; requestAnimationFrame(() => settingsScroll.current?.scrollTo({ y, animated: false })); }
  }}>{label(name, true, s.groupTitle)}</View>;

  return <SafeAreaProvider initialMetrics={initialWindowMetrics} style={{ flex: 1, backgroundColor: immersiveHome ? fadeColor : c.bg }}>
  <IOSMetadataSheet open={metadataOpen} onChange={setMetadataOpen} theme={isLight ? "light" : "dark"}>
    {Platform.OS === "android" ? <AndroidAudioInfo rows={infoRows} dark={!isLight} onClose={() => setMetadataOpen(false)} /> :
    <SafeAreaView style={{ flex: 1, backgroundColor: "transparent" }} edges={["top", "bottom"]}>
      <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", padding: 20 }}><Text accessibilityRole="header" style={{ color: PlatformColor("label"), fontSize: 20, fontWeight: "600" }}>歌曲元数据</Text><IOSMetadataDoneButton accent={c.accent} onPress={() => setMetadataOpen(false)} theme={isLight ? "light" : "dark"} /></View>
      <ScrollView contentContainerStyle={{ paddingHorizontal: 20, paddingBottom: 24 }}>{infoRows.map(([name, value]) => <View key={name} style={{ paddingVertical: 12, borderBottomWidth: StyleSheet.hairlineWidth, borderBottomColor: c.line }}><Text style={{ color: PlatformColor("label"), fontSize: 13, fontWeight: "600" }}>{name}</Text><Text selectable style={{ color: PlatformColor("label"), fontSize: 16, lineHeight: 23, marginTop: 6 }}>{value}</Text></View>)}</ScrollView>
    </SafeAreaView>}
  </IOSMetadataSheet>
  <IOSSettingsNavigation immersive={immersiveHome} settings={settings} onSettingsChange={setSettings} title={homeTitle} player={p} accent={c.accent} theme={isLight ? "light" : "dark"}>
  <View collapsable={false} style={[s.presentationSurface, { backgroundColor: immersiveHome ? fadeColor : c.bg }]} onLayout={event => {
    const { width, height } = event.nativeEvent.layout;
    if (width > 0 && height > 0) setSurfaceSize(previous => previous?.width === width && previous?.height === height ? previous : { width, height });
  }}>
  {immersiveHome && <View pointerEvents="none" accessible={false} accessibilityElementsHidden importantForAccessibility="no-hide-descendants" style={[StyleSheet.absoluteFill, { overflow: "hidden", backgroundColor: fadeColor }]}>
    {hasArtwork ? <Image key={artworkUri} source={{ uri: artworkUri }} resizeMode="contain" onError={() => setFailedArtwork(artworkUri)} style={{ position: "absolute", top: 0, left: landscapePlayer ? 0 : -24, width: artworkSize, height: artworkSize }} /> : <View style={{ position: "absolute", top: 0, left: landscapePlayer ? 0 : -24, width: artworkSize, height: artworkSize, backgroundColor: "#303034", alignItems: "center", justifyContent: "center" }} /> }
    <Image source={require("../assets/artwork-color-fade.png")} resizeMode="stretch" style={{ position: "absolute", top: artworkSize * .82, left: 0, width, height: artworkSize * .18, tintColor: fadeColor, opacity: landscapePlayer ? 0 : 1 }} />
    <Image source={require("../assets/artwork-color-fade-horizontal.png")} resizeMode="stretch" style={{ position: "absolute", left: artworkSize * .70, top: 0, width: artworkSize * .30, height: artworkSize, tintColor: fadeColor, opacity: landscapePlayer ? 1 : 0 }} />
  </View>}
  {/* Native tabs own the bottom inset. Reserve only the header/side insets
      here, so the tab bar's background reaches the home indicator. */}
  <SafeAreaView edges={hasSystemIOSTabs && Platform.OS === "ios" ? ["top", "left", "right"] : ["top", "bottom", "left", "right"]} style={[s.safe, { backgroundColor: Platform.OS === "android" && settings ? md3.bg : homeBackground }]}>
    <StatusBar translucent={Platform.OS === "android"} animated={false} barStyle={settings ? (isLight ? "dark-content" : "light-content") : immersiveHome ? "light-content" : isLight ? "dark-content" : "light-content"} backgroundColor={Platform.OS === "android" ? "transparent" : homeBackground} />
    <View style={[s.root, hasSystemIOSTabs && { paddingBottom: 0 }]}>
      <View style={{ flex: 1 }} accessibilityElementsHidden={settings} importantForAccessibility={settings ? "no-hide-descendants" : "auto"}>
      <View style={[s.header, compactLandscape && np.compactHeader, iosLandscape && np.iosLandscapeHeader]}>
        <View style={{ flex: 1 }}>{label("SDA", true, [s.eyebrow, { color: immersiveHome ? (Platform.OS === "android" ? pc.ink : pc.muted) : c.muted }])}{label(homeTitle, false, [s.pageTitle, { color: immersiveHome ? pc.ink : c.ink }])}</View>
        {Platform.OS === "ios" && (hasNativeIOSSettings ? <IOSHeadTrackingButton enabled={p.headTrackingEnabled}
          disabled={p.busy || p.systemSpatial360RAActive} accent={c.accent}
          onSelect={p.setHeadTracking} /> : <Pressable accessibilityRole="switch"
          accessibilityLabel="AirPods 头部追踪" accessibilityHint={p.headTrackingStatus}
          accessibilityState={{ checked: p.headTrackingEnabled, disabled: p.busy || p.systemSpatial360RAActive }}
          disabled={p.busy || p.systemSpatial360RAActive}
          onPress={() => p.setHeadTracking(!p.headTrackingEnabled)}
          style={({ pressed }) => ({ width: 44, height: 44, marginRight: 8, alignItems: "center", justifyContent: "center",
            opacity: p.busy || p.systemSpatial360RAActive ? .3 : pressed ? .55 : 1 })}>
          <Symbol name={p.headTrackingEnabled ? "person.crop.circle.fill" : "person.crop.circle"}
            fallback="HT" size={24} color={p.headTrackingEnabled ? c.accent : immersiveHome ? pc.ink : c.ink} />
        </Pressable>)}
        {Platform.OS === "android" ? <AndroidGlassSettingsButton dark={immersiveHome || !isLight} onPress={() => setSettings(true)} /> : hasNativeIOSSettings ? <IOSSettingsButton onPress={() => setSettings(true)} /> : icon("gearshape", "更多设置", () => setSettings(true), false, 44, "⚙")}
      </View>
      {/* Status belongs above the native tabs, never below their full-screen
          host, where it steals the home-indicator inset from the tab bar. */}
      {!!p.error && !engineUnavailable && <Text accessibilityRole="alert" style={s.error}>{p.error}</Text>}
      <IOSSystemTabs selected={page} onChange={navigate} player={p} accessory={miniPlayer()} nativeAccessory={p.selectedUri ? miniPlayer : undefined} backgroundColor="transparent" pageBackgroundColors={["transparent", c.bg, c.bg]} accent={c.accent} theme={isLight ? "light" : "dark"} fallback={<View style={s.tabs}>{hasNativeIOSChrome ? <IOSGlassTabs selected={page} onChange={navigate} /> : <View style={[s.fallbackTabs, { backgroundColor: c.panel, borderColor: c.line }]}>{IOS_TABS.map((name, index) => <Pressable key={name} accessibilityRole="tab" accessibilityState={{ selected: page === index }} onPress={() => navigate(index)} style={[s.fallbackTab, { backgroundColor: page === index ? c.field : "transparent" }]}>{label(name, page !== index, s.small)}</Pressable>)}</View>}</View>}>
        <View style={[s.page, np.root, !hasSystemIOSTabs && page !== 0 && s.hidden]}
          onLayout={event => { const available = event.nativeEvent.layout.height; if (available > 0) setPlayerMeasurements(previous => Math.abs((previous[layoutKey] ?? 0) - available) < 0.5 ? previous : { ...previous, [layoutKey]: available }); }}>
          <View style={[s.playerContent, { gap: playerGap, flex: 1, paddingHorizontal: 25 }, landscapePlayer && np.landscapeContent, compactLandscape && np.compactContent]}>
          <Pressable accessibilityRole="button" accessibilityLabel={p.metadata.coverUri ? (p.metadata.album || title) + " 专辑封面，轻点选择歌曲" : hasArtwork ? "临时预览图片，轻点选择歌曲" : "选择歌曲"} onPress={p.chooseFile} disabled={p.busy} style={[{ height: coverSize, flexShrink: 0, alignItems: "center", justifyContent: "center" }, landscapePlayer && { width: artworkSize, height: "100%" }]}>{!hasArtwork && <Symbol name="music.note" fallback="♫" size={110} color="#a0a0a5" />}</Pressable>
          <View style={[{ gap: iosLandscape ? 0 : playerGap, flexShrink: 0 }, androidTabletPortrait && np.tabletControls, landscapePlayer && np.landscapeControls, landscapeSpacing && { paddingTop: landscapeSpacing.topPadding }]}>
          <View style={[np.metadata, { transform: [{ translateY: landscapePlayer ? 0 : 36 }] }]} onLayout={event => measurePlayerBlock("info", event.nativeEvent.layout.height)}>
            <View style={{ flex: 1, paddingRight: 10 }}><Text numberOfLines={2} style={[np.title, compactLandscape && np.compactTitle, { color: pc.ink }]}>{title}</Text><Text numberOfLines={1} style={[np.artist, compactLandscape && np.compactArtist, { color: pc.muted }]}>{artist}</Text></View>
          </View>
          <View style={[s.playerBlock, np.controlsDown, landscapePlayer && np.landscapeBlock, landscapeSpacing && { marginTop: landscapeSpacing.metadataGap }]} onLayout={event => measurePlayerBlock("controls", Math.max(0, event.nativeEvent.layout.height - transportMargin))}>
          <View style={[np.infoProgressOffset, iosLandscape && np.landscapeBlock]}>
            <View accessibilityRole="progressbar" accessibilityLabel="播放进度（当前不支持拖动定位）" accessibilityValue={{ text: `${time(p.positionMs)} / ${p.durationMs > 0 ? time(p.durationMs) : "未知时长"}` }} style={[np.track, { backgroundColor: pc.line }]}><View style={[np.fill, { backgroundColor: pc.muted, width: `${progress * 100}%` }]} /></View>
            <View style={np.times}><Text style={[np.time, { color: pc.muted }]}>{time(p.positionMs)}</Text><Pressable accessibilityRole="button" accessibilityLabel={format + "，查看歌曲元数据"} onPress={() => setMetadataOpen(true)} hitSlop={10} style={{ minHeight: 32, justifyContent: "center", paddingHorizontal: 8 }}><Text style={[np.format, { color: pc.muted }]}>{format}</Text></Pressable><Text style={[np.time, { color: pc.muted }]}>{p.durationMs > 0 ? `−${time(p.durationMs - p.positionMs)}` : "−–:––"}</Text></View>
          </View>
          <View style={[np.transport, { marginTop: transportMargin }]}>
            <Pressable accessibilityRole="button" accessibilityLabel="上一曲" accessibilityState={{ disabled: p.busy || !p.queue.length }} disabled={p.busy || !p.queue.length} onPress={p.previous} style={({ pressed }) => [np.transportTouch, landscapePlayer && np.landscapeTransportTouch, compactLandscape && np.compactTransportTouch, { opacity: p.busy || !p.queue.length ? .3 : pressed ? .55 : 1 }]}><IOSSkipSymbol direction="previous" color={pc.ink} size={30} /></Pressable>
            <Pressable accessibilityRole="button" accessibilityLabel={playing ? "暂停" : "播放"} accessibilityState={{ disabled: p.busy || !p.selectedUri }} disabled={p.busy || !p.selectedUri} onPress={() => p.playing ? p.togglePause() : p.play()} style={({ pressed }) => [np.transportTouch, landscapePlayer && np.landscapeTransportTouch, compactLandscape && np.compactTransportTouch, { opacity: p.busy || !p.selectedUri ? .3 : pressed ? .55 : 1 }]}><IOSPlaybackSymbol playing={playing} color={pc.ink} size={38} /></Pressable>
            <Pressable accessibilityRole="button" accessibilityLabel="下一曲" accessibilityState={{ disabled: p.busy || !p.queue.length }} disabled={p.busy || !p.queue.length} onPress={p.next} style={({ pressed }) => [np.transportTouch, landscapePlayer && np.landscapeTransportTouch, compactLandscape && np.compactTransportTouch, { opacity: p.busy || !p.queue.length ? .3 : pressed ? .55 : 1 }]}><IOSSkipSymbol direction="next" color={pc.ink} size={30} /></Pressable>
          </View>
          </View>
          <View style={[np.volume, np.controlsDown, landscapePlayer && np.landscapeBlock, landscapeSpacing && { marginTop: landscapeSpacing.volumeGap }]} onLayout={event => measurePlayerBlock("volume", event.nativeEvent.layout.height)}>
            <Symbol name="speaker.fill" fallback="◂" size={13} color={pc.muted} />
            <View accessible accessibilityRole="adjustable" accessibilityLabel="音量滑块" accessibilityValue={{ min: 0, max: 100, now: Math.round(p.volume * 100) }} accessibilityActions={[{ name: "increment" }, { name: "decrement" }]} onAccessibilityAction={e => p.setVolume(Math.max(0, Math.min(1, p.volume + (e.nativeEvent.actionName === "increment" ? .05 : -.05))))} {...volumeGesture.panHandlers} onLayout={e => { volumeWidth.current = Math.max(1, e.nativeEvent.layout.width); }} style={np.volumeTouch}><View style={[np.track, { backgroundColor: pc.line }]}><View style={[np.fill, { backgroundColor: pc.muted, width: `${Math.max(0, Math.min(1, p.volume)) * 100}%` }]} /></View></View>
            <Symbol name="speaker.wave.2.fill" fallback="▸" size={18} color={pc.muted} />
          </View>
          <View style={s.playerBlock} onLayout={event => measurePlayerBlock("status", event.nativeEvent.layout.height)}>
          </View>
          </View>
          </View>
        </View>
        <ScrollView style={[s.page, s.libraryViewport, !hasSystemIOSTabs && page !== 1 && s.hidden]} contentContainerStyle={[s.otherContent, s.libraryContent]}
          scrollEnabled bounces={false} alwaysBounceVertical={false} showsVerticalScrollIndicator
          indicatorStyle={isLight ? "black" : "white"} persistentScrollbar={false}
          automaticallyAdjustsScrollIndicatorInsets={false} scrollIndicatorInsets={{ top: 10, bottom: 10, right: 3, left: 0 }}
          contentInsetAdjustmentBehavior="automatic">
          {label("音乐留在你的设备上", true, s.lead)}
          <View style={[s.group, { backgroundColor: c.panel, borderColor: c.line }]}>{actionRow("folder.open", "打开本机文件", "从「文件」选择音乐", p.chooseFile, p.busy)}</View>
          <View style={s.libraryPlaybackModes}>
            {(["sequence", "repeat-all", "repeat-one"] as const).map(mode => {
              const selected = p.playbackMode === mode;
              return <Pressable key={mode} accessibilityRole="button" accessibilityLabel={PLAYBACK_MODE_LABELS[mode]} accessibilityState={{ selected }} onPress={() => p.setPlaybackMode(mode)}
                style={({ pressed }) => [s.loopPill, { backgroundColor: selected ? c.accent : c.field, opacity: pressed ? .65 : 1 }]}>
                <Symbol name={mode === "sequence" ? "list.bullet" : mode === "repeat-one" ? "repeat.1" : "repeat"} fallback={mode === "sequence" ? "≡" : mode === "repeat-one" ? "↻1" : "↻"} size={22} theme={selected ? "dark" : isLight ? "light" : "dark"} color={selected ? "#ffffff" : c.ink} />
              </Pressable>;
            })}
          </View>
          <View style={[s.row, { marginTop: 28, marginBottom: 8 }]}>{label("播放列表", false, s.sectionTitle)}{label(p.queue.length + " 首", true, s.small)}</View>
          {p.queue.map((track, index) => <Pressable key={track.contentHash} accessibilityRole="button" accessibilityLabel={`${trackTitle(track)}${track.preparation ? `，${track.preparation.stage}` : ""}`} accessibilityState={{ selected: index === p.queueIndex, disabled: p.busy || !!track.preparation, busy: track.preparation?.state === "loading" }} disabled={p.busy || !!track.preparation} onPress={() => { p.selectTrack(index); navigate(0); }} style={[s.queueRow, { borderBottomColor: c.line, overflow: "hidden" }]}>
            {track.metadata.coverUri ? <Image source={{ uri: track.metadata.coverUri }} resizeMode="cover" style={s.queueCover} /> : <View style={[s.queueCover, s.emptyCover, { backgroundColor: c.field }]}>{label("♫", true, { fontSize: 23 })}</View>}
            <View style={{ flex: 1 }}><Text numberOfLines={2} style={[s.queueTitle, { color: c.ink }]}>{trackTitle(track)}</Text><Text numberOfLines={1} style={[s.queueArtist, { color: c.muted }]}>{track.metadata.artist || track.metadata.albumArtist || "本机文件"}{track.metadata.album ? " · " + track.metadata.album : ""}</Text></View>
            {label(index === p.queueIndex ? "●" : "›", index !== p.queueIndex, { color: index === p.queueIndex ? c.accent : c.muted })}
            <QueuePreparationOverlay preparation={track.preparation} dark={!isLight} accent={c.accent} />
          </Pressable>)}
          {!p.queue.length && <View style={s.emptyLibrary}>{label("还没有歌曲", false, s.sectionTitle)}{label("打开本机文件，添加到播放列表", true, s.lead)}</View>}
          {label("本机播放，不上传音乐，也不改变源文件。", true, s.libraryHint)}
        </ScrollView>
        <View style={[s.page, s.otherContent, compactLandscape && s.compactScenePage, !hasSystemIOSTabs && page !== 2 && s.hidden]}>
          <View style={s.row}>{label(format, true, s.small)}{label(p.layout === "360RA-13" ? "360° 球形声场" : p.layout, true, s.small)}</View>
          <View style={[s.scene, s.sceneViewport, compactLandscape && s.compactSceneViewport]}>{sceneVisited && <MobileObjectScene layout={p.layout} objects={p.objects} active={page === 2 && !settings} />}</View>
          <View style={[s.row, s.sceneInfo, compactLandscape && s.compactSceneInfo]}><View>{label(p.systemSpatial360RAActive ? "系统空间音频" : "HRTFHQ", false, s.settingTitle)}{label(p.systemSpatial360RAActive ? (p.outputChannels === 2 ? "2.0 系统输出" : "7.1.4 系统输出") : currentPreset?.label || "SDA 空间渲染", true, s.small)}</View><View style={{ alignItems: "flex-end" }}>{label(p.objects.length + " 个对象", false, s.settingTitle)}{label("实时位置", true, s.small)}</View></View>
          {label("单指旋转 · 双指缩放", true, [s.sceneHint, compactLandscape && s.compactSceneHint])}
        </View>
      </IOSSystemTabs>

      </View>
      {settings && Platform.OS === "android" && <AndroidSettings player={p} dark={!isLight} onClose={() => setSettings(false)} />}
      {settings && !hasNativeIOSSettings && Platform.OS !== "android" && <View style={[s.settingsPage, { backgroundColor: c.bg }]} accessibilityViewIsModal>
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
          {Platform.OS === "ios" && <>{heading("声音增强")}
          <View style={[s.group, { backgroundColor: c.panel, borderColor: c.line }]}>
            {actionRow("scope", "头部方向归中", "", p.resetYaw, !p.headTrackingEnabled || p.systemSpatial360RAActive)}
            {divider()}
            {toggle("空间平衡", "按空间位置调整辅助层（最多 +1.5 dB），并做轻微高频补偿，仅对 SDA 共享渲染生效", p.spatialEnhancementEnabled, p.setSpatialEnhancement, p.busy || p.systemSpatial360RAActive)}
          </View>
          {heading("360 Reality Audio")}
          <View style={[s.group, { backgroundColor: c.panel, borderColor: c.line }]}>{toggle("系统空间音频 · 7.1.4", "仅 360RA：12 声道交给系统，旁路 KU100 直达渲染", p.systemSpatial360RA, p.setSystemSpatial360RA, p.busy)}</View>
          {label("修改后下一次播放生效，不中断当前歌曲。关闭后恢复 SDA / HRTFHQ 空间渲染，并非普通立体声下混。", true, s.groupHint)}
          {heading("ALAC 立体声")}
          <View style={[s.group, { backgroundColor: c.panel, borderColor: c.line }]}>
            {toggle("立体声上混 · 7.1.4", "由立体声生成环绕和高度声道，不是原生 Atmos 或独立对象", p.alacStereoUpmix, p.setAlacStereoUpmix, p.busy)}{divider()}
            {toggle("立体声系统空间音频", "关闭上混时提交 2.0；开启上混时提交 7.1.4，旁路 KU100", p.systemSpatialStereo, p.setSystemSpatialStereo, p.busy)}
          </View>
          {label("上混播放中平滑切换，已缓冲音频播完后生效；系统输出开关下次播放生效。系统空间效果取决于输出设备及系统设置。", true, s.groupHint)}
          </>}
          {heading("空间渲染", "settings-spatial")}
          <View style={[s.group, { backgroundColor: c.panel, borderColor: c.line }]}>
            {renderingPresets.map(profile => <View key={profile.id}>{actionRow(isPresetSelected(p, profile) ? "checkmark.circle.fill" : "circle", profile.label, profile.description, () => p.setRenderingPreset(profile.id), presetDisabled, isPresetSelected(p, profile))}{divider()}</View>)}
            {toggle("逐对象渲染", "每个对象独立生成双耳声音", p.directObjects, value => p.setRendering(value, p.directionalObjects), audioOptionsDisabled)}
            {divider()}
            {toggle("实际方向", "按对象真实位置定位声音", p.directionalObjects, value => p.setRendering(p.directObjects, value), audioOptionsDisabled)}
          </View>
          {label("切换预设保留播放进度与播放 / 暂停状态；加载时可能短暂缓冲。", true, s.groupHint)}
          {heading("主题色")}
          <View style={[s.group, { backgroundColor: c.panel }]}>{CHARACTER_THEMES.map(t => actionRow("paintpalette", t.name, t.color, () => appTheme.setTheme(t.id), false, appTheme.id === t.id))}</View>
          {heading("应用与输出")}
          <View style={[s.group, { backgroundColor: c.panel, borderColor: c.line }]}>
            <View style={s.settingRow}>{label("外观", false, s.settingTitle)}{label("跟随系统", true, s.small)}</View>{divider()}
            <View style={s.settingRow}><View style={s.settingCopy}>{label("音频输出", false, s.settingTitle)}{label(p.playing ? p.renderingStatus : "等待播放", true, s.settingHint)}{label(p.systemSpatial360RAActive && p.outputChannels === 12 ? "48 kHz · 浮点 PCM · 7.1.4" : "48 kHz · 浮点 PCM · 双声道", true, s.settingHint)}</View></View>{divider()}
            {actionRow("info.circle", "关于 SDA", "Spatial Decoder App", () => Alert.alert("SDA", "Spatial Decoder App\n本机空间音频播放器"))}
          </View>

        </ScrollView>
      </View>}
    </View>
  </SafeAreaView>
  </View>
  </IOSSettingsNavigation>
  </SafeAreaProvider>;
}

const s = StyleSheet.create({
  libraryViewport: { marginHorizontal: -18 },
  libraryContent: { paddingHorizontal: 18 },
  libraryPlaybackModes: { flexDirection: "row", alignItems: "center", gap: 12, marginTop: 20 },
  loopPill: { flex: 1, minHeight: 48, borderRadius: 24, alignItems: "center", justifyContent: "center" },
  engineNotice: { fontSize: 12, lineHeight: 18, marginBottom: 12 },
  presentationSurface: { flex: 1, overflow: "hidden" },
  safe: { flex: 1 }, root: { flex: 1, paddingHorizontal: 18, paddingBottom: 8 },
  header: { flexDirection: "row", alignItems: "center", paddingVertical: 13, paddingHorizontal: 3 },
  eyebrow: { fontSize: 11, letterSpacing: 3 }, pageTitle: { fontSize: 24, fontWeight: "600", marginTop: 5 },
  pages: { flex: 1 }, page: { flex: 1 }, hidden: { display: "none" },
  playerContent: { paddingHorizontal: 7, paddingTop: 8, paddingBottom: 14, justifyContent: "flex-start" }, playerBlock: { flexShrink: 0 }, otherContent: { paddingTop: 8, paddingBottom: 20 },
  row: { flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }, small: { fontSize: 11, lineHeight: 17 },
  formatBadge: { paddingHorizontal: 10, paddingVertical: 4, borderRadius: 12 },
  playerActions: { flexDirection: "row", alignItems: "center", justifyContent: "space-between", paddingHorizontal: 8, marginTop: 8 },
  transportButton: { width: 60, height: 60, alignItems: "center", justifyContent: "center" },
  primaryTransport: { width: 80, height: 68 },
  album: { alignSelf: "center", flexShrink: 0, shadowColor: "#000", shadowOpacity: .2, shadowRadius: 24, shadowOffset: { width: 0, height: 12 } },
  emptyCover: { alignItems: "center", justifyContent: "center" }, trackTitle: { fontSize: 22, fontWeight: "600", textAlign: "left", lineHeight: 29 },
  trackSubtitle: { fontSize: 14, textAlign: "left", marginTop: 5, lineHeight: 20 },
  progress: { height: 5, borderRadius: 8, overflow: "hidden", marginTop: 4 }, times: { flexDirection: "row", justifyContent: "space-between", marginTop: 8 },
  transport: { flexDirection: "row", justifyContent: "center", alignItems: "center", gap: 30, marginTop: 12 }, icon: { borderRadius: 32, alignItems: "center", justifyContent: "center" },
  volume: { flexShrink: 0, flexDirection: "row", alignItems: "center", gap: 10 }, volumeValue: { fontSize: 11, minWidth: 31, textAlign: "right", fontVariant: ["tabular-nums"] }, volumeTouch: { flex: 1, height: 44, justifyContent: "center" },
  lead: { fontSize: 13, lineHeight: 21, marginBottom: 22 }, group: { borderRadius: 15, borderWidth: StyleSheet.hairlineWidth, paddingHorizontal: 12, overflow: "hidden" },
  sectionTitle: { fontSize: 18, fontWeight: "600" }, queueRow: { flexDirection: "row", alignItems: "center", gap: 12, paddingVertical: 15, borderBottomWidth: StyleSheet.hairlineWidth },
  queueCover: { width: 49, height: 49, borderRadius: 8 }, queueTitle: { fontSize: 15, lineHeight: 21 }, queueArtist: { fontSize: 12, marginTop: 5 },
  emptyLibrary: { paddingVertical: 35, gap: 10 }, libraryHint: { fontSize: 12, lineHeight: 20, marginTop: 28 },
  nativeMiniPlayer: { flex: 1, borderWidth: 0, borderRadius: 0, backgroundColor: "transparent", paddingHorizontal: 12, paddingVertical: 4 },
  miniPlayer: { flexDirection: "row", alignItems: "center", gap: 4, borderRadius: 16, borderWidth: StyleSheet.hairlineWidth, padding: 8 },
  miniPlayback: { width: 44, height: 44, alignItems: "center", justifyContent: "center", backgroundColor: "transparent" },
  miniSong: { flex: 1, flexDirection: "row", alignItems: "center", gap: 10 }, miniTitle: { fontSize: 13, lineHeight: 18 }, miniArtist: { fontSize: 11, marginTop: 3 },
  tabs: { marginTop: 10 }, fallbackTabs: { flexDirection: "row", padding: 5, borderWidth: StyleSheet.hairlineWidth, borderRadius: 26 }, fallbackTab: { flex: 1, minHeight: 50, alignItems: "center", justifyContent: "center", borderRadius: 22 },
  sceneViewport: { flex: 1, minHeight: 0, maxHeight: 410 },
  compactScenePage: { paddingTop: 4, paddingBottom: 6 },
  compactSceneViewport: { marginTop: 8 },
  compactSceneInfo: { marginTop: 8 },
  compactSceneHint: { marginTop: 6 },
  scene: { marginTop: 17, borderRadius: 18, overflow: "hidden", backgroundColor: "#171a19" }, sceneInfo: { marginTop: 18, paddingHorizontal: 4 }, sceneHint: { textAlign: "center", fontSize: 12, marginTop: 18 },
  settingsPage: { position: "absolute", top: 0, right: 0, bottom: 0, left: 0, zIndex: 10 }, settingsHeader: { flexDirection: "row", alignItems: "center", justifyContent: "space-between", paddingHorizontal: 8, paddingVertical: 8, overflow: "hidden" }, settingsTitle: { fontSize: 18, fontWeight: "600" },
  settingsContent: { paddingHorizontal: 3, paddingBottom: 28 }, groupTitle: { fontSize: 12, marginTop: 25, marginBottom: 8, marginLeft: 12 },
  settingRow: { flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 12, paddingVertical: 13, minHeight: 54 }, settingCopy: { flex: 1 }, settingTitle: { fontSize: 15, lineHeight: 22 }, settingHint: { fontSize: 12, lineHeight: 19, marginTop: 4 }, groupHint: { fontSize: 12, lineHeight: 19, marginTop: 9, paddingHorizontal: 12 },
  divider: { height: StyleSheet.hairlineWidth }, settingsActions: { flexDirection: "row", justifyContent: "center", gap: 24, marginTop: 25 }, error: { color: "#bc483a", fontSize: 12, lineHeight: 18, marginTop: 7 },
});

const np = StyleSheet.create({
  // Absorb unused artwork space above the whole control group on tall tablets.
  tabletControls: { marginTop: "auto" },
  iosLandscapeHeader: { marginTop: 14 },
  compactHeader: { paddingVertical: 2 },
  compactContent: { paddingTop: 0, paddingBottom: 0 },
  compactTitle: { fontSize: 18, lineHeight: 22 },
  compactArtist: { fontSize: 15, lineHeight: 20 },
  compactTransportTouch: { height: 48 },
  landscapeContent: { flexDirection: "row", paddingHorizontal: 0, gap: 24 },
  landscapeControls: { flex: 1, minWidth: 0, justifyContent: "center", paddingRight: 25 },
  landscapeBlock: { transform: [{ translateY: 0 }] },
  landscapeTransportTouch: { flex: 1, width: 0, minWidth: 48, maxWidth: 76 },
  controlsDown: { transform: [{ translateY: 24 }] },
  infoProgressOffset: { transform: [{ translateY: 12 }] },
  root: { flex: 1, backgroundColor: "transparent", marginHorizontal: -18, overflow: "hidden" },
  artwork: { position: "absolute", top: 0, left: 0, width: "100%" },
  defaultArtwork: { position: "absolute", top: 0, left: 0, width: "100%", backgroundColor: "#303034", alignItems: "center", justifyContent: "center" },
  cover: { flexShrink: 0, alignSelf: "center", shadowColor: "#000", shadowOffset: { width: 0, height: 14 }, shadowOpacity: .26, shadowRadius: 22 },
  coverImage: { width: "100%", height: "100%", borderRadius: 9 }, emptyCover: { backgroundColor: "transparent", alignItems: "center", justifyContent: "center" },
  metadata: { flexShrink: 0, flexDirection: "row", alignItems: "center", gap: 8 }, title: { color: "#fff", fontSize: 20, fontWeight: "600", lineHeight: 26 }, artist: { color: "rgba(255,255,255,.58)", fontSize: 19, marginTop: 2 },
  action: { width: 48, height: 44, alignItems: "center", justifyContent: "center" }, smallAction: { width: 32, height: 32, borderRadius: 16, backgroundColor: "transparent", alignItems: "center", justifyContent: "center" },
  track: { height: 5, backgroundColor: "rgba(255,255,255,.22)", borderRadius: 4, overflow: "hidden" }, fill: { height: 5, backgroundColor: "rgba(255,255,255,.68)", borderRadius: 4 },
  times: { flexDirection: "row", justifyContent: "space-between", alignItems: "center", marginTop: 7 }, time: { fontSize: 11, color: "rgba(255,255,255,.58)", fontVariant: ["tabular-nums"] }, format: { color: "rgba(255,255,255,.58)", fontSize: 11 },
  transport: { flexDirection: "row", alignItems: "center", justifyContent: "space-evenly" }, transportTouch: { width: 76, height: 68, alignItems: "center", justifyContent: "center" },
  volume: { flexShrink: 0, flexDirection: "row", alignItems: "center", gap: 9 }, volumeTouch: { flex: 1, height: 44, justifyContent: "center" },
  footer: { flexDirection: "row", alignItems: "center", justifyContent: "space-around", marginTop: 8 }, notice: { color: "rgba(255,255,255,.58)", fontSize: 11, textAlign: "center", marginTop: 6 },
});
