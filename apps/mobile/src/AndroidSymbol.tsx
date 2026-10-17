import React from "react";
import { Platform, Text, View } from "react-native";
import { requireNativeView } from "expo";

interface SymbolProps { name: string; tint: string; style: { width: number; height: number }; pointerEvents: "none" }
const NativeSymbol = Platform.OS === "android" && (globalThis as any).expo?.modules?.SdaAndroidSymbol
  ? (requireNativeView as unknown as <P>(name: string) => React.ComponentType<P>)<SymbolProps>("SdaAndroidSymbol") : null;
export function AndroidSymbol({ name, color, size = 24 }: { name: string; color: string; size?: number }) {
  return <View pointerEvents="none" accessible={false} importantForAccessibility="no-hide-descendants" style={{ width: size + 4, height: size + 4, alignItems: "center", justifyContent: "center" }}>
    {NativeSymbol ? <NativeSymbol name={name} tint={color} pointerEvents="none" style={{ width: size, height: size }} /> : <Text style={{ color, fontSize: size }}>{name === "pause.fill" ? "Ⅱ" : name === "play.fill" ? "▶" : "•"}</Text>}
  </View>;
}
