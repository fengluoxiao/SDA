import React from "react";
import { StyleSheet, View } from "react-native";
import { SafeAreaView, useSafeAreaInsets } from "react-native-safe-area-context";
import { SIDE_NAVIGATION_WIDTH } from "./SideNavigationRail";

// Right content ends at safeInset + railWidth + 8 from the screen edge.
// Center the rail in that remaining lane instead of pinning it to the edge.
export function rightSideDockInset(rightSafeInset: number) {
  return (Math.max(0, rightSafeInset) + 8) / 2;
}

export function SideNavigationDock({ side, placement, testID, children }: {
  side: "left" | "right"; placement: "top" | "bottom"; testID: string; children: React.ReactNode;
}) {
  const insets = useSafeAreaInsets();
  const left = side === "left";
  return <SafeAreaView testID={testID} pointerEvents="box-none"
    edges={{ left: "maximum", right: "off", ...(placement === "top" ? { top: "additive" as const } : { bottom: "additive" as const }) }}
    style={[StyleSheet.absoluteFill, {
      alignItems: left ? "flex-start" : "flex-end", justifyContent: placement === "top" ? "flex-start" : "flex-end",
      paddingLeft: 14, paddingRight: left ? 6 : rightSideDockInset(insets.right),
      paddingTop: placement === "top" ? 16 : 0, paddingBottom: placement === "bottom" ? 8 : 0,
    }]}>
    <View pointerEvents="box-none" style={{ width: SIDE_NAVIGATION_WIDTH, marginHorizontal: left ? 6 : 0, alignItems: "center", gap: 8 }}>
      {children}
    </View>
  </SafeAreaView>;
}
