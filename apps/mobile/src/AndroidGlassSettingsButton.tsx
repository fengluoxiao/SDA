import { spatialHeadIndicator } from "./spatial-head-indicator";
import React from "react";
import { requireNativeView } from "expo";
import { Platform, useColorScheme } from "react-native";

interface Props {
  settingsButton: boolean; backButton: boolean; spatialButton: boolean; accessibilityLabel?: string; dark: boolean; glassDark: boolean; busy: boolean;
  spatialEnabled?: boolean; spatialTracking?: boolean; headYawDegrees?: number; accentColorValue?: string;
  onExpand(): void; style: { width: number; height: number; marginRight?: number };
}
const NativeButton = Platform.OS === "android" && (globalThis as any).expo?.modules?.SdaAndroidChrome
  ? (requireNativeView as unknown as <P>(name: string) => React.ComponentType<P>)<Props>("SdaAndroidChrome") : null;

export function AndroidGlassSettingsButton({ onPress, back = false, spatial = false, disabled = false, accessibilityLabel, sideDock = false, spatialEnabled = false, trackingEnabled = false, headYaw = 0, accent = "#7799cc" }: { onPress(): void; back?: boolean; spatial?: boolean; disabled?: boolean; accessibilityLabel?: string; sideDock?: boolean; spatialEnabled?: boolean; trackingEnabled?: boolean; headYaw?: number; accent?: string }) {
  const dark = useColorScheme() === "dark";
  const indicator = spatialHeadIndicator(spatialEnabled, trackingEnabled, headYaw, accent, "");
  return NativeButton ? <NativeButton settingsButton backButton={back} spatialButton={spatial} spatialEnabled={spatial && indicator.active} spatialTracking={spatial && indicator.tracking} headYawDegrees={indicator.poseDegrees} accentColorValue={accent} accessibilityLabel={accessibilityLabel} dark={dark} glassDark={dark} busy={disabled} onExpand={onPress} style={{ width: 44, height: 44, ...(spatial ? { marginRight: sideDock ? 0 : 8 } : {}) }} /> : null;
}
