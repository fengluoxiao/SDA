import React from "react";
import { Platform, StyleSheet, View } from "react-native";
import { SafeAreaView, useSafeAreaInsets } from "react-native-safe-area-context";
import { SIDE_NAVIGATION_WIDTH } from "./SideNavigationRail";

import { rightSideDockInset, leftSideDockInset, type IOSCutoutSide } from "./ios-ui-model";
export { rightSideDockInset } from "./ios-ui-model";

export function SideNavigationDock({ side, placement, testID, children, cutoutSide = null }: {
  side: "left" | "right"; placement: "top" | "bottom"; testID: string; children: React.ReactNode; cutoutSide?: IOSCutoutSide;
}) {
  const insets = useSafeAreaInsets();
  const left = side === "left";
  const android = Platform.OS === "android";
  return <SafeAreaView testID={testID} pointerEvents="box-none"
    edges={{ left: "off", right: "off", ...(placement === "top" ? { top: "additive" as const } : { bottom: "additive" as const }) }}
    style={[StyleSheet.absoluteFill, {
      alignItems: left ? "flex-start" : "flex-end", justifyContent: placement === "top" ? "flex-start" : "flex-end",
      paddingLeft: left ? (android ? insets.left + 8 : leftSideDockInset(insets.left, cutoutSide)) : 14, paddingRight: left ? 6 : (android ? insets.right + 8 : rightSideDockInset(insets.right, cutoutSide)),
      paddingTop: placement === "top" ? 16 : 0, paddingBottom: placement === "bottom" ? 8 : 0,
    }]}>
    <View pointerEvents="box-none" style={{ width: SIDE_NAVIGATION_WIDTH, marginHorizontal: 0, alignItems: "center", gap: 8 }}>
      {children}
    </View>
  </SafeAreaView>;
}
