import React, { useState } from "react";
import type { NativeSyntheticEvent } from "react-native";
import type { TabSelectedEvent } from "react-native-screens";
import { Platform, UIManager, View } from "react-native";
import { SafeAreaProvider, SafeAreaView } from "react-native-safe-area-context";
import { IOS_TABS } from "./ios-ui-model";

// Check the native binary before importing codegen components. Expo Go and
// installed SDA versions may expose different native navigation surfaces.
export const hasUIKitIOSTabs = Platform.OS === "ios" &&
  ["RNSTabsHostIOS", "RNSTabsScreenIOS"].every(name => UIManager.hasViewManagerConfig(name));
export const hasUIKitTabAccessory = hasUIKitIOSTabs && Number(Platform.Version) >= 26 &&
  ["RNSTabsBottomAccessory", "RNSTabsBottomAccessoryContent"].every(name => UIManager.hasViewManagerConfig(name));
if (__DEV__ && Platform.OS === "ios") {
  console.info("[SDA UI] tab bar:", hasUIKitIOSTabs ? "UIKit / explicit transparent appearances" : "SwiftUI compatibility / UIKit managers unavailable");
}
const screens: typeof import("react-native-screens") | null = hasUIKitIOSTabs
  ? require("react-native-screens") : null;

export function IOSUIKitTabs({ selected, onChange, pages, accessory, nativeAccessory, accent, theme, backgroundColor, pageBackgroundColors }: {
  selected: number; onChange(index: number): void; pages: React.ReactNode[];
  accessory: React.ReactNode; nativeAccessory?: (environment: "regular" | "inline") => React.ReactNode; accent: string; theme: "light" | "dark"; backgroundColor: string; pageBackgroundColors?: string[];
}) {
  const [provenance, setProvenance] = useState(0);
  if (!screens) return null;
  const { Tabs } = screens;
  const symbols = ["play.circle.fill", "music.note.list", "cube.transparent"];
  // These properties configure UITabBarAppearance itself. Painting a background
  // behind the TabView cannot override its opaque native bar appearance.
  // Keep the system glass effect; remove only the explicit fill and divider.
  const appearance = {
    tabBarBackgroundColor: "transparent",
    tabBarShadowColor: "transparent",
    tabBarBlurEffect: "systemDefault" as const,
  };
  return <View style={{ flex: 1, marginHorizontal: -18, backgroundColor }}>
    <Tabs.Host
      navStateRequest={{ selectedScreenKey: String(selected), baseProvenance: provenance }}
      nativeContainerStyle={{ backgroundColor }} colorScheme={theme}
      ios={{ tabBarTintColor: accent, tabBarControllerMode: "tabBar", tabBarMinimizeBehavior: "never",
        // The factory is mounted twice by screens. Its content has no local
        // playback state/effects; both variants act on the same parent engine.
        bottomAccessory: hasUIKitTabAccessory && nativeAccessory ? nativeAccessory : undefined,
        bottomAccessoryHidden: selected === 0 || !nativeAccessory,
      }}
      onTabSelected={({ nativeEvent }: NativeSyntheticEvent<TabSelectedEvent>) => {
        setProvenance(nativeEvent.provenance);
        const index = Number(nativeEvent.selectedScreenKey);
        if (Number.isInteger(index) && index >= 0 && index < IOS_TABS.length && index !== selected) onChange(index);
      }}>
      {IOS_TABS.map((label, index) => <Tabs.Screen key={label} screenKey={String(index)} title={label}
        style={{ backgroundColor: pageBackgroundColors?.[index] ?? backgroundColor }} tabBarItemAccessibilityLabel={label}
        ios={{
          experimental_userInterfaceStyle: theme,
          icon: { type: "sfSymbol", name: symbols[index] },
          standardAppearance: appearance, scrollEdgeAppearance: appearance,
        }}>
        {/* The page owns its content inset, while its background remains full
            screen behind the floating bar. Pages stay mounted across selection. */}
        <SafeAreaProvider style={{ flex: 1, backgroundColor: pageBackgroundColors?.[index] ?? backgroundColor }}>
          <SafeAreaView edges={["bottom"]} style={{ flex: 1, paddingHorizontal: 18, backgroundColor: pageBackgroundColors?.[index] ?? backgroundColor }}>
            {pages[index]}{index !== 0 && !(hasUIKitTabAccessory && nativeAccessory) && accessory}
          </SafeAreaView>
        </SafeAreaProvider>
      </Tabs.Screen>)}
    </Tabs.Host>
  </View>;
}
