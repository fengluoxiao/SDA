import React from "react";
import { Text, View } from "react-native";
import { swiftUI } from "./IOSSystemTabs";

// Display only: a real SF Symbol, not a button that changes volume.
export function IOSVolumeSymbol({ volume, color }: { volume: number; color: string }) {
  const hasImage = !!swiftUI && !!(globalThis as any).expo?.getViewConfig?.("ExpoUI", "ImageView");
  const name = volume <= 0 ? "speaker.slash.fill" : "speaker.wave.2.fill";
  return <View accessible accessibilityLabel={volume <= 0 ? "已静音" : "音量"} style={{ width: 24, height: 24, alignItems: "center", justifyContent: "center" }}>
    {hasImage && swiftUI ? <swiftUI.Host style={{ width: 24, height: 24 }}>
      <swiftUI.Image systemName={name} size={18} color={color} variableValue={Math.max(0, Math.min(1, volume))} />
    </swiftUI.Host> : <Text style={{ color, fontSize: 18 }}>{volume <= 0 ? "🔇" : "🔊"}</Text>}
  </View>;
}
