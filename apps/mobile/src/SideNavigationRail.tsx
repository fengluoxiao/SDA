import React from "react";
import { Platform, Pressable, StyleSheet, View } from "react-native";
import { IOS_TABS } from "./ios-ui-model";

export const SIDE_NAVIGATION_WIDTH = 76;
const nativeUI: typeof import("@expo/ui/swift-ui") | null = Platform.OS === "ios" &&
  Number(Platform.Version) >= 26 && (globalThis as any).expo?.modules?.ExpoUI &&
  ["HostView", "VStackView"].every(name => (globalThis as any).expo?.getViewConfig?.("ExpoUI", name))
    ? require("@expo/ui/swift-ui") : null;

export function SideNavigationRail({ selected, onChange, accent, dark, renderIcon }: {
  selected: number; onChange(index: number): void; accent: string; dark: boolean;
  renderIcon(index: number, color: string): React.ReactNode;
}) {
  const material = nativeUI ? require("@expo/ui/swift-ui/modifiers") as typeof import("@expo/ui/swift-ui/modifiers") : null;
  return <View style={[s.rail, { backgroundColor: nativeUI ? "transparent" : dark ? "rgba(55,55,60,.92)" : "rgba(245,245,250,.92)" }]}>
    {nativeUI && material && <View pointerEvents="none" style={StyleSheet.absoluteFill}>
      <nativeUI.Host style={{ width: 64, height: 184 }} colorScheme={dark ? "dark" : "light"}>
        <nativeUI.VStack modifiers={[material.frame({ width: 64, height: 184 }), material.glassEffect({ glass: { variant: "regular" }, shape: "capsule" })]}>{null}</nativeUI.VStack>
      </nativeUI.Host>
    </View>}
    {IOS_TABS.map((label, index) => <Pressable key={label} accessibilityRole="tab" accessibilityLabel={label}
      accessibilityState={{ selected: index === selected }} onPress={() => onChange(index)}
      style={({ pressed }) => [s.tab, { backgroundColor: selected === index ? accent : "transparent", opacity: pressed ? .65 : 1 }]}>
      {renderIcon(index, selected === index ? "#ffffff" : dark ? "#f5f5f7" : "#1c1c1e")}
    </Pressable>)}
  </View>;
}
const s = StyleSheet.create({
  rail: { width: 64, height: 184, borderRadius: 32, padding: 6, gap: 8 },
  tab: { width: 52, height: 52, borderRadius: 26, alignItems: "center", justifyContent: "center" },
});
