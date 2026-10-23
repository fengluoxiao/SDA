import React from "react";
import { Platform, processColor, type NativeSyntheticEvent } from "react-native";
import { requireNativeView } from "expo";

export const hasSystemVolumeSlider = (Platform.OS === "ios" || Platform.OS === "android") && !!(globalThis as any).expo?.modules?.SdaSystemVolume;
type NativeProps = {
 activeColorArgb: number; inactiveColorArgb: number;
 onChange(event: NativeSyntheticEvent<{ value: number }>): void;
 onTracking(event: NativeSyntheticEvent<{ active: boolean }>): void;
 style: { flex: number; height: number };
};
const NativeSlider = hasSystemVolumeSlider
 ? (requireNativeView as unknown as <P>(name: string) => React.ComponentType<P>)<NativeProps>("SdaSystemVolume") : null;
function argb(color: string, fallback: number) {
 const value = processColor(color);
 return typeof value === "number" ? value >>> 0 : fallback;
}
// The system owns the value. Do not send the saved app/preview volume down to
// the native slider: that would unexpectedly change hardware volume on mount.
export function SystemVolumeSlider({ activeColor, inactiveColor, onChange, onTracking }: {
 activeColor: string; inactiveColor: string; onChange(value: number): void; onTracking?(active: boolean): void;
}) {
 if (!NativeSlider) return null;
 return <NativeSlider activeColorArgb={argb(activeColor, 0xffffffff)} inactiveColorArgb={argb(inactiveColor, 0x3dffffff)}
  onChange={event => { const value = event.nativeEvent.value; if (Number.isFinite(value)) onChange(Math.max(0, Math.min(1, value))); }}
  onTracking={event => onTracking?.(event.nativeEvent.active === true)} style={{ flex: 1, height: 44 }} />;
}
