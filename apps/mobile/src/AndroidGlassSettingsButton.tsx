import React from "react";
import { requireNativeView } from "expo";
import { Platform } from "react-native";

interface Props {
  settingsButton: boolean; backButton: boolean; dark: boolean; busy: boolean;
  onExpand(): void; style: { width: number; height: number };
}
const NativeButton = Platform.OS === "android" && (globalThis as any).expo?.modules?.SdaAndroidChrome
  ? (requireNativeView as unknown as <P>(name: string) => React.ComponentType<P>)<Props>("SdaAndroidChrome") : null;

export function AndroidGlassSettingsButton({ dark, onPress, back = false }: { dark: boolean; onPress(): void; back?: boolean }) {
  return NativeButton ? <NativeButton settingsButton backButton={back} dark={dark} busy={false} onExpand={onPress} style={{ width: 44, height: 44 }} /> : null;
}
