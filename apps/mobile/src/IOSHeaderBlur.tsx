import React from "react";
import { Animated, Platform, StyleSheet, View, type StyleProp, type ViewStyle } from "react-native";

// Expo's own view registry safely guards older installed app binaries. Do not
// probe experimental RN screen managers: Expo Go can throw during that probe.
function loadBlur(): typeof import("expo-blur").BlurView | null {
  if (Platform.OS !== "ios") return null;
  try {
    if (!(globalThis as any).expo?.getViewConfig?.("ExpoBlur", "ExpoBlurView")) return null;
    return require("expo-blur").BlurView;
  } catch { return null; }
}
const NativeBlur = loadBlur();
if (__DEV__ && Platform.OS === "ios") console.info("[SDA UI] header blur:", NativeBlur ? "ExpoBlur native" : "opaque fallback; native blur unavailable");

export function IOSHeaderBlur({ dark, style, opacity = 1 }: { dark: boolean; style?: StyleProp<ViewStyle>; opacity?: number | Animated.Value | Animated.AnimatedInterpolation<number> }) {
  return NativeBlur ? <Animated.View pointerEvents="none" accessible={false} accessibilityElementsHidden style={[style, { opacity }]}>
    <NativeBlur intensity={100} tint={dark ? "systemMaterialDark" : "systemMaterialLight"} style={StyleSheet.absoluteFill} />
  </Animated.View> :
    <View pointerEvents="none" accessible={false} accessibilityElementsHidden
      style={[style, { backgroundColor: dark ? "#1c1c1e" : "#f2f2f7" }]} />;
}
