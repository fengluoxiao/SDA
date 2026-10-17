import React from "react";
import { StyleSheet } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { IOSHeaderBlur } from "./IOSHeaderBlur";

export function IOSLibraryHeaderBackdrop({ dark }: { dark: boolean }) {
  const insets = useSafeAreaInsets();
  return <IOSHeaderBlur dark={dark} style={[StyleSheet.absoluteFill,
    { top: -insets.top, left: -insets.left - 18, right: -insets.right - 18 }]} />;
}
