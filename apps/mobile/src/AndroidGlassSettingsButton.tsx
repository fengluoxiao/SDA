import React from "react";
import { requireNativeView } from "expo";
import { Platform, useColorScheme } from "react-native";

interface Props {
  settingsButton: boolean; backButton: boolean; spatialButton: boolean; accessibilityLabel?: string; dark: boolean; glassDark: boolean; busy: boolean;
  onExpand(): void; style: { width: number; height: number; marginRight?: number };
}
const NativeButton = Platform.OS === "android" && (globalThis as any).expo?.modules?.SdaAndroidChrome
  ? (requireNativeView as unknown as <P>(name: string) => React.ComponentType<P>)<Props>("SdaAndroidChrome") : null;

export function AndroidGlassSettingsButton({ onPress, back = false, spatial = false, disabled = false, accessibilityLabel, sideDock = false }: { onPress(): void; back?: boolean; spatial?: boolean; disabled?: boolean; accessibilityLabel?: string; sideDock?: boolean }) {
  const dark = useColorScheme() === "dark";
  return NativeButton ? <NativeButton settingsButton backButton={back} spatialButton={spatial} accessibilityLabel={accessibilityLabel} dark={dark} glassDark={dark} busy={disabled} onExpand={onPress} style={{ width: 44, height: 44, ...(spatial ? { marginRight: sideDock ? 0 : 8 } : {}) }} /> : null;
}
