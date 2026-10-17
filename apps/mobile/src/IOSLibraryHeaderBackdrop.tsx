import React, { useState } from "react";
import { StyleSheet, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { swiftUI } from "./IOSSystemTabs";

// Decorative backdrop only. The existing RN header keeps its labels/actions,
// while the library scrolls beneath it. The root provider owns screen insets.
export function IOSLibraryHeaderBackdrop({ dark }: { dark: boolean }) {
  const insets = useSafeAreaInsets();
  const [size, setSize] = useState({ width: 0, height: 0 });
  const native = swiftUI && ["HostView", "VStackView"].every(name => (globalThis as any).expo?.getViewConfig?.("ExpoUI", name));
  const modifiers = native ? require("@expo/ui/swift-ui/modifiers") as typeof import("@expo/ui/swift-ui/modifiers") : null;
  return <View pointerEvents="none" accessible={false} accessibilityElementsHidden
    style={[StyleSheet.absoluteFill, { top: -insets.top, left: -insets.left - 18, right: -insets.right - 18,
      backgroundColor: native ? "transparent" : dark ? "rgba(28,28,30,0.9)" : "rgba(242,242,247,0.9)" }]}
    onLayout={({ nativeEvent: { layout } }) => setSize(previous => previous.width === layout.width && previous.height === layout.height ? previous : { width: layout.width, height: layout.height })}>
    {native && swiftUI && modifiers && size.width > 0 && size.height > 0 && <swiftUI.Host ignoreSafeArea="container" style={size} colorScheme={dark ? "dark" : "light"}>
      <swiftUI.VStack modifiers={[modifiers.frame(size), modifiers.background({ type: "material", material: "bar" })]}>{null}</swiftUI.VStack>
    </swiftUI.Host>}
  </View>;
}
