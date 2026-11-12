import foliaNotices from "../../../vendor/folia/notices.json";
import React, { useEffect, useState } from "react";
import { ActivityIndicator, KeyboardAvoidingView, Platform, Pressable, ScrollView, StyleSheet, Switch, Text, TextInput, View, useColorScheme } from "react-native";
import { SafeAreaProvider, SafeAreaView, initialWindowMetrics } from "react-native-safe-area-context";
import { IOSMetadataDoneButton, IOSMetadataSheet } from "./IOSMetadataSheet";
import { AndroidGlassAction, hasAndroidChrome } from "./AndroidChrome";
import { androidMaterialTheme } from "./android-material-theme";
import { useAppTheme } from "./app-theme";
import { sceneColorContrast } from "./ai-scene";
import type { AiSceneContextValue } from "./ai-scene-context";

export function AiSceneSettingsLink({ onPress, dark = false, accent = "#7799CC" }: { onPress(): void; dark?: boolean; accent?: string }) {
  return <Pressable accessibilityRole="button" testID="ai-scene-settings-link" accessibilityLabel="配置 AI 场景" onPress={onPress} style={s.link}>
    <Text style={{ color: dark ? "#f5f5f8" : "#17171d", fontSize: 16 }}>AI 场景</Text><Text style={{ color: accent, fontSize: 15 }}>配置与生成 ›</Text>
  </Pressable>;
}

export function AiSceneSettingsModal({ value }: { value: AiSceneContextValue }) {
  const mode = useColorScheme() === "dark" ? "dark" : "light";
  return <SafeAreaProvider initialMetrics={initialWindowMetrics} style={s.host} pointerEvents="box-none">
    <IOSMetadataSheet open={value.settingsOpen} onChange={open => { if (!open) value.closeSettings(); }} theme={mode}>
      <AiSceneSettingsContent value={value} onClose={value.closeSettings} sheet={Platform.OS === "ios"} />
    </IOSMetadataSheet>
  </SafeAreaProvider>;
}

/** AndroidMetadataSheet supplies its animated native close callback and measured bounds. */
export function AiSceneSettingsContent({ value: v, onClose, sheet = false }: {
  value: AiSceneContextValue; onClose(): void; sheet?: boolean;
}) {
  const dark = useColorScheme() === "dark", theme = useAppTheme();
  const [licensesOpen, setLicensesOpen] = useState(false);
  const [draft, setDraft] = useState(v.settings), [key, setKey] = useState("");
  useEffect(() => { if (v.settingsOpen) { setDraft(v.settings); setKey(""); setLicensesOpen(false); } }, [v.settingsOpen, v.ready]);
  const android = Platform.OS === "android", material = androidMaterialTheme(theme.id, dark);
  const ink = android ? material.ink : dark ? "#f5f5fa" : "#1c1c1e";
  const muted = android ? material.muted : dark ? "#a2a2aa" : "#6c6c74";
  const accent = android ? material.primary : theme.accent;
  const line = dark ? "#ffffff14" : "#0000000d", field = android ? material.panel : dark ? "#ffffff08" : "#00000003";
  const locked = !v.ready || v.saving, generationDisabled = locked || !draft.enabled || v.pending;
  const close = () => { if (!v.saving) onClose(); };
  const save = async (generate: boolean) => {
    if (await v.save(draft, key || undefined)) {
      setKey("");
      if (generate) void v.regenerate(); else onClose();
    }
  };
  const section = (title: string) => <Text style={[s.section, { color: muted }]}>{title}</Text>;
  const input = (label: string, text: string, onChangeText: (text: string) => void, placeholder: string, secret = false, multiline = false, last = false) => <View style={[s.field, !last && { borderBottomWidth: StyleSheet.hairlineWidth, borderBottomColor: line }]}>
    {!multiline && <Text style={[s.label, { color: muted }]}>{label}</Text>}
    <TextInput accessibilityLabel={label} value={text} onChangeText={onChangeText} placeholder={placeholder} placeholderTextColor={muted} secureTextEntry={secret}
      multiline={multiline} autoCapitalize="none" autoCorrect={false} editable={!locked} maxLength={secret ? 2048 : multiline ? 600 : 512}
      keyboardType={label === "Base URL" ? "url" : "default"} textContentType="none" importantForAutofill="no"
      style={[s.input, { color: ink }, multiline && s.multiline]} />
  </View>;
  const toggle = (label: string, enabled: boolean, change: (value: boolean) => void, detail?: string, last = false) => <View style={[s.toggle, !last && { borderBottomWidth: StyleSheet.hairlineWidth, borderBottomColor: line }]}>
    <View style={s.toggleText}><Text style={[s.rowTitle, { color: ink }]}>{label}</Text>{detail && <Text style={[s.hint, { color: muted }]}>{detail}</Text>}</View>
    <Switch accessibilityLabel={label} value={enabled} onValueChange={change} disabled={locked} trackColor={{ true: accent }} />
  </View>;
  return <SafeAreaView testID="ai-scene-settings-content" edges={sheet ? ["bottom"] : ["top", "bottom", "left", "right"]} style={[s.page, { backgroundColor: android ? material.bg : "transparent" }]}>
    <View style={[s.header, android && s.androidHeader]}>
      <Text accessibilityRole="header" style={[s.title, { color: ink }]}>AI 场景</Text>
      <View style={{ opacity: v.saving ? .45 : 1 }}>
        {android && hasAndroidChrome ? <AndroidGlassAction title="完成" dark={dark} accent={accent} onPress={close} />
          : <IOSMetadataDoneButton theme={dark ? "dark" : "light"} accent={accent} onPress={close} disabled={v.saving} accessibilityLabel="关闭 AI 场景设置" />}
      </View>
    </View>
    <KeyboardAvoidingView style={s.page} behavior={Platform.OS === "ios" ? "padding" : undefined}>
      <ScrollView testID="ai-scene-settings-scroll" style={s.page} keyboardShouldPersistTaps="handled" keyboardDismissMode="interactive" nestedScrollEnabled contentContainerStyle={s.content}>
        <Text style={[s.intro, { color: muted }]}>让歌曲拥有自己的配色与氛围。</Text>
        <View testID="ai-scene-switch-group" style={[s.group, { backgroundColor: field }]}>
          {toggle("开启 AI 场景", draft.enabled, enabled => setDraft({ ...draft, enabled }))}
          {toggle("切歌时自动生成", draft.autoGenerate, autoGenerate => setDraft({ ...draft, autoGenerate }), "为新歌曲自动生成主题", true)}
        </View>
        {section("连接服务")}
        <View testID="ai-scene-connection-group" style={[s.group, { backgroundColor: field }]}>
          {input("Base URL", draft.baseUrl, baseUrl => setDraft({ ...draft, baseUrl }), "https://你的服务地址/v1")}
          {input("API Key", key, setKey, v.hasApiKey ? "已安全保存；留空继续使用原 Key" : "填写你的 API Key", true)}
          {input("模型", draft.model, model => setDraft({ ...draft, model }), "填写模型 ID", false, false, true)}
        </View>
        <Text style={[s.footnote, { color: muted }]}>兼容 Chat Completions 接口，Key 保存在系统安全存储中。</Text>
        {section("场景偏好 · 可选")}
        <View style={[s.group, { backgroundColor: field }]}>
          {input("场景偏好（可选）", draft.prompt, prompt => setDraft({ ...draft, prompt }), "例如：温暖色调、柔和光线、缓慢流动", false, true, true)}
        </View>
        {!!v.songTitle && <View style={[s.song, { borderBottomColor: line }]}><Text style={[s.label, { color: muted }]}>当前歌曲</Text><Text numberOfLines={2} style={[s.rowTitle, { color: ink }]}>{v.songTitle}</Text></View>}
        {v.pending ? <View testID="ai-scene-pending" style={[s.pending, { backgroundColor: field }]}><ActivityIndicator color={accent} /><Text style={[s.pendingText, { color: muted }]}>正在生成场景…</Text><Pressable onPress={v.cancel} accessibilityRole="button" accessibilityLabel="取消场景生成" style={s.action}><Text style={{ color: accent }}>取消</Text></Pressable></View>
          : v.scene ? <Text testID="ai-scene-name" style={[s.result, { color: muted }]}>当前场景 · {v.scene.name}</Text> : null}
        {!!(v.preferenceError || v.error) && <Text accessibilityRole="alert" style={s.error}>{v.preferenceError || v.error}</Text>}
        <Pressable testID="ai-scene-save" accessibilityRole="button" disabled={locked} onPress={() => void save(false)} style={[s.primary, { backgroundColor: accent, opacity: locked ? .5 : 1 }]}>
          <Text style={[s.primaryText, { color: sceneColorContrast(accent, "#ffffff") >= 4.5 ? "#ffffff" : "#17171d" }]}>{v.saving ? "正在保存…" : "保存设置"}</Text>
        </Pressable>
        <Pressable testID="ai-scene-generate" accessibilityRole="button" disabled={generationDisabled} onPress={() => void save(true)} style={[s.secondary, { opacity: generationDisabled ? .4 : 1 }]}><Text style={{ color: accent, fontSize: 16 }}>{v.scene ? "为当前歌曲重新生成" : "为当前歌曲生成"}</Text></Pressable>
        <Text style={[s.footnote, { color: muted }]}>仅发送歌名、歌手、专辑、年份及场景偏好，音频和封面留在本机。生成期间照常播放。</Text>
        {v.hasApiKey && <Pressable accessibilityRole="button" disabled={locked} onPress={async () => { if (await v.save({ ...draft, enabled: false }, undefined, true)) { setKey(""); setDraft({ ...draft, enabled: false }); } }} style={s.secondary}><Text style={{ color: muted }}>清除已保存的 Key 并关闭 AI</Text></Pressable>}
        <Text style={[s.credit, { color: muted }]} testID="folia-upstream-credit">本地可视化 · Folia Latent / Paper Shaders {foliaNotices.paperVersion}</Text>
        <Pressable accessibilityRole="button" onPress={() => setLicensesOpen(!licensesOpen)} style={s.secondary}><Text style={{ color: accent, fontSize: 13 }}>开源代码与许可证</Text></Pressable>
        {licensesOpen && <ScrollView nestedScrollEnabled style={s.licenses}><Text selectable style={{ color: muted, fontSize: 12, lineHeight: 18 }}>Folia {foliaNotices.commit}{"\n"}{foliaNotices.foliaLicense}{"\n"}{foliaNotices.paperLicense}{"\n"}{foliaNotices.paperNotice}</Text></ScrollView>}
      </ScrollView>
    </KeyboardAvoidingView>
  </SafeAreaView>;
}
const s = StyleSheet.create({
  host: { position: "absolute", top: 0, bottom: 0, left: 0, right: 0 }, page: { flex: 1, minHeight: 0 },
  header: { flexShrink: 0, minHeight: 84, padding: 20, flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 12 },
  androidHeader: { minHeight: 56, paddingVertical: 6 }, title: { fontSize: 20, fontWeight: "600", flexShrink: 1 },
  content: { paddingHorizontal: 20, paddingBottom: 24, width: "100%", maxWidth: 680, alignSelf: "center" },
  intro: { fontSize: 14, lineHeight: 21, marginBottom: 18 },
  section: { fontSize: 13, lineHeight: 18, marginTop: 24, marginBottom: 8, paddingHorizontal: 14 },
  group: { borderRadius: 16, overflow: "hidden" }, toggle: { flexDirection: "row", alignItems: "center", minHeight: 56, paddingHorizontal: 14, paddingVertical: 12, gap: 12 },
  toggleText: { flex: 1, minWidth: 0 }, rowTitle: { fontSize: 16, lineHeight: 22 }, hint: { fontSize: 12, lineHeight: 18, marginTop: 3 },
  field: { paddingHorizontal: 14, paddingVertical: 12 }, label: { fontSize: 13, lineHeight: 18, marginBottom: 6 },
  input: { padding: 0, fontSize: 16, lineHeight: 24, minHeight: 28 }, multiline: { minHeight: 78, textAlignVertical: "top" },
  footnote: { fontSize: 12, lineHeight: 18, paddingHorizontal: 4, marginTop: 10 }, song: { marginTop: 20, paddingVertical: 12, borderBottomWidth: StyleSheet.hairlineWidth },
  primary: { borderRadius: 14, minHeight: 50, paddingHorizontal: 14, paddingVertical: 12, marginTop: 22, alignItems: "center", justifyContent: "center" },
  primaryText: { fontSize: 16, fontWeight: "600" }, secondary: { minHeight: 44, alignItems: "center", justifyContent: "center", marginTop: 4, paddingVertical: 10 },
  action: { minHeight: 44, paddingHorizontal: 12, justifyContent: "center", alignItems: "center" }, pending: { flexDirection: "row", alignItems: "center", gap: 10, paddingHorizontal: 14, borderRadius: 14, marginTop: 16 },
  pendingText: { flex: 1 }, result: { marginTop: 14, fontSize: 14, lineHeight: 20 }, error: { color: "#c94c53", fontSize: 14, lineHeight: 20, marginTop: 12 },
  credit: { fontSize: 12, lineHeight: 18, textAlign: "center", marginTop: 22 }, licenses: { maxHeight: 260 },
  link: { minHeight: 52, paddingHorizontal: 16, flexDirection: "row", gap: 12, justifyContent: "space-between", alignItems: "center" },
});
