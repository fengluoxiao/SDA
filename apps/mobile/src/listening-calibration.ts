import { Alert } from "react-native";
import type { PlayerProps } from "./RemotePlayer";
import { DEFAULT_LISTENING_PROFILE, listeningProfile, type ListeningProfile } from "./listening-profile";
// Native system sheets in Expo Go and release; the musical queue is untouched.
export function showListeningCalibration(p: PlayerProps) {
  if (!p.setListeningProfile) { Alert.alert("试听校准", "请更新原生模块"); return; }
  const original = listeningProfile(p.listeningProfile);
  let candidate = { ...original }, comparing = false, saving = false;
  const steps: { key: keyof ListeningProfile; title: string; copy: string; values: number[]; names: string[] }[] = [
    { key: "rear", title: "后方辨识", copy: "用熟悉的后方声源试听，选择清楚且不抢前方的程度。只调整原有细节处理，不移动声源。", values: [.5, 1, 1.5], names: ["柔和", "标准", "清晰"] },
    { key: "height", title: "上下辨识", copy: "试听头顶或下方细节，选择自然的程度。不会叠加后方和高度增益。", values: [.5, 1, 1.5], names: ["柔和", "标准", "清晰"] },
    { key: "bassDb", title: "耳机低频", copy: "这是耳机听感偏好，不是自动测量。选择低频厚度，保持舒适音量。", values: [-2, 0, 2], names: ["收敛 −2 dB", "原样", "厚实 +2 dB"] },
    { key: "trebleDb", title: "耳机高频", copy: "选择高频明亮程度；以人声、镲片自然且不刺耳为准。", values: [-2, 0, 2], names: ["柔和 −2 dB", "原样", "明亮 +2 dB"] },
  ];
  const save = async (profile: ListeningProfile, next: () => void) => {
    if (saving) return; saving = true;
    try { await p.setListeningProfile!(profile); saving = false; next(); }
    catch (e) { saving = false; Alert.alert("试听设置未应用", String(e), [{ text: "返回试听", onPress: () => step(0) }, { text: "恢复进入前设置", onPress: () => void save(original, () => {}) }]); }
  };
  const cancel = () => void save(original, () => {});
  const review = () => Alert.alert("比较试听", comparing ? "正在听进入前的设置。" : "正在听本次选择。使用同一段音乐比较；输出保护保持开启。", [
    { text: comparing ? "听本次选择" : "听进入前设置", onPress: () => void save(comparing ? candidate : original, () => { comparing = !comparing; review(); }) },
    { text: "保存本次选择", onPress: () => void save(candidate, () => {}) },
    { text: "取消并恢复", style: "cancel", onPress: cancel },
  ], { cancelable: false });
  const step = (index: number) => {
    if (index >= steps.length) { review(); return; }
    const s = steps[index]!;
    Alert.alert(`${index + 1}/4 · ${s.title}`, `${s.copy}\n当前：${candidate[s.key]}`, [
      { text: "调整并试听", onPress: () => Alert.alert(s.title, "选择后会平滑应用，再回到本步骤。", s.values.map((v, i) => ({
        text: s.names[i]!, onPress: () => {
          const next = { ...candidate, [s.key]: v };
          void save(next, () => { candidate = next; step(index); });
        },
      })), { cancelable: true, onDismiss: () => step(index) }) },
      { text: "下一步", onPress: () => step(index + 1) },
      { text: "取消并恢复", style: "cancel", onPress: cancel },
    ], { cancelable: false });
  };
  Alert.alert("SDA Spatial · 试听校准", (p.previewMode ? "Expo Go 仅预览设置流程，不输出原生试听音频。\n" : "") + "戴好常用耳机，先播放一段熟悉且有后方、上下声源的音乐。四步调节会立即试听，最后可比较与恢复。这里只保存听感偏好，不代表测得个人 HRTF 或耳机频响。", [
    { text: "开始试听", onPress: () => { if (!p.previewMode && (!p.playing || p.paused || !p.spatialEnhancementEnabled || p.binauralEnabled === false)) { Alert.alert("先准备试听", "请先使用 SDA Spatial 播放空间曲目，开启空间平衡，再返回开始试听。"); return; } step(0); } },
    { text: "恢复默认", onPress: () => void save({ ...DEFAULT_LISTENING_PROFILE }, () => {}) },
    { text: "取消", style: "cancel" },
  ]);
}
