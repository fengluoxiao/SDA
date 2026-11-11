import React from "react";
import { Pressable, StyleSheet, Text, View, type StyleProp, type ViewStyle } from "react-native";
import { SPACE_STAGE, spaceVisualColumns } from "./space-visual-model";

type Props = React.PropsWithChildren<{
  enabled: boolean; compact?: boolean;
  accent: string; objectCount: number;
  vr?: boolean; fullscreen?: boolean; fullscreenInsets?: { top: number; right: number; bottom: number; left: number }; onFullscreenChange?: (fullscreen: boolean) => void; header: React.ReactNode; footer: React.ReactNode | ((columns: boolean) => React.ReactNode);
  viewportStyle: StyleProp<ViewStyle>;
}>;

/** Both appearances keep the same viewport/Canvas mounted in the same tree position. */
export function SpaceVisualPage(p: Props) {
  const [size, setSize] = React.useState({ width: 0, height: 0 });
  const columns = !p.fullscreen && p.enabled && spaceVisualColumns(size.width, size.height);
  const panelWidth = Math.min(p.compact ? 224 : 264, Math.max(176, size.width * .24));
  return <View style={[s.page, columns && s.columns]} onLayout={event => { const { width, height } = event.nativeEvent.layout; setSize(old => old.width === width && old.height === height ? old : { width, height }); }} testID={p.enabled ? "immersive-space-page" : "classic-space-page"}>
    <View style={[columns && { width: panelWidth }, p.fullscreen && s.hidden]}>
      {p.header}
    </View>
    <View style={[p.enabled ? [s.stage, p.compact && s.compactStage, columns && s.columnStage] : p.viewportStyle, p.fullscreen && s.fullscreenStage]} testID="space-scene-viewport">
      {p.children}
      {p.enabled && !(p.vr && p.fullscreen) && <View pointerEvents="none" accessible={false} accessibilityElementsHidden importantForAccessibility="no-hide-descendants" style={StyleSheet.absoluteFill}>
        <View style={[s.sceneCaption, p.compact && s.compactCaption, p.fullscreen && { top: 20 + (p.fullscreenInsets?.top ?? 0), left: 18 + (p.fullscreenInsets?.left ?? 0) }]}><View style={[s.statusDot, { backgroundColor: p.accent }]} /><Text testID={p.fullscreen ? "space-listener-view" : undefined} style={s.caption}>{p.fullscreen ? "人头麦视角" : "空间声场"}</Text></View>
        <View style={[s.counter, p.compact && s.compactCounter, p.fullscreen && { top: 16 + (p.fullscreenInsets?.top ?? 0), right: 18 + (p.fullscreenInsets?.right ?? 0) }]}><Text style={s.counterLabel}>显示对象</Text><Text style={[s.counterValue, p.compact && s.compactCounterValue]}>{String(p.objectCount).padStart(2, "0")}</Text></View>
      </View>}
      {p.onFullscreenChange && !(p.vr && p.fullscreen) && <Pressable testID="space-fullscreen-button" accessibilityRole="button" accessibilityLabel={p.fullscreen ? "退出空间全屏" : "空间全屏"} onPress={() => p.onFullscreenChange?.(!p.fullscreen)} style={[s.fullscreenButton, p.fullscreen && { bottom: 12 + (p.fullscreenInsets?.bottom ?? 0), right: 12 + (p.fullscreenInsets?.right ?? 0) }]}>
        <Text style={s.fullscreenLabel}>{p.fullscreen ? "退出全屏" : "全屏"}</Text>
      </Pressable>}
    </View>
    <View style={[p.fullscreen && s.hidden, columns && { position: "absolute", left: 0, bottom: 0, width: panelWidth }]}>{typeof p.footer === "function" ? p.footer(columns) : p.footer}</View>
  </View>;
}
const s = StyleSheet.create({
  hidden: { display: "none" }, fullscreenStage: { flex: 1, maxHeight: "100%", marginHorizontal: 0, marginTop: 0, borderRadius: 0, borderWidth: 0 },
  fullscreenButton: { position: "absolute", bottom: 12, right: 12, minWidth: 64, minHeight: 44, paddingHorizontal: 14, justifyContent: "center", alignItems: "center", borderRadius: 22, backgroundColor: "#1b243be6" },
  fullscreenLabel: { color: "#eef2ff", fontSize: 13, fontWeight: "600" },
  page: { flex: 1, minHeight: 0, minWidth: 0 }, columns: { flexDirection: "row", columnGap: 18 }, columnStage: { marginTop: 0 },
  stage: { flex: 1, minHeight: 0, marginTop: 13, borderRadius: 28, borderWidth: 1, borderColor: "#cad7f126", overflow: "hidden", backgroundColor: SPACE_STAGE.background }, compactStage: { marginTop: 7, borderRadius: 20 },
  sceneCaption: { position: "absolute", left: 18, top: 20, flexDirection: "row", alignItems: "center", gap: 7 }, compactCaption: { top: 12, left: 14 }, statusDot: { width: 5, height: 5, borderRadius: 3 }, caption: { fontSize: 11, letterSpacing: 1.3, color: "#b8c6e2" },
  counter: { position: "absolute", top: 16, right: 18, alignItems: "flex-end" }, compactCounter: { top: 10, right: 14 }, counterLabel: { fontSize: 9, letterSpacing: 1.5, color: "#889ab9" },
  counterValue: { fontSize: 38, lineHeight: 44, fontWeight: "200", color: SPACE_STAGE.foreground, fontVariant: ["tabular-nums"] }, compactCounterValue: { fontSize: 24, lineHeight: 28 },
});
