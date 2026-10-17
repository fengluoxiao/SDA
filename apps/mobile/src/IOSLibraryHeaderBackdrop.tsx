import React from "react";
import { StyleSheet } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { IOSHeaderBlur } from "./IOSHeaderBlur";

export function IOSLibraryHeaderBackdrop({ dark, horizontalPadding = 18, opacity }: { dark: boolean; horizontalPadding?: number; opacity?: React.ComponentProps<typeof IOSHeaderBlur>["opacity"] }) {
  const insets = useSafeAreaInsets();
  return <IOSHeaderBlur dark={dark} opacity={opacity} style={[StyleSheet.absoluteFill,
    { top: -insets.top, left: -insets.left - horizontalPadding, right: -insets.right - horizontalPadding }]} />;
}
