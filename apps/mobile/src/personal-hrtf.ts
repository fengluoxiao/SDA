import { Alert } from "react-native";
export function showPersonalHrtfSettings() {
 const engine = (globalThis as any).expo?.modules?.SdaEngine;
 if (!engine?.personalHrtfAction) { Alert.alert("个人 HRTF · 前置 TrueDepth", "采集需要含原生模块的 iOS 测试包，Expo Go 可预览入口。仅使用前置相机；完整实验流程在 iPhone 上完成深度配准、三维重建和 Mesh2HRTF 求解，不使用后置相机。此预览不执行采集或计算。"); return; }
 const status = engine.personalHrtfStatus?.() || {};
 const run = async (action: string) => {
  try {
   const result = await engine.personalHrtfAction(action);
   if (result?.cancelled) return;
   Alert.alert("个人 HRTF", result?.message || "设置已保存，下一次打开曲目时使用；当前播放不受影响。");
  } catch (e) { Alert.alert("个人 HRTF 未完成", String(e)); }
 };
 Alert.alert("个人 HRTF · 实验功能", `${status.name || "SDA Spatial · 默认档案"}\n仅使用前置 TrueDepth，需他人协助缓慢扫描完整头耳。头耳重建、质量检查和声学求解在 iPhone 本机执行；候选网格不能直接当作个人 HRTF。完整本机流程未通过验证的构建不会开放采集。已导入的响应由 SDA 自有引擎渲染；系统空间输出与文件调优播放使用各自路径。`, [
  { text: "本机适配与档案", onPress: () => Alert.alert("头耳扫描与档案", "原始相机帧直接送入本机重建器，不保存整段扫描。临时网格在任务完成或取消后删除。单份个人档案最多 16 MiB，总存储上限 32 MiB。", [
   { text: "前置 TrueDepth 扫描", onPress: () => { if (!status.scanSupported) { Alert.alert("无法采集", "此设备没有可用的前置 TrueDepth 相机。"); return; } void run("scan"); } },
   { text: "导入个人 .phrtf", onPress: () => void run("import") },
   { text: "取消", style: "cancel" },
  ]) },
  { text: "恢复默认／删除", onPress: () => Alert.alert("个人档案", "下一次打开曲目时生效。删除后立即取消选择，占用中的旧档案在重启应用时清理。", [
   { text: "使用 SDA 默认档案", onPress: () => void run("default") },
   { text: "删除已导入档案", style: "destructive", onPress: () => void run("delete") },
   { text: "取消", style: "cancel" },
  ]) },
  { text: "取消", style: "cancel" },
 ]);
}
