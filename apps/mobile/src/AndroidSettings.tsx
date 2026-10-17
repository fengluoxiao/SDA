import renderExperiment from "../ios-experiments/active.json";
import { androidMaterialTheme } from "./android-material-theme";
import { CHARACTER_THEMES, useAppTheme } from "./app-theme";
import React from "react";
import { Alert, Modal, Platform, Pressable, ScrollView, StyleSheet, Switch, Text, View } from "react-native";
import type { PlayerProps } from "./RemotePlayer";
import { AndroidSymbol } from "./AndroidSymbol";
import { AndroidGlassSettingsButton } from "./AndroidGlassSettingsButton";
import { isPresetSelected } from "./ios-ui-model";
import renderingPresets from "../rendering-presets.json";
import { PLAYBACK_MODES, PLAYBACK_MODE_LABELS } from "../../web/src/playbackOrder";
const compose: typeof import("@expo/ui/jetpack-compose") | null = Platform.OS === "android" && (globalThis as any).expo?.modules?.ExpoUI
  ? require("@expo/ui/jetpack-compose") : null;

export function AndroidSettings({ player: p, dark, onClose }: { player: PlayerProps; dark: boolean; onClose(): void }) {
  const appTheme = useAppTheme();
  const [selection, setSelection] = React.useState<null | { title: string; options: { id: string; label: string }[]; selected: string; choose(id: string): void }>(null);
  const c = androidMaterialTheme(appTheme.id, dark);
  const preset = renderingPresets.find(profile => isPresetSelected(p, profile));
  const audioDisabled = p.busy || p.systemSpatial360RAActive;
  const presetDisabled = audioDisabled || p.roomBusy || p.nearFieldBusy;
  const text = (value: string, muted = false, style: object = {}) => <Text style={[{ color: muted ? c.muted : c.ink }, style]}>{value}</Text>;
  const separator = () => null;
  const section = (title: string, children: React.ReactNode, footer?: string) => <View style={s.section}>
    <Text style={[s.heading, { color: c.primary }]}>{title}</Text><View style={s.group}>{children}</View>
    {!!footer && text(footer, true, s.footer)}
  </View>;
  const toggle = (title: string, value: boolean, onChange: (value: boolean) => void, disabled: boolean, subtitle?: string) => <View style={[s.row, { opacity: disabled ? .45 : 1 }]}>
    <View style={s.copy}>{text(title, false, s.title)}{!!subtitle && text(subtitle, true, s.subtitle)}</View>
    {compose ? <View accessibilityLabel={title} style={{ width: 60, height: 48 }}><compose.Host style={{ width: 60, height: 48 }} colorScheme={dark ? "dark" : "light"} seedColor={appTheme.accent}>
      <compose.Switch value={value} enabled={!disabled} onCheckedChange={onChange} colors={{ checkedTrackColor: c.primary, checkedThumbColor: c.onPrimary, uncheckedTrackColor: c.panel, uncheckedThumbColor: c.muted, uncheckedBorderColor: c.muted }} />
    </compose.Host></View> : <Switch accessibilityLabel={title} disabled={disabled} value={value} onValueChange={onChange}
      trackColor={{ false: c.line, true: c.primary }} thumbColor={value ? c.onPrimary : c.muted} />}
  </View>;
  const menu = (title: string, value: string, options: { id: string; label: string }[], selected: string, choose: (id: string) => void, disabled = false) =>
    <Pressable accessibilityRole="button" accessibilityLabel={`${title}，${value}`} accessibilityState={{ disabled }} disabled={disabled}
      onPress={() => setSelection({ title, options, selected, choose })}
      android_ripple={{ color: dark ? "#ffffff18" : "#7799CC18" }}
      style={({ pressed }) => [s.row, { opacity: disabled ? .45 : pressed ? .6 : 1 }]}>
      <View style={s.copy}>{text(title, false, s.title)}{text(value, true, s.subtitle)}</View><AndroidSymbol name="chevron.right" size={24} color={c.muted} />
    </Pressable>;
  const action = (label: string, symbol: string, callback: () => void, disabled: boolean, destructive = false) => <Pressable
    accessibilityRole="button" accessibilityLabel={label} accessibilityState={{ disabled }} disabled={disabled} onPress={() => { onClose(); callback(); }}
    style={({ pressed }) => [s.action, { opacity: disabled ? .38 : pressed ? .7 : 1, backgroundColor: destructive ? "transparent" : c.container, borderColor: destructive ? c.line : "transparent" }]}>
    <AndroidSymbol name={symbol} size={19} color={destructive ? c.error : c.onContainer} />
    <Text style={{ color: destructive ? c.error : c.onContainer, fontSize: 14, fontWeight: "500" }}>{label}</Text>
  </Pressable>;
  return <View style={[StyleSheet.absoluteFill, { backgroundColor: c.bg }]} accessibilityViewIsModal>
    <View style={[s.header, { backgroundColor: c.bg }]}>
      <View style={s.back}><AndroidGlassSettingsButton back dark={dark} onPress={onClose} /></View>
      <Text accessibilityRole="header" style={[s.headerTitle, { color: c.ink }]}>设置</Text>
    </View>
    <ScrollView showsVerticalScrollIndicator={false} contentContainerStyle={s.content}>
      {section("播放操作", <View style={s.actions}>
        {action("重新播放", "arrow.counterclockwise", p.play, p.busy || !p.selectedUri)}
        {action("停止播放", "stop.fill", p.stop, p.busy || !p.playing, true)}
      </View>)}
      {section("播放", <>
        {toggle("音量平衡", p.volumeBalanceEnabled, p.setVolumeBalance, p.busy)}{separator()}
        {menu("播放模式", PLAYBACK_MODE_LABELS[p.playbackMode], PLAYBACK_MODES.map(id => ({ id, label: PLAYBACK_MODE_LABELS[id] })), p.playbackMode,
          id => { if (PLAYBACK_MODES.includes(id as typeof p.playbackMode)) p.setPlaybackMode(id as typeof p.playbackMode); })}
      </>, "双声道 / 360RA 响度平衡，只衰减不增益。")}
      {section("声音增强", toggle("空间平衡", p.spatialEnhancementEnabled, p.setSpatialEnhancement, audioDisabled),
        "按空间位置调整辅助层（最多 +1.5 dB），并做轻微高频补偿。")}
      {section("360 Reality Audio", <View style={s.row}><View style={s.copy}>{text("系统空间音频", false, s.title)}{text("Android 系统多声道输出尚未接通", true, s.subtitle)}</View></View>,
        "360RA 文件仍可通过 SDA / HRTFHQ 双耳渲染播放。此项不是 Apple 系统空间音频的等价实现。")}
      {section("ALAC 立体声", toggle("立体声上混 · 7.1.4", p.alacStereoUpmix, p.setAlacStereoUpmix, p.busy),
        "直接使用 SDA 共享上混处理，经共享 HRTF 渲染为双声道。不是原生 Atmos 或独立对象；播放中平滑切换，已缓冲音频播完后生效。")}
      {section("空间渲染", <>
        {menu("渲染预设", preset?.label || "自定义", renderingPresets.map(profile => ({ id: profile.id, label: profile.label })), preset?.id || "custom", p.setRenderingPreset, presetDisabled)}{separator()}
        {toggle("逐对象渲染", p.directObjects, value => p.setRendering(value, p.directionalObjects), audioDisabled, "每个对象独立生成双耳声音")}{separator()}
        {toggle("实际方向", p.directionalObjects, value => p.setRendering(p.directObjects, value), audioDisabled, "按对象真实位置定位声音")}
      </>, `${preset?.description || "当前使用自定义渲染设置。"} 切换预设保留播放进度与播放 / 暂停状态；加载时可能短暂缓冲。`)}
      {renderExperiment.spatialCues !== false && section("空间线索强度", menu("辅助空间效果", `${p.spatialCueDb} dB`, [0, -3, -6, -9, -12].map(db => ({ id: String(db), label: `${db} dB${db === -6 ? " · 默认" : ""}` })), String(p.spatialCueDb),
        id => p.setSpatialCueDb(Number(id)), p.spatialCueBusy || p.busy),
        "仅调节额外空间线索，不改变主音量或干声增益。播放中平滑切换，已缓冲的声音播放完后生效。")}
      {section("主题色", <>{CHARACTER_THEMES.map(t => <Pressable key={t.id} onPress={() => appTheme.setTheme(t.id)} accessibilityRole="button" accessibilityState={{ selected: appTheme.id === t.id }} style={s.row}><View style={{ width: 22, height: 22, borderRadius: 11, backgroundColor: t.color, marginRight: 12 }} />{text(t.name, false, s.title)}{text(appTheme.id === t.id ? "✓" : "", false)}</Pressable>)}</>)}
      {section("应用与输出", <>
        <View style={s.row}>{text("外观 · 跟随系统", false, s.title)}</View>{separator()}
        <View style={s.row}><View style={s.copy}>{text("音频输出：" + (p.playing ? p.renderingStatus : "等待播放"), false, s.output)}{text("48 kHz · 浮点 PCM · 双声道", true, s.subtitle)}</View></View>{separator()}
        <Pressable accessibilityRole="button" accessibilityLabel="关于 SDA" onPress={() => Alert.alert("SDA", "Spatial Decoder App\n本机空间音频播放器")} style={s.row}>
          <View style={s.about}><AndroidSymbol name="info.circle" size={24} color={c.muted} /><Text style={{ color: c.ink, fontSize: 16 }}>关于 SDA</Text></View><AndroidSymbol name="chevron.right" size={24} color={c.muted} />
        </Pressable>
      </>)}
    </ScrollView>
    <Modal transparent visible={selection !== null} animationType="fade" onRequestClose={() => setSelection(null)}>
      <View style={s.scrim}>
        <Pressable accessibilityLabel="取消选择" onPress={() => setSelection(null)} style={StyleSheet.absoluteFill} />
        <View style={[s.dialog, { backgroundColor: c.panel }]} accessibilityViewIsModal>
          <Text accessibilityRole="header" style={{ color: c.ink, padding: 24, fontSize: 24, lineHeight: 32 }}>{selection?.title}</Text>
          <ScrollView>{selection?.options.map(option => <Pressable key={option.id} accessibilityRole="radio" accessibilityState={{ checked: option.id === selection.selected }}
            onPress={() => { const choose = selection.choose; setSelection(null); choose(option.id); }} android_ripple={{ color: dark ? "#ffffff18" : "#7799CC18" }} style={[s.row, { justifyContent: "flex-start" }]}>
            <View style={[s.radio, { borderColor: option.id === selection.selected ? c.primary : c.muted }]}>{option.id === selection.selected && <View style={[s.radioDot, { backgroundColor: c.primary }]} />}</View>{text(option.label, false, s.title)}
          </Pressable>)}</ScrollView>
          <Pressable accessibilityRole="button" onPress={() => setSelection(null)} style={s.cancel}><Text style={{ color: c.primary, fontSize: 14, fontWeight: "500" }}>取消</Text></Pressable>
        </View>
      </View>
    </Modal>
  </View>;
}

const s = StyleSheet.create({
  header: { height: 64, flexDirection: "row", alignItems: "center", gap: 12, paddingHorizontal: 8 },
  back: { width: 48, height: 48, alignItems: "center", justifyContent: "center" }, headerTitle: { fontSize: 22, fontWeight: "400" },
  content: { paddingTop: 4, paddingBottom: 32 }, section: { marginTop: 16 },
  heading: { fontSize: 14, fontWeight: "500", lineHeight: 20, marginHorizontal: 24, marginBottom: 8 }, group: {},
  footer: { fontSize: 12, lineHeight: 18, marginHorizontal: 24, marginTop: 8, marginBottom: 12 },
  row: { minHeight: 64, paddingVertical: 12, paddingHorizontal: 24, flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 16 },
  title: { fontSize: 16, lineHeight: 24, flexShrink: 1 }, subtitle: { fontSize: 14, lineHeight: 20, marginTop: 2 }, copy: { flex: 1 },
  value: { flexShrink: 1, flexDirection: "row", alignItems: "center", gap: 5 }, valueText: { fontSize: 16, lineHeight: 22, flexShrink: 1 },
  actions: { flexDirection: "row", gap: 12, paddingHorizontal: 24 }, action: { flex: 1, minHeight: 48, borderRadius: 24, borderWidth: 1, flexDirection: "row", alignItems: "center", justifyContent: "center", gap: 8 },
  output: { fontSize: 16, lineHeight: 22 }, about: { flexDirection: "row", alignItems: "center", gap: 8 },
  scrim: { flex: 1, justifyContent: "center", paddingHorizontal: 28, backgroundColor: "rgba(0,0,0,.32)" }, dialog: { borderRadius: 28, maxHeight: "70%", overflow: "hidden" },
  radio: { width: 20, height: 20, borderRadius: 10, borderWidth: 2, alignItems: "center", justifyContent: "center" }, radioDot: { width: 10, height: 10, borderRadius: 5 },
  cancel: { minHeight: 48, alignSelf: "flex-end", justifyContent: "center", paddingHorizontal: 24, marginBottom: 8 },
});
