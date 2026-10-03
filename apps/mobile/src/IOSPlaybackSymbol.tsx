import React from "react";
import { Text, View } from "react-native";
import { swiftUI } from "./IOSSystemTabs";

// Shared by Expo Go and the packaged app; the parent owns the action and label.
export function IOSPlaybackSymbol({ playing, color }: { playing: boolean; color: string }) {
  const hasImage = !!swiftUI && !!(globalThis as any).expo?.getViewConfig?.("ExpoUI", "ImageView");
  return <View pointerEvents="none" accessible={false} accessibilityElementsHidden importantForAccessibility="no-hide-descendants" style={{ width: 24, height: 24, alignItems: "center", justifyContent: "center" }}>
    {hasImage && swiftUI ? <swiftUI.Host style={{ width: 24, height: 24 }}>
      <swiftUI.Image systemName={playing ? "pause.fill" : "play.fill"} size={20} color={color} />
    </swiftUI.Host> : <Text style={{ color, fontSize: 20 }}>{playing ? "Ⅱ" : "▶"}</Text>}
  </View>;
}

export function IOSSkipSymbol({ direction, color }: { direction: "previous" | "next"; color: string }) {
  const hasImage = !!swiftUI && !!(globalThis as any).expo?.getViewConfig?.("ExpoUI", "ImageView");
  return <View pointerEvents="none" accessible={false} accessibilityElementsHidden style={{ width: 28, height: 28, alignItems: "center", justifyContent: "center" }}>
    {hasImage && swiftUI ? <swiftUI.Host style={{ width: 28, height: 28 }}>
      <swiftUI.Image systemName={direction === "previous" ? "backward.fill" : "forward.fill"} size={24} color={color} />
    </swiftUI.Host> : <Text style={{ color, fontSize: 22 }}>{direction === "previous" ? "◀◀" : "▶▶"}</Text>}
  </View>;
}
