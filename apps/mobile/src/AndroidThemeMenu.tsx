import React, { useState } from "react";
import { requireNativeView } from "expo";
import { Platform, Pressable, StyleSheet, Text, View, type NativeSyntheticEvent, type StyleProp, type ViewStyle } from "react-native";
import { CHARACTER_THEMES, useAppTheme, validTheme } from "./app-theme";
import { androidMaterialTheme } from "./android-material-theme";
import { AndroidSymbol } from "./AndroidSymbol";

interface AnchorProps {
  presentation: "theme-anchor"; dark: boolean; accentColorValue: string; themeMenuItems: string; themeMenuSelected: string; themeMenuRequest: number;
  pointerEvents: "none"; accessible: false; style: StyleProp<ViewStyle>;
  onThemeSelect(event: NativeSyntheticEvent<{ id: string }>): void; onThemeDismiss(): void;
}
const NativeAnchor = Platform.OS === "android" && (globalThis as any).expo?.modules?.SdaAndroidChrome
  ? (requireNativeView as unknown as <P>(name: string) => React.ComponentType<P>)<AnchorProps>("SdaAndroidChrome") : null;
const items = JSON.stringify(CHARACTER_THEMES.map(item => ({ id: item.id, label: item.name, color: item.color })));

// The invisible native anchor ends at the disclosure indicator. The popup itself
// is android.widget.PopupMenu, styled and positioned by the Android framework.
export function AndroidThemeMenu({ dark, onFallbackOpen }: { dark: boolean; onFallbackOpen(): void }) {
  const theme = useAppTheme();
  const c = androidMaterialTheme(theme.id, dark);
  const current = CHARACTER_THEMES.find(item => item.id === theme.id) ?? CHARACTER_THEMES[0];
  const [expanded, setExpanded] = useState(false);
  const [request, setRequest] = useState(0);
  return <View>
    <Pressable accessibilityRole="button" accessibilityLabel={`主题色，${current.name}`} accessibilityState={{ expanded }}
      onPress={() => { if (!NativeAnchor) onFallbackOpen(); else if (!expanded) { setExpanded(true); setRequest(value => value + 1); } }}
      android_ripple={{ color: dark ? "#ffffff18" : "#7799CC18" }} style={s.row}>
      <View style={[s.dot, { backgroundColor: current.color }]} />
      <Text numberOfLines={1} style={[s.name, { color: c.ink }]}>{current.name}</Text>
      <AndroidSymbol name="chevron.up.chevron.down" size={20} color={c.muted} />
    </Pressable>
    {NativeAnchor && <NativeAnchor presentation="theme-anchor" dark={dark} accentColorValue={c.primary} themeMenuItems={items} themeMenuSelected={theme.id} themeMenuRequest={request}
      pointerEvents="none" accessible={false} style={s.anchor}
      onThemeSelect={event => { setExpanded(false); const id = event.nativeEvent?.id; if (validTheme(id)) theme.setTheme(id); }}
      onThemeDismiss={() => setExpanded(false)} />}
  </View>;
}
const s = StyleSheet.create({
  row: { minHeight: 64, paddingVertical: 12, paddingHorizontal: 24, flexDirection: "row", alignItems: "center", gap: 12 },
  dot: { width: 18, height: 18, borderRadius: 9 }, name: { flex: 1, fontSize: 16, lineHeight: 24 },
  anchor: { position: "absolute", right: 24, top: 0, bottom: 0, width: 24 },
});
