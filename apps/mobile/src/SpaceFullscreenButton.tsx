import React from "react";
import { Platform, Pressable, StyleSheet, Text, View, useColorScheme, type StyleProp, type ViewStyle } from "react-native";
import { IOSGlassIconButton } from "./IOSNativeSettings";
import { AndroidGlassSettingsButton, hasAndroidGlassSettingsButton } from "./AndroidGlassSettingsButton";

/** The same native glass surface as the player settings action on each platform. */
export function SpaceFullscreenButton({ expanded = false, onPress, style, accessibilityLabel, testID = "space-fullscreen-button" }: {
  expanded?: boolean; onPress(): void; style?: StyleProp<ViewStyle>; accessibilityLabel?: string; testID?: string;
}) {
  const dark = useColorScheme() === "dark", label = accessibilityLabel || (expanded ? "退出空间全屏" : "空间全屏");
  const ios = Platform.OS === "ios" ? IOSGlassIconButton({ onPress, symbol: expanded ? "arrow.down.right.and.arrow.up.left" : "arrow.up.left.and.arrow.down.right", label }) : null;
  return <View testID={testID} nativeID="space-glass-fullscreen" style={style}>
    {ios || (Platform.OS === "android" && hasAndroidGlassSettingsButton
      ? <AndroidGlassSettingsButton fullscreen expanded={expanded} onPress={onPress} accessibilityLabel={label} />
      : <Pressable accessibilityRole="button" accessibilityLabel={label} onPress={onPress} style={[s.fallback, { backgroundColor: dark ? "#ffffff20" : "#ffffffb8" }]}><Text style={{ color: dark ? "#ffffff" : "#25252b", fontSize: 24 }}>{expanded ? "⤡" : "⤢"}</Text></Pressable>)}
  </View>;
}
const s = StyleSheet.create({ fallback: { width: 44, height: 44, borderRadius: 22, alignItems: "center", justifyContent: "center" } });
