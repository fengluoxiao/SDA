import React, { useState } from "react";
import { Platform, StyleSheet, View } from "react-native";
import { swiftUI } from "./IOSSystemTabs";

// The system accessory supplies its own glass in portrait. Only the floating
// in-page mini-player needs this background; both share the same content/actions.
export function IOSMiniPlayerBackdrop({ dark }: { dark: boolean }) {
  const [size, setSize] = useState({ width: 0, height: 0 });
  const native = Platform.OS === "ios" && Number(Platform.Version) >= 26 && swiftUI &&
    ["HostView", "VStackView"].every(name => (globalThis as any).expo?.getViewConfig?.("ExpoUI", name));
  const modifiers = native ? require("@expo/ui/swift-ui/modifiers") as typeof import("@expo/ui/swift-ui/modifiers") : null;
  return <View pointerEvents="none" accessible={false} accessibilityElementsHidden
    style={[StyleSheet.absoluteFill, { borderRadius: 26, backgroundColor: native ? "transparent" : dark ? "rgba(55,55,60,0.65)" : "rgba(245,245,250,0.65)" }]}
    onLayout={({ nativeEvent: { layout } }) => setSize(previous => previous.width === layout.width && previous.height === layout.height ? previous : { width: layout.width, height: layout.height })}>
    {native && swiftUI && modifiers && size.width > 0 && size.height > 0 && <swiftUI.Host ignoreSafeArea="container" colorScheme={dark ? "dark" : "light"} style={size}>
      <swiftUI.VStack modifiers={[modifiers.frame(size), modifiers.glassEffect({ glass: { variant: "regular" }, shape: "capsule" })]}>{null}</swiftUI.VStack>
    </swiftUI.Host>}
  </View>;
}
