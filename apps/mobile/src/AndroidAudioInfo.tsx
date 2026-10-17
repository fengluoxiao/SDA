import { androidMaterialTheme } from "./android-material-theme";
import { useAppTheme } from "./app-theme";
import React from "react";
import { ScrollView, StatusBar, StyleSheet, Text, View } from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";
import { AndroidGlassSettingsButton } from "./AndroidGlassSettingsButton";

export function AndroidAudioInfo({ rows, dark, onClose }: { rows: [string, string][]; dark: boolean; onClose(): void }) {
  const appTheme = useAppTheme();
  const c = androidMaterialTheme(appTheme.id, dark);
  return <SafeAreaView style={{ flex: 1, backgroundColor: c.bg }} edges={["top", "bottom", "left", "right"]}>
    <StatusBar translucent backgroundColor="transparent" barStyle={dark ? "light-content" : "dark-content"} />
    <View style={s.header}><View style={s.back}><AndroidGlassSettingsButton back dark={dark} onPress={onClose} /></View>
      <Text accessibilityRole="header" style={[s.title, { color: c.ink }]}>音频信息</Text></View>
    <ScrollView contentContainerStyle={s.content}>
      <Text style={[s.section, { color: c.primary }]}>文件与播放</Text>
      {rows.map(([name, value]) => <View key={name} style={s.row}>
        <Text style={[s.label, { color: c.muted }]}>{name}</Text>
        <Text selectable style={[s.value, { color: c.ink }]}>{value}</Text>
      </View>)}
    </ScrollView>
  </SafeAreaView>;
}
const s = StyleSheet.create({
  header: { height: 64, flexDirection: "row", alignItems: "center", gap: 12, paddingHorizontal: 8 },
  back: { width: 48, height: 48, alignItems: "center", justifyContent: "center" }, title: { fontSize: 22, lineHeight: 28 },
  content: { paddingTop: 20, paddingBottom: 32 }, section: { fontSize: 14, lineHeight: 20, fontWeight: "500", paddingHorizontal: 24, marginBottom: 8 },
  row: { minHeight: 72, paddingVertical: 12, paddingHorizontal: 24, gap: 4 }, label: { fontSize: 14, lineHeight: 20 }, value: { fontSize: 16, lineHeight: 24 },
});
