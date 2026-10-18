import React from "react";
import { requireNativeView } from "expo";
import { Platform, useColorScheme } from "react-native";

interface Props {
  settingsButton: boolean; backButton: boolean; dark: boolean; glassDark: boolean; busy: boolean;
  onExpand(): void; style: { width: number; height: number };
}
const NativeButton = Platform.OS === "android" && (globalThis as any).expo?.modules?.SdaAndroidChrome
  ? (requireNativeView as unknown as <P>(name: string) => React.ComponentType<P>)<Props>("SdaAndroidChrome") : null;

export function AndroidGlassSettingsButton({ onPress, back = false }: { onPress(): void; back?: boolean }) {
  const dark = useColorScheme() === "dark";
  return NativeButton ? <NativeButton settingsButton backButton={back} dark={dark} glassDark={dark} busy={false} onExpand={onPress} style={{ width: 44, height: 44 }} /> : null;
}
