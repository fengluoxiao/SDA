import { spatialHeadIndicator } from "./spatial-head-indicator";
import { IOSNativePage, hasNativePageHeader } from "./IOSNativePage";
import { useNavigationPreferences } from "./navigation-preferences";
import { CHARACTER_THEMES, useAppTheme, validTheme } from "./app-theme";
import React, { useState, useEffect } from "react";
import { Alert, Modal, Platform, PlatformColor, StyleSheet, View, useWindowDimensions } from "react-native";
import { swiftUI } from "./IOSSystemTabs";
import type { PlayerProps } from "./RemotePlayer";
import renderingPresets from "../rendering-presets.json";
import sharedRenderProfile from "../ios-experiments/active.json";
import { isPresetSelected } from "./ios-ui-model";
import { PLAYBACK_MODES, PLAYBACK_MODE_LABELS } from "../../web/src/playbackOrder";

const hasView = (name: string) => !!(globalThis as any).expo?.getViewConfig?.("ExpoUI", name);
export const hasNativeIOSSettings = !!swiftUI && ["HostView", "FormView", "SectionView", "ToggleView", "PickerView", "TextView", "Button", "HStackView", "VStackView"].every(hasView);

// Use the same UIKit settings stack in Expo Go and the standalone app.
// The transition stack hides its own bar; native pages own their headers.
const hasUIKitSettingsNavigation = hasNativePageHeader;
const nativeScreens: typeof import("react-native-screens") | null =
  hasUIKitSettingsNavigation ? require("react-native-screens") : null;

// Settings uses one inline system title. Never add a second large title
// inside the hosted Form; keep the same header in Expo Go and the app.

const settingsButtonStyle = Platform.OS === "ios" && Number(Platform.Version) >= 26 ? "glass" : "bordered";

const playerActionIconSize = 22;

export function IOSSettingsButton({ onPress, sideDock = false }: { onPress(): void; sideDock?: boolean }) {
  if (!swiftUI || !hasView("Button")) return null;
  const { Host, Button } = swiftUI;
  const { accessibilityLabel, labelStyle, buttonStyle, buttonBorderShape, font } = require("@expo/ui/swift-ui/modifiers") as typeof import("@expo/ui/swift-ui/modifiers");
  const image = hasView("ImageView") ? <swiftUI.Image systemName="gearshape" size={playerActionIconSize} /> : undefined;
  return <Host ignoreSafeArea={sideDock ? "container" : undefined} style={{ width: 44, height: 44 }}>
    <Button label={image ? undefined : "更多设置"} systemImage={image ? undefined : "gearshape"} onPress={onPress}
      modifiers={[labelStyle("iconOnly"), ...(!image ? [font({ size: playerActionIconSize })] : []), buttonStyle(settingsButtonStyle), buttonBorderShape("circle"), accessibilityLabel("更多设置")]}>{image}</Button>
  </Host>;
}

function IOSSettingsBackButton({ onPress }: { onPress(): void }) {
  if (!swiftUI) return null;
  const { Host, Button } = swiftUI;
  const { labelStyle, buttonStyle, buttonBorderShape, accessibilityLabel, controlSize, font, frame } = require("@expo/ui/swift-ui/modifiers") as typeof import("@expo/ui/swift-ui/modifiers");
  return <Host ignoreSafeArea="container" style={{ width: 44, height: 44 }}>
    <Button label="返回" systemImage="chevron.left" onPress={onPress} modifiers={[
      labelStyle("iconOnly"), controlSize("large"), font({ size: 22, weight: "semibold" }),
      buttonStyle(settingsButtonStyle), buttonBorderShape("circle"), frame({ width: 44, height: 44 }), accessibilityLabel("返回播放器"),
    ]} />
  </Host>;
}

export function IOSHeadTrackingButton({ enabled, disabled: isDisabled, accent, onSelect, onDisable, spatialEnabled = true, sideDock = false, headYaw = 0, inactiveColor = "#ffffff" }: {
  enabled: boolean; spatialEnabled?: boolean; onDisable?(): void; disabled: boolean; accent: string; onSelect(enabled: boolean): void; sideDock?: boolean; headYaw?: number; inactiveColor?: string;
}) {
  if (!swiftUI || !hasView("Button")) return null;
  const { Host, Menu, Picker, Text } = swiftUI;
  const { accessibilityLabel, labelStyle, buttonStyle, buttonBorderShape, disabled, tint, tag, pickerStyle, rotation3DEffect, font } = require("@expo/ui/swift-ui/modifiers") as typeof import("@expo/ui/swift-ui/modifiers");
  const indicator = spatialHeadIndicator(spatialEnabled, enabled, headYaw, accent, inactiveColor);
  const label = indicator.label;
  const image = hasView("ImageView") ? <swiftUI.Image systemName={indicator.symbol} size={playerActionIconSize} color={indicator.color}
    modifiers={[rotation3DEffect({ angle: indicator.poseDegrees, axis: { y: 1 }, perspective: .35 })]} /> : null;
  const modifiers = [labelStyle("iconOnly"), ...(!image ? [font({ size: playerActionIconSize })] : []), buttonStyle(settingsButtonStyle), buttonBorderShape("circle"),
    accessibilityLabel(label), disabled(isDisabled), tint(indicator.color)];
  if (!hasView("MenuView")) return <Host ignoreSafeArea={sideDock ? "container" : undefined} style={{ width: 44, height: 44, marginRight: sideDock ? 0 : 8 }}>
    <swiftUI.Button label={image ? undefined : label} systemImage={image ? undefined : indicator.symbol} onPress={() => Alert.alert("空间音频", "", [
      ...(onDisable ? [{ text: "关闭", onPress: onDisable }] : []),
      { text: "固定", onPress: () => onSelect(false) }, { text: "头部追踪", onPress: () => onSelect(true) }, { text: "取消", style: "cancel" },
    ])} modifiers={modifiers}>{image || undefined}</swiftUI.Button>
  </Host>;
  return <Host ignoreSafeArea={sideDock ? "container" : undefined} style={{ width: 44, height: 44, marginRight: sideDock ? 0 : 8 }}>
    <Menu label={image || label} systemImage={image ? undefined : indicator.symbol} modifiers={modifiers}>
      <Picker label="空间音频" selection={!spatialEnabled ? "off" : enabled ? "tracking" : "fixed"}
        onSelectionChange={(value: string) => value === "off" ? onDisable?.() : onSelect(value === "tracking")} modifiers={[pickerStyle("inline")]}>
        {!!onDisable && <Text modifiers={[tag("off")]}>关闭</Text>}
        <Text modifiers={[tag("fixed")]}>固定</Text>
        <Text modifiers={[tag("tracking")]}>头部追踪</Text>
      </Picker>
    </Menu>
  </Host>;
}

// A single native navigation stack stays mounted over the existing player.
// Its controlled path changes only presentation, never the audio engine.
export function IOSSettingsNavigation({ children, settings, onSettingsChange, title, player, accent, theme, immersive = false }: {
  children: React.ReactElement; settings: boolean; onSettingsChange(value: boolean): void;
  title: string; player: PlayerProps; accent: string; theme: "light" | "dark"; immersive?: boolean;
}) {
  if (!swiftUI || !hasNativeIOSSettings) return children;
  if (nativeScreens) {
    const { ScreenStack, ScreenStackItem } = nativeScreens;
    const { Host } = swiftUI;
    if (__DEV__) console.info("[SDA UI] settings navigation: native page-owned UINavigationBar");
    return <ScreenStack style={{ flex: 1 }}>
      {/* NativeStack forbids decreasing activityState; UIKit owns push/pop visibility.
          Stack screens overlap at full size; flex siblings would split the viewport. */}
      <ScreenStackItem screenId="sda-home" activityState={2} freezeOnBlur={false}
        headerConfig={{ hidden: true, title }} contentStyle={{ flex: 1, backgroundColor: immersive ? "transparent" : PlatformColor("systemGroupedBackground") }} style={StyleSheet.absoluteFill}>
        {children}
      </ScreenStackItem>
      {settings && <ScreenStackItem screenId="sda-settings" activityState={2} stackPresentation="push"
        onDismissed={() => onSettingsChange(false)}
        headerConfig={{ hidden: true }}
        contentStyle={{ flex: 1, backgroundColor: PlatformColor("systemGroupedBackground") }} style={StyleSheet.absoluteFill}>
        <IOSNativePage screenId="sda-settings-content" title="设置" theme={theme} accent={accent}
          headerLeft={<IOSSettingsBackButton onPress={() => onSettingsChange(false)} />}>
          <Host style={{ flex: 1 }} colorScheme={theme}>
            <NativeSettingsForm player={player} onClose={() => onSettingsChange(false)} />
          </Host>
        </IOSNativePage>
      </ScreenStackItem>}
    </ScreenStack>;
  }
  return <>{children}<Modal visible={settings} animationType="slide" presentationStyle="fullScreen" onRequestClose={() => onSettingsChange(false)}>
    <IOSSettingsCompatibilityPage player={player} theme={theme} onClose={() => onSettingsChange(false)} />
  </Modal></>;
}

// Compatibility only for binaries without the UIKit navigation primitives.
// Keep the compatibility navigation row full-width with a balanced trailing
// spacer, so the title stays centered and the back button stays at the left.
function IOSSettingsCompatibilityPage({ player, theme, onClose }: {
  player: PlayerProps; theme: "light" | "dark"; onClose(): void;
}) {
  const appTheme = useAppTheme();
  const { width } = useWindowDimensions();
  if (!swiftUI) return null;
  const { Host, VStack, HStack, Button, Text } = swiftUI;
  const { frame, padding, font, buttonStyle, buttonBorderShape, labelStyle, controlSize, tint, accessibilityLabel, background } = require("@expo/ui/swift-ui/modifiers") as typeof import("@expo/ui/swift-ui/modifiers");
  // One full-screen host owns the safe area. Only the background extends
  // behind system chrome; controls retain SwiftUI's safe-area positioning.
  // Splitting this into RN SafeAreaView + two hosts creates visible edge bands.
  return <View style={{ flex: 1, backgroundColor: PlatformColor("systemGroupedBackground") }}>
    <Host style={{ flex: 1 }} colorScheme={theme}>
      <VStack alignment="leading" spacing={0} modifiers={[
        frame({ maxWidth: width, alignment: "leading" }),
        background(PlatformColor("systemGroupedBackground"), { ignoresSafeAreaEdges: "all" }),
      ]}>
        <HStack spacing={0} modifiers={[padding({ horizontal: 16 }), frame({ maxWidth: width, height: 52 })]}>
          <Button label="返回" systemImage="chevron.left" onPress={onClose} modifiers={[
            buttonStyle(settingsButtonStyle), buttonBorderShape("circle"), labelStyle("iconOnly"), controlSize("large"), tint(appTheme.accent),
            font({ size: 22, weight: "semibold" }), frame({ width: 44, minHeight: 44 }),
            frame({ width: 88, alignment: "leading" }), accessibilityLabel("返回播放器"),
          ]} />
          <Text modifiers={[font({ textStyle: "headline", weight: "semibold" }), frame({ maxWidth: width })]}>设置</Text>
          <Text modifiers={[frame({ width: 88 }), accessibilityLabel("")]}>{""}</Text>
        </HStack>
        <NativeSettingsForm player={player} onClose={onClose} />
      </VStack>
    </Host>
  </View>;
}

function NativeSettingsForm({ player: p, onClose }: { player: PlayerProps; onClose(): void }) {
  const appTheme = useAppTheme();
  const navigationPreferences = useNavigationPreferences();
  const { width } = useWindowDimensions();
  if (!swiftUI) return null;
  const { Form, Section, Toggle, Picker, Text, Button, HStack } = swiftUI;
  const { disabled, tag, pickerStyle, tint, scrollContentBackground, background, frame, buttonStyle } = require("@expo/ui/swift-ui/modifiers") as typeof import("@expo/ui/swift-ui/modifiers");
  const audioDisabled = p.systemSpatial360RAActive || p.busy;
  const presetDisabled = audioDisabled || p.roomBusy || p.nearFieldBusy;
  const preset = renderingPresets.find(profile => isPresetSelected(p, profile));
  return <Form modifiers={[
    // The header owns the only title; the Form contains settings rows only.
    tint(appTheme.accent),
    scrollContentBackground("hidden"),
    // The native navigation bar owns the scroll-edge appearance in both orientations.
    background(PlatformColor("systemGroupedBackground"), { ignoresSafeAreaEdges: "all" }),
  ]}>
    <Section title="播放操作">
      <HStack spacing={12}>
        <Button label="重新播放" systemImage="arrow.counterclockwise" onPress={() => { onClose(); p.play(); }} modifiers={[buttonStyle("borderless"), frame({ maxWidth: width, minHeight: 44 }), disabled(p.busy || !p.selectedUri)]} />
        <Button label="停止播放" systemImage="stop.fill" role="destructive" onPress={() => { onClose(); p.stop(); }} modifiers={[buttonStyle("borderless"), frame({ maxWidth: width, minHeight: 44 }), disabled(p.busy || !p.playing)]} />
      </HStack>
    </Section>
    <Section title="播放" footer={<Text>双声道 / 360RA / Audio Vivid 响度平衡，只衰减不增益。Audio Vivid 起播前用短缓冲估计初始响度，播放中继续更新，不扫描整首歌曲。</Text>}>
      <Toggle label="音量平衡" isOn={p.volumeBalanceEnabled} onIsOnChange={p.setVolumeBalance} modifiers={[tint(appTheme.accent), disabled(p.busy)]} />
      <Picker label="播放模式" selection={p.playbackMode} onSelectionChange={p.setPlaybackMode} modifiers={[pickerStyle("menu")]}>
        {PLAYBACK_MODES.map(mode => <Text key={mode} modifiers={[tag(mode)]}>{PLAYBACK_MODE_LABELS[mode]}</Text>)}
      </Picker>
    </Section>
    <Section title="声音增强" footer={<Text>按空间位置调整辅助层（最多 +1.5 dB），并做轻微高频补偿。不识别人声。仅对 SDA 共享渲染生效，系统空间音频路径不应用。</Text>}>
      <Toggle label="空间平衡" isOn={p.spatialEnhancementEnabled} onIsOnChange={p.setSpatialEnhancement} modifiers={[tint(appTheme.accent), disabled(p.busy || p.systemSpatial360RAActive)]} />
    </Section>
    <Section title="360RA／Audio Vivid" footer={<Text>360RA / Audio Vivid：12 声道交给系统，旁路 SDA 共享渲染。修改后下一次播放生效，不中断当前歌曲。关闭后恢复 SDA / HRTFHQ 空间渲染，并非普通立体声下混。</Text>}>
      <Toggle label="系统空间音频 · 7.1.4" isOn={p.systemSpatial360RA} onIsOnChange={p.setSystemSpatial360RA} modifiers={[tint(appTheme.accent), disabled(p.busy)]} />
    </Section>
    <Section title="ALAC 立体声" footer={<Text>上混由立体声生成环绕和高度声道，不是原生 Atmos 或独立对象。系统输出开启时，未上混提交 2.0，上混提交 7.1.4，旁路 SDA 共享渲染。上混播放中平滑切换，已缓冲音频播完后生效；系统输出开关下次播放生效；空间效果取决于输出设备及系统设置。</Text>}>
      <Toggle label="立体声上混 · 7.1.4" isOn={p.alacStereoUpmix} onIsOnChange={p.setAlacStereoUpmix} modifiers={[tint(appTheme.accent), disabled(p.busy)]} />
      <Toggle label="立体声系统空间音频" isOn={p.systemSpatialStereo} onIsOnChange={p.setSystemSpatialStereo} modifiers={[tint(appTheme.accent), disabled(p.busy)]} />
    </Section>
    <Section title="空间渲染" footer={<Text>{preset?.description || "当前使用自定义渲染设置。"} 切换预设保留播放进度与播放 / 暂停状态；加载时可能短暂缓冲。</Text>}>
      <Picker label="渲染预设" selection={preset?.id || "custom"} onSelectionChange={(id: string) => { if (renderingPresets.some(profile => profile.id === id)) p.setRenderingPreset(id); }} modifiers={[pickerStyle("menu"), disabled(presetDisabled)]}>
        {!preset && <Text modifiers={[tag("custom")]}>自定义</Text>}
        {renderingPresets.map(profile => <Text key={profile.id} modifiers={[tag(profile.id)]}>{profile.label}</Text>)}
      </Picker>
    </Section>
    {sharedRenderProfile.spatialCues !== false && <Section title="空间线索强度" footer={<Text>仅调节 SDA / KU100 额外空间线索，不调整主音量或干声增益。播放中平滑切换，不暂停或重新播放；已缓冲的声音播放完后生效。系统空间音频旁路时仅保存设置，恢复 SDA 渲染后生效。</Text>}>
      <Picker label="辅助空间效果" selection={String(p.spatialCueDb)} onSelectionChange={(value: string) => p.setSpatialCueDb(Number(value))} modifiers={[pickerStyle("menu"), disabled(p.spatialCueBusy || p.busy)]}>
        {[0, -3, -6, -9, -12].map(db => <Text key={db} modifiers={[tag(String(db))]}>{`${db} dB${db === -6 ? " · 默认" : ""}`}</Text>)}
      </Picker>
    </Section>}
    {Platform.OS === "ios" && !Platform.isPad && <Section title="界面布局" footer={<Text>竖屏使用平板布局，横屏只改为模糊背景，封面保持原样；关闭恢复原有样式。</Text>}>
      <Toggle label="平板风格界面" isOn={navigationPreferences.tabletPortraitLayout} onIsOnChange={navigationPreferences.setTabletPortraitLayout} />
    </Section>}
    <Section title="界面" footer={<Text>横屏时在所选侧的下角显示导航栏，长按后滑动选择，竖屏保持底部导航。</Text>}>
      <Toggle label="横屏侧边导航" isOn={navigationPreferences.landscapeSideTabs} onIsOnChange={navigationPreferences.setLandscapeSideTabs} />
      <Picker label="导航栏位置" selection={navigationPreferences.sideTabPosition} onSelectionChange={(value: string) => { if (value === "left" || value === "right") navigationPreferences.setSideTabPosition(value); }} modifiers={[pickerStyle("menu")]}>
        <Text modifiers={[tag("left")]}>左边</Text>
        <Text modifiers={[tag("right")]}>右边</Text>
      </Picker>
    </Section>
    <Section title="主题色">
      <Picker label="角色代表色" selection={appTheme.id} onSelectionChange={(value: string) => { if (validTheme(value)) appTheme.setTheme(value); }} modifiers={[pickerStyle("menu")]}>{CHARACTER_THEMES.map(t => <Text key={t.id} modifiers={[tag(t.id)]}>{t.name}</Text>)}</Picker>
    </Section>
    <Section title="应用与输出">
      <Text>外观 · 跟随系统</Text>
      <Text>{"音频输出：" + (p.playing ? p.renderingStatus : "等待播放")}</Text>
      <Text>{p.systemSpatial360RAActive && p.outputChannels === 12 ? "48 kHz · 浮点 PCM · 7.1.4" : "48 kHz · 浮点 PCM · 双声道"}</Text>
      <Button label="关于 SDA" systemImage="info.circle" onPress={() => Alert.alert("SDA", "Spatial Decoder App\n本机空间音频播放器")} />
    </Section>
    <RemoteDiagnosticsSettings />
  </Form>;
}

function RemoteDiagnosticsSettings() {
  const engine = (globalThis as any).expo?.modules?.SdaEngine;
  const [enabled, setEnabled] = useState(() => engine?.diagnosticsEnabled?.() === true);
  const [status, setStatus] = useState("已关闭");
  useEffect(() => { const timer = setInterval(() => setStatus(engine?.diagnosticsStatus?.() ?? "原生模块未更新"), 1000); return () => clearInterval(timer); }, [engine]);
  if (!swiftUI) return null;
  const { Section, Toggle, Text } = swiftUI;
  const save = (next: boolean) => {
    try {
      if (!engine?.configureDiagnostics) throw new Error("请更新原生模块");
      engine.configureDiagnostics(next);
      setEnabled(next);
    } catch (error) { Alert.alert("远程诊断", error instanceof Error ? error.message : String(error)); }
  };
  return <Section title="远程诊断" footer={<Text>{status}</Text>}>
    <Toggle label="远程诊断服务" isOn={enabled} onIsOnChange={save} />
    <Text>开启后，同网络设备可读取播放状态、姿态和性能数据。不提供音频、歌曲名或文件路径；每次启动默认关闭。</Text>
  </Section>;
}
