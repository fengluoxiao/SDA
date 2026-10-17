import React from "react";
import { Platform, Pressable, StyleSheet, View } from "react-native";
import { IOS_TABS } from "./ios-ui-model";

export const SIDE_NAVIGATION_WIDTH = 60;
const nativeUI: typeof import("@expo/ui/swift-ui") | null = Platform.OS === "ios" &&
  Number(Platform.Version) >= 26 && (globalThis as any).expo?.modules?.ExpoUI &&
  ["HostView", "VStackView", "Button"].every(name => (globalThis as any).expo?.getViewConfig?.("ExpoUI", name))
    ? require("@expo/ui/swift-ui") : null;
const symbols = ["play.circle.fill", "music.note.list", "cube.transparent"] as const;

export function SideNavigationRail({ selected, onChange, accent, dark, renderIcon }: {
  selected: number; onChange(index: number): void; accent: string; dark: boolean;
  renderIcon(index: number, color: string): React.ReactNode;
}) {
  if (nativeUI) {
    const { frame, padding, glassEffect, buttonStyle, labelStyle, foregroundStyle, font, accessibilityLabel, accessibilityValue } =
      require("@expo/ui/swift-ui/modifiers") as typeof import("@expo/ui/swift-ui/modifiers");
    return <nativeUI.Host style={{ width: 48, height: 140 }} colorScheme={dark ? "dark" : "light"}>
      <nativeUI.VStack spacing={2} modifiers={[padding({ all: 2 }), frame({ width: 48, height: 140 }), glassEffect({ glass: { variant: "clear" }, shape: "capsule" })]}>
        {IOS_TABS.map((label, index) => <nativeUI.Button key={label} label={label} systemImage={symbols[index]} onPress={() => onChange(index)}
          modifiers={[buttonStyle("plain"), labelStyle("iconOnly"), font({ size: 20 }), frame({ width: 44, height: 44 }),
            foregroundStyle(selected === index ? accent : dark ? "#f5f5f7" : "#1c1c1e"),
            accessibilityLabel(label), accessibilityValue(selected === index ? "已选择" : "未选择"),
            ...(selected === index ? [glassEffect({ glass: { variant: "clear", interactive: true }, shape: "circle" })] : [])]} />)}
      </nativeUI.VStack>
    </nativeUI.Host>;
  }
  return <View style={[s.rail, { backgroundColor: dark ? "rgba(55,55,60,.65)" : "rgba(245,245,250,.65)" }]}>
    {IOS_TABS.map((label, index) => <Pressable key={label} accessibilityRole="tab" accessibilityLabel={label}
      accessibilityState={{ selected: index === selected }} onPress={() => onChange(index)}
      style={({ pressed }) => [s.tab, { backgroundColor: selected === index ? (dark ? "#ffffff22" : "#0000000a") : "transparent", opacity: pressed ? .65 : 1 }]}>
      {renderIcon(index, selected === index ? accent : dark ? "#f5f5f7" : "#1c1c1e")}
    </Pressable>)}
  </View>;
}
const s = StyleSheet.create({
  rail: { width: 48, height: 140, borderRadius: 24, padding: 2, gap: 2 },
  tab: { width: 44, height: 44, borderRadius: 22, alignItems: "center", justifyContent: "center" },
});
