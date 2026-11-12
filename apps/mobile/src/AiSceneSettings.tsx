import React, { useEffect, useState } from "react";
import { ActivityIndicator, KeyboardAvoidingView, Modal, Platform, Pressable, ScrollView, StyleSheet, Switch, Text, TextInput, View, useColorScheme } from "react-native";
import { SafeAreaProvider, SafeAreaView, initialWindowMetrics } from "react-native-safe-area-context";
import { useAppTheme } from "./app-theme";
import type { AiSceneContextValue } from "./ai-scene-context";

export function AiSceneSettingsLink({ onPress, dark = false, accent = "#7799CC" }: { onPress(): void; dark?: boolean; accent?: string }) {
  return <Pressable accessibilityRole="button" testID="ai-scene-settings-link" accessibilityLabel="配置 AI 场景" onPress={onPress} style={s.link}>
    <Text style={{ color: dark ? "#f5f5f8" : "#17171d", fontSize: 16 }}>AI 场景</Text><Text style={{ color: accent, fontSize: 15 }}>配置与生成 ›</Text>
  </Pressable>;
}
export function AiSceneSettingsModal({ value: v }: { value: AiSceneContextValue }) {
  const dark = useColorScheme() === "dark", theme = useAppTheme();
  const [draft, setDraft] = useState(v.settings), [key, setKey] = useState("");
  useEffect(() => { if (v.settingsOpen) { setDraft(v.settings); setKey(""); } }, [v.settingsOpen, v.ready]);
  const ink = dark ? "#f5f5fa" : "#1b1b22", muted = dark ? "#a0a4b6" : "#666b7d", field = dark ? "#222631" : "#f0f1f6";
  const locked = !v.ready || v.saving;
  const save = async (generate: boolean) => {
    if (await v.save(draft, key || undefined)) {
      setKey("");
      if (generate) void v.regenerate(); else v.closeSettings();
    }
  };
  const input = (label: string, text: string, onChangeText: (text: string) => void, placeholder: string, secret = false, multiline = false) => <View style={s.field}>
    <Text style={[s.label, { color: ink }]}>{label}</Text>
    <TextInput accessibilityLabel={label} value={text} onChangeText={onChangeText} placeholder={placeholder} placeholderTextColor={muted} secureTextEntry={secret}
      multiline={multiline} autoCapitalize="none" autoCorrect={false} editable={!locked} maxLength={secret ? 2048 : multiline ? 600 : 512}
      keyboardType={label === "Base URL" ? "url" : "default"} textContentType="none" importantForAutofill="no" style={[s.input, { color: ink, backgroundColor: field }, multiline && { minHeight: 86, textAlignVertical: "top" }]} />
  </View>;
  const toggle = (label: string, enabled: boolean, change: (value: boolean) => void) => <View style={s.toggle}><Text style={[s.label, { color: ink, flex: 1 }]}>{label}</Text><Switch accessibilityLabel={label} value={enabled} onValueChange={change} disabled={locked} trackColor={{ true: theme.accent }} /></View>;
  return <Modal visible={v.settingsOpen} onRequestClose={v.closeSettings} animationType="slide" presentationStyle="pageSheet">
    <SafeAreaProvider initialMetrics={initialWindowMetrics}><SafeAreaView edges={["top", "bottom"]} style={[s.page, { backgroundColor: dark ? "#131722" : "#fafbfe" }]}>
      <View style={s.header}><Text accessibilityRole="header" style={{ color: ink, fontSize: 21, fontWeight: "600" }}>AI 场景</Text>
        <Pressable accessibilityRole="button" accessibilityLabel="关闭 AI 场景设置" onPress={v.closeSettings} disabled={v.saving} style={s.action}><Text style={{ color: theme.accent, fontSize: 17 }}>完成</Text></Pressable></View>
      <KeyboardAvoidingView style={{ flex: 1 }} behavior={Platform.OS === "ios" ? "padding" : undefined}>
        <ScrollView keyboardShouldPersistTaps="handled" contentContainerStyle={s.content}>
          <Text style={[s.description, { color: muted }]}>根据歌曲生成专属的色彩、光带、粒子与空间构图。开启后，歌曲信息会发送到你填写的 AI 服务；音频和封面留在本机。</Text>
          {toggle("开启 AI 场景", draft.enabled, enabled => setDraft({ ...draft, enabled }))}
          {toggle("切歌时自动生成", draft.autoGenerate, autoGenerate => setDraft({ ...draft, autoGenerate }))}
          {input("Base URL", draft.baseUrl, baseUrl => setDraft({ ...draft, baseUrl }), "https://你的服务地址/v1")}
          {input("API Key", key, setKey, v.hasApiKey ? "已安全保存；留空继续使用原 Key" : "填写你的 API Key", true)}
          {input("模型", draft.model, model => setDraft({ ...draft, model }), "填写服务支持的模型 ID")}
          {input("场景偏好（可选）", draft.prompt, prompt => setDraft({ ...draft, prompt }), "例如：朦胧的胶片质感，冷色流光，结构自由一些", false, true)}
          <Text style={[s.description, { color: muted }]}>兼容 Chat Completions 接口。只发送歌名、歌手、专辑、年份和场景偏好。Key 使用系统安全存储。生成期间照常播放，失败时保留原空间视图。</Text>
          {!!v.songTitle && <Text numberOfLines={2} style={{ color: muted }}>当前歌曲：{v.songTitle}</Text>}
          {v.pending ? <View style={s.pending}><ActivityIndicator color={theme.accent} /><Text style={{ color: muted }}>正在生成场景…</Text><Pressable onPress={v.cancel} accessibilityRole="button" style={s.action}><Text style={{ color: theme.accent }}>取消</Text></Pressable></View>
            : v.scene ? <Text testID="ai-scene-name" style={[s.result, { color: theme.accent }]}>当前场景 · {v.scene.name}</Text> : null}
          {!!(v.preferenceError || v.error) && <Text accessibilityRole="alert" style={s.error}>{v.preferenceError || v.error}</Text>}
          <Pressable testID="ai-scene-save" accessibilityRole="button" disabled={locked} onPress={() => void save(false)} style={[s.primary, { backgroundColor: theme.accent, opacity: locked ? .5 : 1 }]}><Text style={s.primaryText}>{v.saving ? "正在保存…" : "保存设置"}</Text></Pressable>
          <Pressable testID="ai-scene-generate" accessibilityRole="button" disabled={locked || !draft.enabled || v.pending} onPress={() => void save(true)} style={[s.secondary, { opacity: locked || !draft.enabled || v.pending ? .4 : 1 }]}><Text style={{ color: theme.accent, fontSize: 17 }}>{v.scene ? "为当前歌曲重新生成" : "为当前歌曲生成"}</Text></Pressable>
          {v.hasApiKey && <Pressable accessibilityRole="button" disabled={locked} onPress={async () => { if (await v.save({ ...draft, enabled: false }, undefined, true)) { setKey(""); setDraft({ ...draft, enabled: false }); } }} style={s.secondary}><Text style={{ color: muted }}>清除已保存的 Key 并关闭 AI</Text></Pressable>}
        </ScrollView>
      </KeyboardAvoidingView>
    </SafeAreaView></SafeAreaProvider>
  </Modal>;
}
const s = StyleSheet.create({
  page: { flex: 1 }, header: { paddingHorizontal: 20, minHeight: 56, flexDirection: "row", alignItems: "center", justifyContent: "space-between" }, action: { minHeight: 44, paddingHorizontal: 12, justifyContent: "center", alignItems: "center" },
  content: { paddingHorizontal: 22, paddingBottom: 32, maxWidth: 680, width: "100%", alignSelf: "center" }, description: { fontSize: 13, lineHeight: 20, marginVertical: 12 },
  toggle: { flexDirection: "row", alignItems: "center", minHeight: 52, gap: 12 }, label: { fontSize: 16, marginBottom: 6 }, field: { marginTop: 16 }, input: { borderRadius: 12, paddingHorizontal: 14, paddingVertical: 13, fontSize: 16, minHeight: 48 },
  primary: { borderRadius: 14, minHeight: 50, marginTop: 20, alignItems: "center", justifyContent: "center" }, primaryText: { color: "#fff", fontSize: 17, fontWeight: "600" }, secondary: { minHeight: 50, alignItems: "center", justifyContent: "center", marginTop: 6 },
  pending: { flexDirection: "row", alignItems: "center", gap: 10, marginTop: 16 }, result: { marginTop: 16, fontSize: 15 }, error: { color: "#d45a61", fontSize: 14, lineHeight: 20, marginTop: 12 },
  link: { minHeight: 52, paddingHorizontal: 16, flexDirection: "row", gap: 12, justifyContent: "space-between", alignItems: "center" },
});
