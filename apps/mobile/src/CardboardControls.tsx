import React, { useEffect, useState } from "react";
import { Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import { CARDBOARD_DEFAULTS, CARDBOARD_RANGES, normalizeCardboardConfig, type CardboardConfig } from "./cardboard-config";

export function CardboardControls({ config, onChange, landscape, status, visible, calibrating, onCalibrate, onHide, onRecenter, onRetry, onExit }: {
  config: CardboardConfig; onChange?: (config: CardboardConfig) => void; landscape: boolean; status: string;
  visible: boolean; calibrating: boolean; onCalibrate: (value: boolean) => void; onHide: () => void;
  onRecenter: () => void; onRetry: () => void; onExit?: () => void;
}) {
  const [panelVisible, setPanelVisible] = useState(true);
  useEffect(() => { if (calibrating) setPanelVisible(true); }, [calibrating]);
  const button = (title: string, action: () => void) => <Pressable key={title} accessibilityRole="button" accessibilityLabel={title} onPress={action} style={s.button}><Text style={s.text}>{title}</Text></Pressable>;
  const labels = { eyeDistanceMm: "眼距 · mm", lensDistanceMm: "镜片间距 · mm", screenWidthMm: "屏幕横向可视宽度 · mm", fov: "垂直视场角 · °", k1: "畸变系数 k1", k2: "畸变系数 k2" };
  return <View pointerEvents="box-none" style={StyleSheet.absoluteFill} testID="cardboard-vr-controls">
    {!landscape && <View style={s.rotate}><Text style={s.title}>请把手机转成横屏</Text><Text style={s.note}>横屏后显示左右眼画面</Text></View>}
    {!!status && <View style={s.status}><Text accessibilityRole="alert" style={s.note}>{status}</Text>{button("重试姿态连接", onRetry)}</View>}
    {visible && !calibrating && <View style={s.toolbar}>{button("视角回中", onRecenter)}{button("镜片校准", () => onCalibrate(true))}{button("开始观看", onHide)}{onExit && button("退出 VR", onExit)}</View>}
    {calibrating && !panelVisible && visible && <View style={s.toolbar}>{button("调整校准参数", () => setPanelVisible(true))}{button("隐藏按钮", onHide)}{button("完成校准", () => onCalibrate(false))}{onExit && button("退出 VR", onExit)}</View>}
    {calibrating && panelVisible && <View style={s.panel} testID="cardboard-calibration-panel"><ScrollView contentContainerStyle={{ padding: 14 }}>
      <Text style={s.title}>纸盒 VR 校准</Text><Text style={s.note}>默认参数只是起点，请按眼镜和屏幕实测值调整。网格应融合、横竖线应尽量笔直；完成后轻点画面可再次显示按钮。</Text>
      {(Object.keys(CARDBOARD_RANGES) as (keyof typeof CARDBOARD_RANGES)[]).map(key => {
        const [min, max, step] = CARDBOARD_RANGES[key];
        const change = (delta: number) => onChange?.(normalizeCardboardConfig({ ...config, [key]: Math.round((config[key] + delta) * 100) / 100 }));
        return <View key={key} style={s.row}><Text style={s.text}>{labels[key]}</Text><View style={s.valueRow}><Pressable accessibilityRole="button" accessibilityLabel={"减小" + labels[key]} disabled={config[key] <= min} onPress={() => change(-step)} style={s.step}><Text style={s.text}>−</Text></Pressable><Text style={[s.text, s.value]}>{config[key].toFixed(key === "k1" || key === "k2" ? 2 : 0)}</Text><Pressable accessibilityRole="button" accessibilityLabel={"增大" + labels[key]} disabled={config[key] >= max} onPress={() => change(step)} style={s.step}><Text style={s.text}>＋</Text></Pressable></View></View>;
      })}
      <View style={s.actions}>{button("查看完整网格", () => setPanelVisible(false))}{button("恢复初始参数", () => onChange?.({ ...CARDBOARD_DEFAULTS, enabled: config.enabled }))}{button("完成校准", () => onCalibrate(false))}</View>
    </ScrollView></View>}
  </View>;
}
const s = StyleSheet.create({
  toolbar: { position: "absolute", left: 8, right: 8, bottom: 10, flexDirection: "row", justifyContent: "center", flexWrap: "wrap", gap: 8 },
  button: { minHeight: 44, paddingHorizontal: 16, borderRadius: 22, alignItems: "center", justifyContent: "center", backgroundColor: "#182135ed" },
  text: { color: "#eef2ff", fontSize: 13 }, note: { color: "#b7c5df", fontSize: 12, lineHeight: 18 }, title: { color: "#fff", fontSize: 17, fontWeight: "600", marginBottom: 8 },
  status: { position: "absolute", alignSelf: "center", top: 14, maxWidth: "85%", borderRadius: 14, padding: 10, backgroundColor: "#101827ed", alignItems: "center", gap: 6 },
  rotate: { position: "absolute", top: "30%", alignSelf: "center", backgroundColor: "#101827ee", padding: 20, borderRadius: 16 },
  panel: { position: "absolute", alignSelf: "center", bottom: 8, width: "94%", maxWidth: 640, maxHeight: "72%", borderRadius: 18, backgroundColor: "#101827f2" },
  row: { flexDirection: "row", alignItems: "center", justifyContent: "space-between", marginTop: 6, gap: 8 }, valueRow: { flexDirection: "row", alignItems: "center" },
  value: { width: 48, textAlign: "center", fontVariant: ["tabular-nums"] }, step: { width: 44, height: 44, alignItems: "center", justifyContent: "center" }, actions: { flexDirection: "row", flexWrap: "wrap", justifyContent: "flex-end", gap: 8, marginTop: 10 },
});
