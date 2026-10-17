import { resolveNativeTabSelection } from "./native-tab-selection";
import React, { useRef, useState } from "react";
import { AppState, Platform, UIManager, View, type NativeSyntheticEvent } from "react-native";
import type { TabSelectedEvent } from "react-native-screens";
import { IOS_TABS } from "./ios-ui-model";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { nativeSideTabFrame, NATIVE_SIDE_TAB_LENGTH } from "./native-side-tab-layout";
import { iosTabAppearance } from "./ios-tab-appearance";

function supportsNativeTabBar() {
  if (Platform.OS !== "ios" || Number(Platform.Version) < 26) return false;
  try {
    return ["RNSTabsHostIOS", "RNSTabsScreenIOS"].every(name => UIManager.hasViewManagerConfig(name));
  } catch { return false; }
}
export const hasNativeVerticalTabBar = supportsNativeTabBar();
const screens: typeof import("react-native-screens") | null = hasNativeVerticalTabBar ? require("react-native-screens") : null;
// Template alpha images are already rotated counter-clockwise. UIKit owns the
// real items, including their placement, selected tint and drag-hover tint.
const nativeIcons = [
  require("../assets/side-tabs/play-ccw.png"),
  require("../assets/side-tabs/library-ccw.png"),
  require("../assets/side-tabs/space-ccw.png"),
] as const;

// Reuse the same UITabBarController and appearances as portrait. Empty pages
// here only host the bar; the existing player/pages remain owned by IOSPlayer.
export function IOSVerticalTabBar({ selected, onChange, accent, dark }: {
  selected: number; onChange(index: number): void; accent: string; dark: boolean; backgroundColor?: string;
}) {
  const [provenance, setProvenance] = useState(0);
  const lastProvenance = useRef(0);
  const screenInsets = useSafeAreaInsets();
  // Child pages report content-safe insets after UIKit has already laid out
  // the bar. Feeding those values back into the controller shrinks it again.
  const nativeFrame = nativeSideTabFrame(screenInsets);
  if (!screens) return null;
  const { Tabs } = screens;
  return <View testID="native-vertical-tab-bar" collapsable={false}
    style={{ width: 48, height: NATIVE_SIDE_TAB_LENGTH, overflow: "visible", backgroundColor: "transparent" }}>
    <View testID="vertical-tab-touch-layer" style={{ position: "absolute", width: NATIVE_SIDE_TAB_LENGTH, height: 48, left: (48 - NATIVE_SIDE_TAB_LENGTH) / 2, top: (NATIVE_SIDE_TAB_LENGTH - 48) / 2,
      backgroundColor: "transparent", transform: [{ rotate: "90deg" }] }}>
      <View testID="vertical-tab-inset-compensation" style={{ position: "absolute", ...nativeFrame, backgroundColor: "transparent" }}>
      <Tabs.Host style={{ width: nativeFrame.width, height: nativeFrame.height, flex: 0 }} navStateRequest={{ selectedScreenKey: String(selected), baseProvenance: provenance }}
        nativeContainerStyle={{ backgroundColor: "transparent" }} colorScheme={dark ? "dark" : "light"}
        ios={{ tabBarTintColor: accent, tabBarControllerMode: "tabBar", tabBarMinimizeBehavior: "never", bottomAccessoryHidden: true }}
        onTabSelected={({ nativeEvent }: NativeSyntheticEvent<TabSelectedEvent>) => {
          const update = resolveNativeTabSelection(nativeEvent, lastProvenance.current, selected, AppState.currentState === "active", "side");
          if (!update) return;
          lastProvenance.current = update.provenance;
          setProvenance(update.provenance);
          if (update.index !== null) onChange(update.index);
        }}>
        {/* Omit title entirely: even a thin space reserves inline label space,
            which becomes a vertical icon offset after rotating the bar. */}
        {IOS_TABS.map((label, index) => <Tabs.Screen key={label} screenKey={String(index)}
          tabBarItemAccessibilityLabel={label} style={{ backgroundColor: "transparent" }} ios={{
            experimental_userInterfaceStyle: dark ? "dark" : "light",
            icon: { type: "templateSource", templateSource: nativeIcons[index]! },
            standardAppearance: iosTabAppearance, scrollEdgeAppearance: iosTabAppearance,
          }}><View pointerEvents="none" style={{ flex: 1, backgroundColor: "transparent" }} /></Tabs.Screen>)}
      </Tabs.Host>
      </View>
    </View>
  </View>;
}
