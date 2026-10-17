import React from "react";
import { Platform, StyleSheet, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { iosRightLibraryGutter } from "./ios-ui-model";
import { SIDE_NAVIGATION_WIDTH } from "./SideNavigationRail";

function loadNativeBlur(): typeof import("expo-blur").BlurView | null {
  if (Platform.OS !== "ios") return null;
  try {
    if (!(globalThis as any).expo?.getViewConfig?.("ExpoBlur", "ExpoBlurView")) return null;
    return require("expo-blur").BlurView;
  } catch { return null; }
}
const NativeBlur = loadNativeBlur();
if (__DEV__ && Platform.OS === "ios") console.info("[SDA UI] side navigation background:", NativeBlur ? "full-height UIKit blur" : "fallback; native blur unavailable");

// Cover the entire right-side control region, not the capsule. Its inner edge
// meets the native library scroll-edge viewport, leaving the scrollbar outside.
export function IOSSideNavigationBackground({ dark }: { dark: boolean }) {
  const { right } = useSafeAreaInsets();
  const width = Math.max(0, right) + SIDE_NAVIGATION_WIDTH + 8 - iosRightLibraryGutter(right).paddingRight;
  return <View testID="side-navigation-background" pointerEvents="none" accessible={false} accessibilityElementsHidden
    style={{ position: "absolute", top: 0, bottom: 0, right: 0, width }}>
    {NativeBlur ? <NativeBlur pointerEvents="none" intensity={100}
      tint={dark ? "systemChromeMaterialDark" : "systemChromeMaterialLight"} style={StyleSheet.absoluteFill} /> :
      <View style={[StyleSheet.absoluteFill, { backgroundColor: dark ? "rgba(28,28,30,0.85)" : "rgba(242,242,247,0.85)" }]} />}
  </View>;
}
