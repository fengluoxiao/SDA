import React, { useState } from "react";
import { Platform, Pressable, StyleSheet, Text, View } from "react-native";
import { CHARACTER_THEMES, useAppTheme } from "./app-theme";
import { androidMaterialTheme } from "./android-material-theme";
import { AndroidSymbol } from "./AndroidSymbol";

const compose: typeof import("@expo/ui/jetpack-compose") | null = Platform.OS === "android" && (globalThis as any).expo?.modules?.ExpoUI
  ? require("@expo/ui/jetpack-compose") : null;
const modifiers: typeof import("@expo/ui/jetpack-compose/modifiers") | null = compose
  ? require("@expo/ui/jetpack-compose/modifiers") : null;

// The row stays in the settings page; Material DropdownMenu presents its choices
// in a native popup window rather than expanding the settings scroll content.
export function AndroidThemeMenu({ dark, onFallbackOpen }: { dark: boolean; onFallbackOpen(): void }) {
  const theme = useAppTheme();
  const c = androidMaterialTheme(theme.id, dark);
  const current = CHARACTER_THEMES.find(item => item.id === theme.id) ?? CHARACTER_THEMES[0];
  const [expanded, setExpanded] = useState(false);
  const [width, setWidth] = useState(0);
  const native = !!compose && !!modifiers && width > 0;
  const trigger = <Pressable accessibilityRole="button" accessibilityLabel={`主题色，${current.name}`} accessibilityState={{ expanded }}
    onPress={() => native ? setExpanded(true) : onFallbackOpen()}
    android_ripple={{ color: dark ? "#ffffff18" : "#7799CC18" }} style={[s.row, width > 0 && { width }]}>
    <View style={[s.dot, { backgroundColor: current.color }]} />
    <Text numberOfLines={1} style={[s.name, { color: c.ink }]}>{current.name}</Text>
    <AndroidSymbol name="chevron.up.chevron.down" size={20} color={c.muted} />
  </Pressable>;
  return <View style={s.host} onLayout={event => setWidth(event.nativeEvent.layout.width)}>
    {compose && modifiers && width > 0 ? <compose.Host colorScheme={dark ? "dark" : "light"} seedColor={theme.accent} matchContents={{ vertical: true }} style={{ width }}>
      <compose.DropdownMenu expanded={expanded} onDismissRequest={() => setExpanded(false)} color={c.panel} style={{ width }}>
        <compose.DropdownMenu.Trigger><compose.RNHostView matchContents>{trigger}</compose.RNHostView></compose.DropdownMenu.Trigger>
        <compose.DropdownMenu.Items>{CHARACTER_THEMES.map(item => <compose.DropdownMenuItem key={item.id}
          onClick={() => { setExpanded(false); theme.setTheme(item.id); }}>
          <compose.DropdownMenuItem.LeadingIcon><compose.Box modifiers={[modifiers.size(18, 18), modifiers.clip(modifiers.Shapes.Circle), modifiers.background(item.color)]} /></compose.DropdownMenuItem.LeadingIcon>
          <compose.DropdownMenuItem.Text><compose.Text color={c.ink} style={{ fontSize: 16 }}>{item.name}</compose.Text></compose.DropdownMenuItem.Text>
          {theme.id === item.id && <compose.DropdownMenuItem.TrailingIcon><compose.Text color={c.primary} style={{ fontSize: 16 }}>✓</compose.Text></compose.DropdownMenuItem.TrailingIcon>}
        </compose.DropdownMenuItem>)}</compose.DropdownMenu.Items>
      </compose.DropdownMenu>
    </compose.Host> : trigger}
  </View>;
}

const s = StyleSheet.create({
  host: { width: "100%" },
  row: { minHeight: 64, paddingVertical: 12, paddingHorizontal: 24, flexDirection: "row", alignItems: "center", gap: 12 },
  dot: { width: 18, height: 18, borderRadius: 9 },
  name: { flex: 1, fontSize: 16, lineHeight: 24 },
});
