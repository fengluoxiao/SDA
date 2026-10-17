import React from "react";
import { Animated, Image, Platform, StyleSheet, View, type StyleProp, type ViewStyle } from "react-native";

// Expo's own view registry safely guards older installed app binaries. Do not
// probe experimental RN screen managers: Expo Go can throw during that probe.
function loadBlur(): typeof import("expo-blur").BlurView | null {
  if (Platform.OS !== "ios") return null;
  try {
    if (!(globalThis as any).expo?.getViewConfig?.("ExpoBlur", "ExpoBlurView")) return null;
    return require("expo-blur").BlurView;
  } catch { return null; }
}
function loadMask(): typeof import("@expo/ui/community/masked-view").MaskedView | null {
  if (Platform.OS !== "ios") return null;
  try {
    if (!["HostView", "MaskView", "RNHostView", "SlotView"].every(name => (globalThis as any).expo?.getViewConfig?.("ExpoUI", name))) return null;
    return require("@expo/ui/community/masked-view").MaskedView;
  } catch { return null; }
}
const NativeBlur = loadBlur();
const SoftMask = NativeBlur ? loadMask() : null;
const edgeHeight = 16;
if (__DEV__ && Platform.OS === "ios") console.info("[SDA UI] header blur:", NativeBlur ? (SoftMask ? "system chrome / soft edge" : "system chrome / mask unavailable") : "opaque fallback; native blur unavailable");

export function IOSHeaderBlur({ dark, style, opacity = 1 }: { dark: boolean; style?: StyleProp<ViewStyle>; opacity?: number | Animated.Value | Animated.AnimatedInterpolation<number> }) {
  if (!NativeBlur) return <View pointerEvents="none" accessible={false} accessibilityElementsHidden
    style={[style, { backgroundColor: dark ? "#1c1c1e" : "#f2f2f7" }]} />;
  const blur = <NativeBlur intensity={100} tint={dark ? "systemChromeMaterialDark" : "systemChromeMaterialLight"} style={StyleSheet.absoluteFill} />;
  return <Animated.View pointerEvents="none" accessible={false} accessibilityElementsHidden
    style={[style, { opacity }, SoftMask && { bottom: -edgeHeight }]}>
    {SoftMask ? <SoftMask style={StyleSheet.absoluteFill} maskElement={
      <View style={{ flex: 1 }}>
        <View style={{ flex: 1, backgroundColor: "#000" }} />
        <Image source={require("../assets/header-blur-edge-mask.png")} resizeMode="stretch" style={{ width: "100%", height: edgeHeight }} />
      </View>
    }>{blur}</SoftMask> : blur}
  </Animated.View>;
}
