import React from "react";
import { Alert, Modal, Platform, PlatformColor, UIManager, StyleSheet, View, useWindowDimensions } from "react-native";
import { swiftUI } from "./IOSSystemTabs";
import type { PlayerProps } from "./RemotePlayer";
import renderingPresets from "../rendering-presets.json";
import { isPresetSelected } from "./ios-ui-model";
import { PLAYBACK_MODES, PLAYBACK_MODE_LABELS } from "../../web/src/playbackOrder";

const hasView = (name: string) => !!(globalThis as any).expo?.getViewConfig?.("ExpoUI", name);
export const hasNativeIOSSettings = !!swiftUI && ["HostView", "FormView", "SectionView", "ToggleView", "PickerView", "TextView", "Button", "HStackView", "VStackView"].every(hasView);

// Use the same UIKit settings stack in Expo Go and the standalone app.
// Home keeps the approved SDA header; only settings owns a system nav bar.
const hasUIKitSettingsNavigation = Platform.OS === "ios" &&
  ["RNSScreen", "RNSScreenStack", "RNSScreenStackHeaderConfig"].every(name => UIManager.hasViewManagerConfig(name));
const nativeScreens: typeof import("react-native-screens") | null =
  hasUIKitSettingsNavigation ? require("react-native-screens") : null;

// Settings uses one inline system title. Never add a second large title
// inside the hosted Form; keep the same header in Expo Go and the app.

const settingsButtonStyle = Platform.OS === "ios" && Number(Platform.Version) >= 26 ? "glass" : "bordered";

export function IOSSettingsButton({ onPress }: { onPress(): void }) {
  if (!swiftUI || !hasView("Button")) return null;
  const { Host, Button } = swiftUI;
  const { accessibilityLabel, labelStyle, buttonStyle, buttonBorderShape } = require("@expo/ui/swift-ui/modifiers") as typeof import("@expo/ui/swift-ui/modifiers");
  return <Host style={{ width: 44, height: 44 }}><Button label="更多设置" systemImage="gearshape" onPress={onPress} modifiers={[labelStyle("iconOnly"), buttonStyle(settingsButtonStyle), buttonBorderShape("circle"), accessibilityLabel("更多设置")]} /></Host>;
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
    if (__DEV__) console.info("[SDA UI] settings navigation: UIKit system header");
    return <ScreenStack style={{ flex: 1 }}>
      {/* NativeStack forbids decreasing activityState; UIKit owns push/pop visibility.
          Stack screens overlap at full size; flex siblings would split the viewport. */}
      <ScreenStackItem screenId="sda-home" activityState={2} freezeOnBlur={false}
        headerConfig={{ hidden: true, title }} contentStyle={{ flex: 1, backgroundColor: immersive ? "transparent" : PlatformColor("systemGroupedBackground") }} style={StyleSheet.absoluteFill}>
        {children}
      </ScreenStackItem>
      {settings && <ScreenStackItem screenId="sda-settings" activityState={2} stackPresentation="push"
        onDismissed={() => onSettingsChange(false)}
        // Keep the centered native title/back button, without a large title.
        headerConfig={{
          title: "设置", backButtonDisplayMode: "minimal", backTitleVisible: false,
          largeTitle: false, translucent: true, hideShadow: true,
          color: PlatformColor("systemBlue"), backgroundColor: "transparent",
          experimental_userInterfaceStyle: theme }}
        contentStyle={{ flex: 1, backgroundColor: PlatformColor("systemGroupedBackground") }} style={StyleSheet.absoluteFill}>
        <Host style={{ flex: 1 }} colorScheme={theme}>
          <NativeSettingsForm player={player} onClose={() => onSettingsChange(false)} />
        </Host>
      </ScreenStackItem>}
    </ScreenStack>;
  }
  return <>{children}<Modal visible={settings} animationType="slide" presentationStyle="fullScreen" onRequestClose={() => onSettingsChange(false)}>
    <IOSSettingsCompatibilityPage player={player} theme={theme} onClose={() => onSettingsChange(false)} />
  </Modal></>;
}

// Compatibility header for environments without the UIKit screen stack.
// Keep the compatibility navigation row full-width with a balanced trailing
// spacer, so the title stays centered and the back button stays at the left.
function IOSSettingsCompatibilityPage({ player, theme, onClose }: {
  player: PlayerProps; theme: "light" | "dark"; onClose(): void;
}) {
  const { width } = useWindowDimensions();
  if (!swiftUI) return null;
  const { Host, VStack, HStack, Button, Text } = swiftUI;
  const { frame, padding, font, buttonStyle, tint, accessibilityLabel, background } = require("@expo/ui/swift-ui/modifiers") as typeof import("@expo/ui/swift-ui/modifiers");
  // One full-screen host owns the safe area. Only the background extends
  // behind system chrome; controls retain SwiftUI's safe-area positioning.
  // Splitting this into RN SafeAreaView + two hosts creates visible edge bands.
  return <View style={{ flex: 1, backgroundColor: PlatformColor("systemGroupedBackground") }}>
    <Host style={{ flex: 1 }} colorScheme={theme}>
      <VStack alignment="leading" spacing={0} modifiers={[
        frame({ width, alignment: "leading" }),
        background(PlatformColor("systemGroupedBackground"), { ignoresSafeAreaEdges: "all" }),
      ]}>
        <HStack spacing={0} modifiers={[padding({ horizontal: 16 }), frame({ width, height: 52 })]}>
          <Button label="返回" systemImage="chevron.left" onPress={onClose} modifiers={[
            buttonStyle(settingsButtonStyle), tint(PlatformColor("systemBlue")),
            font({ textStyle: "body" }), frame({ minHeight: 44 }),
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
  const { width } = useWindowDimensions();
  if (!swiftUI) return null;
  const { Form, Section, Toggle, Picker, Text, Button, HStack } = swiftUI;
  const { disabled, tag, pickerStyle, tint, scrollContentBackground, background, frame, buttonStyle } = require("@expo/ui/swift-ui/modifiers") as typeof import("@expo/ui/swift-ui/modifiers");
  const audioDisabled = p.systemSpatial360RAActive || p.busy;
  const presetDisabled = audioDisabled || p.roomBusy || p.nearFieldBusy;
  const preset = renderingPresets.find(profile => isPresetSelected(p, profile));
  return <Form modifiers={[
    // The header owns the only title; the Form contains settings rows only.
    tint(PlatformColor("systemBlue")),
    scrollContentBackground("hidden"),
    // No forced material or custom scroll-edge style: the native header/Form use
    // the OS's default treatment, including the iOS 26 Liquid Glass edge effect.
    background(PlatformColor("systemGroupedBackground"), { ignoresSafeAreaEdges: "all" }),
  ]}>
    <Section title="播放操作">
      <HStack spacing={12}>
        <Button label="重新播放" systemImage="arrow.counterclockwise" onPress={() => { onClose(); p.play(); }} modifiers={[buttonStyle("borderless"), frame({ maxWidth: width, minHeight: 44 }), disabled(p.busy || !p.selectedUri)]} />
        <Button label="停止播放" systemImage="stop.fill" role="destructive" onPress={() => { onClose(); p.stop(); }} modifiers={[buttonStyle("borderless"), frame({ maxWidth: width, minHeight: 44 }), disabled(p.busy || !p.playing)]} />
      </HStack>
    </Section>
    <Section title="播放" footer={<Text>双声道 / 360RA 响度平衡，只衰减不增益。</Text>}>
      <Toggle label="音量平衡" isOn={p.volumeBalanceEnabled} onIsOnChange={p.setVolumeBalance} modifiers={[tint(PlatformColor("systemGreen")), disabled(p.busy)]} />
      <Picker label="播放模式" selection={p.playbackMode} onSelectionChange={p.setPlaybackMode} modifiers={[pickerStyle("menu")]}>
        {PLAYBACK_MODES.map(mode => <Text key={mode} modifiers={[tag(mode)]}>{PLAYBACK_MODE_LABELS[mode]}</Text>)}
      </Picker>
    </Section>
    <Section title="声音增强" footer={<Text>主层维持原电平，取消辅助层额外 +6 dB；保留总输出 +2 dB 与主层补偿，峰值保护后整体微降 0.75 dB。不识别人声。仅 SDA / KU100 路径应用，系统空间音频路径不应用。</Text>}>
      <Toggle label="空间平衡 · −0.75 dB" isOn={p.spatialEnhancementEnabled} onIsOnChange={p.setSpatialEnhancement} modifiers={[tint(PlatformColor("systemGreen")), disabled(p.busy || p.systemSpatial360RAActive)]} />
    </Section>
    <Section title="360 Reality Audio" footer={<Text>仅 360RA：12 声道交给系统，旁路 KU100 直达渲染。修改后下一次播放生效，不中断当前歌曲。关闭后恢复 SDA / KU100 空间渲染，并非普通立体声下混。</Text>}>
      <Toggle label="系统空间音频 · 7.1.4" isOn={p.systemSpatial360RA} onIsOnChange={p.setSystemSpatial360RA} modifiers={[tint(PlatformColor("systemGreen")), disabled(p.busy)]} />
    </Section>
    <Section title="ALAC 立体声" footer={<Text>上混由立体声生成环绕和高度声道，不是原生 Atmos 或独立对象。系统输出开启时，未上混提交 2.0，上混提交 7.1.4，旁路 KU100。上混播放中平滑切换，已缓冲音频播完后生效；系统输出开关下次播放生效；空间效果取决于输出设备及系统设置。</Text>}>
      <Toggle label="立体声上混 · 7.1.4" isOn={p.alacStereoUpmix} onIsOnChange={p.setAlacStereoUpmix} modifiers={[tint(PlatformColor("systemGreen")), disabled(p.busy)]} />
      <Toggle label="立体声系统空间音频" isOn={p.systemSpatialStereo} onIsOnChange={p.setSystemSpatialStereo} modifiers={[tint(PlatformColor("systemGreen")), disabled(p.busy)]} />
    </Section>
    <Section title="空间渲染" footer={<Text>{preset?.description || "当前使用自定义渲染设置。"} 切换预设保留播放进度与播放 / 暂停状态；加载时可能短暂缓冲。</Text>}>
      <Picker label="渲染预设" selection={preset?.id || "custom"} onSelectionChange={(id: string) => { if (renderingPresets.some(profile => profile.id === id)) p.setRenderingPreset(id); }} modifiers={[pickerStyle("menu"), disabled(presetDisabled)]}>
        {!preset && <Text modifiers={[tag("custom")]}>自定义</Text>}
        {renderingPresets.map(profile => <Text key={profile.id} modifiers={[tag(profile.id)]}>{profile.label}</Text>)}
      </Picker>
      <Toggle label="逐对象渲染" isOn={p.directObjects} onIsOnChange={value => p.setRendering(value, p.directionalObjects)} modifiers={[tint(PlatformColor("systemGreen")), disabled(audioDisabled)]}><Text>逐对象渲染</Text><Text>每个对象独立生成双耳声音</Text></Toggle>
      <Toggle label="实际方向" isOn={p.directionalObjects} onIsOnChange={value => p.setRendering(p.directObjects, value)} modifiers={[tint(PlatformColor("systemGreen")), disabled(audioDisabled)]}><Text>实际方向</Text><Text>按对象真实位置定位声音</Text></Toggle>
    </Section>
    <Section title="空间线索强度" footer={<Text>仅调节 SDA / KU100 额外空间线索，不调整主音量或干声增益。播放中平滑切换，不暂停或重新播放；已缓冲的声音播放完后生效。系统空间音频旁路时仅保存设置，恢复 SDA 渲染后生效。</Text>}>
      <Picker label="辅助空间效果" selection={String(p.spatialCueDb)} onSelectionChange={(value: string) => p.setSpatialCueDb(Number(value))} modifiers={[pickerStyle("menu"), disabled(p.spatialCueBusy || p.busy)]}>
        {[0, -3, -6, -9, -12].map(db => <Text key={db} modifiers={[tag(String(db))]}>{`${db} dB${db === -6 ? " · 默认" : ""}`}</Text>)}
      </Picker>
    </Section>
    <Section title="应用与输出">
      <Text>外观 · 跟随系统</Text>
      <Text>{"音频输出：" + (p.playing ? p.renderingStatus : "等待播放")}</Text>
      <Text>{p.systemSpatial360RAActive && p.outputChannels === 12 ? "48 kHz · 浮点 PCM · 7.1.4" : "48 kHz · 浮点 PCM · 双声道"}</Text>
      <Button label="关于 SDA" systemImage="info.circle" onPress={() => Alert.alert("SDA", "Spatial Decoder App\n本机空间音频播放器")} />
    </Section>
  </Form>;
}
