import React from "react";
import { Platform, Text, View } from "react-native";
import { AndroidSymbol } from "./AndroidSymbol";
import { swiftUI } from "./IOSSystemTabs";

// Shared by Expo Go and the packaged app; the parent owns the action and label.
export function IOSPlaybackSymbol({ playing, color, size = 20 }: { playing: boolean; color: string; size?: number }) {
  if (Platform.OS === "android") return <AndroidSymbol name={playing ? "pause.fill" : "play.fill"} color={color} size={size >= 30 ? 52 : size} />;
  const hasImage = !!swiftUI && !!(globalThis as any).expo?.getViewConfig?.("ExpoUI", "ImageView") && !!(globalThis as any).expo?.getViewConfig?.("ExpoUI", "VStackView");
  const modifiers = hasImage ? require("@expo/ui/swift-ui/modifiers") as typeof import("@expo/ui/swift-ui/modifiers") : null;
  return <View pointerEvents="none" accessible={false} accessibilityElementsHidden importantForAccessibility="no-hide-descendants" style={{ width: size + 4, height: size + 4, alignItems: "center", justifyContent: "center" }}>
    {hasImage && swiftUI && modifiers ? <swiftUI.Host ignoreSafeArea="container" seedColor={color} style={{ width: size + 4, height: size + 4 }}>
      <swiftUI.VStack modifiers={[modifiers.frame({ width: size + 4, height: size + 4 }), modifiers.foregroundColor(color), modifiers.tint(color)]}>
        <swiftUI.Image systemName={playing ? "pause.fill" : "play.fill"} size={size} color={color} />
      </swiftUI.VStack>
    </swiftUI.Host> : <Text style={{ color, fontSize: size }}>{playing ? "Ⅱ" : "▶"}</Text>}
  </View>;
}

export function IOSSkipSymbol({ direction, color, size = 24 }: { direction: "previous" | "next"; color: string; size?: number }) {
  if (Platform.OS === "android") return <AndroidSymbol name={direction === "previous" ? "backward.fill" : "forward.fill"} color={color} size={size >= 30 ? 42 : size} />;
  const hasImage = !!swiftUI && !!(globalThis as any).expo?.getViewConfig?.("ExpoUI", "ImageView");
  return <View pointerEvents="none" accessible={false} accessibilityElementsHidden style={{ width: size + 4, height: size + 4, alignItems: "center", justifyContent: "center" }}>
    {hasImage && swiftUI ? <swiftUI.Host style={{ width: size + 4, height: size + 4 }} colorScheme={color.startsWith("#1") || color.startsWith("#2") ? "light" : "dark"}>
      <swiftUI.Image systemName={direction === "previous" ? "backward.fill" : "forward.fill"} size={size} color={color} />
    </swiftUI.Host> : <Text style={{ color, fontSize: size }}>{direction === "previous" ? "◀◀" : "▶▶"}</Text>}
  </View>;
}

// Decorative only: the React Native parent retains the full touch target.
export function IOSModeSymbol({ mode, color }: { mode: "sequence" | "repeat-all" | "repeat-one"; color: string }) {
  const hasImage = !!swiftUI && !!(globalThis as any).expo?.getViewConfig?.("ExpoUI", "ImageView");
  const symbol = mode === "repeat-one" ? "repeat.1" : mode === "repeat-all" ? "repeat" : "list.bullet";
  return <View pointerEvents="none" accessible={false} accessibilityElementsHidden importantForAccessibility="no-hide-descendants" style={{ width: 24, height: 24, alignItems: "center", justifyContent: "center" }}>
    {hasImage && swiftUI ? <swiftUI.Host style={{ width: 24, height: 24 }}>
      <swiftUI.Image systemName={symbol} size={20} color={color} />
    </swiftUI.Host> : <Text style={{ color, fontSize: 20 }}>{mode === "repeat-one" ? "↻1" : mode === "repeat-all" ? "↻" : "≡"}</Text>}
  </View>;
}
