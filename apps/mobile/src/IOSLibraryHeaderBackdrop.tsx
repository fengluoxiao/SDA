import React from "react";
import { StyleSheet } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { IOSHeaderBlur } from "./IOSHeaderBlur";

export function IOSLibraryHeaderBackdrop({ dark, horizontalPadding = 18 }: { dark: boolean; horizontalPadding?: number }) {
  const insets = useSafeAreaInsets();
  return <IOSHeaderBlur dark={dark} style={[StyleSheet.absoluteFill,
    { top: -insets.top, left: -insets.left - horizontalPadding, right: -insets.right - horizontalPadding }]} />;
}
