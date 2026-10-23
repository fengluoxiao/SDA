import { spatialHeadIndicator } from "./spatial-head-indicator";
import React from "react";
import { requireNativeView } from "expo";
import { Platform, type NativeSyntheticEvent, useColorScheme } from "react-native";

interface Props {
  compactButton?: boolean; settingsButton: boolean; backButton: boolean; spatialButton: boolean; accessibilityLabel?: string; dark: boolean; glassDark: boolean; busy: boolean;
  spatialEnabled?: boolean; spatialTracking?: boolean; headYawDegrees?: number; accentColorValue?: string;
  onExpand?(): void; onSpatialSelect?(event: NativeSyntheticEvent<{ enabled: boolean }>): void; style: { width: number; height: number; marginRight?: number };
}
const NativeButton = Platform.OS === "android" && (globalThis as any).expo?.modules?.SdaAndroidChrome
  ? (requireNativeView as unknown as <P>(name: string) => React.ComponentType<P>)<Props>("SdaAndroidChrome") : null;

export function AndroidGlassSettingsButton({ onPress, onSpatialSelect, back = false, spatial = false, compact = false, disabled = false, accessibilityLabel, sideDock = false, spatialEnabled = false, trackingEnabled = false, headYaw = 0, accent = "#7799cc" }: { onPress?(): void; onSpatialSelect?(enabled: boolean): void; back?: boolean; spatial?: boolean; compact?: boolean; disabled?: boolean; accessibilityLabel?: string; sideDock?: boolean; spatialEnabled?: boolean; trackingEnabled?: boolean; headYaw?: number; accent?: string }) {
  const dark = useColorScheme() === "dark";
  const indicator = spatialHeadIndicator(spatialEnabled, trackingEnabled, headYaw, accent, "");
  return NativeButton ? <NativeButton compactButton={compact} settingsButton backButton={back} spatialButton={spatial} spatialEnabled={spatial && indicator.active} spatialTracking={spatial && indicator.tracking} headYawDegrees={indicator.poseDegrees} accentColorValue={accent} accessibilityLabel={accessibilityLabel} dark={dark} glassDark={dark} busy={disabled} onExpand={onPress} onSpatialSelect={event => {
    if (!disabled && typeof event.nativeEvent.enabled === "boolean") onSpatialSelect?.(event.nativeEvent.enabled);
  }} style={{ width: compact ? 48 : 44, height: compact ? 48 : 44, ...(spatial ? { marginRight: sideDock || compact ? 0 : 8 } : {}) }} /> : null;
}
