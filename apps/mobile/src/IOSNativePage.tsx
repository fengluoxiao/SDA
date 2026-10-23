import React from "react";
import { Platform, PlatformColor, StyleSheet, UIManager, View, type ColorValue } from "react-native";

function supportsNativePage() {
  if (Platform.OS !== "ios") return false;
  try {
    return ["RNSScreen", "RNSScreenStack", "RNSScreenStackHeaderConfig", "RNSScreenStackHeaderSubview"]
      .every(name => UIManager.hasViewManagerConfig(name));
  } catch { return false; }
}
export const hasNativePageHeader = supportsNativePage();
if (__DEV__ && Platform.OS === "ios") console.info("[SDA UI] native page headers:", hasNativePageHeader ? "UIKit available" : "unavailable");
const screens: typeof import("react-native-screens") | null = hasNativePageHeader ? require("react-native-screens") : null;

// Each page owns its real UINavigationController/header from its first layout.
// The outer stack moves the whole page without a separate header transition.
export function IOSNativePage({ screenId, title, theme, accent, headerLeft, headerRight, headerRightWidth = 96, backgroundColor = PlatformColor("systemGroupedBackground"), onHeaderHeightChange, children }: {
  screenId: string; title: string; theme: "light" | "dark"; accent: string;
  headerLeft?: React.ReactNode; headerRight?: React.ReactNode; headerRightWidth?: number; backgroundColor?: ColorValue; onHeaderHeightChange?(height: number): void; children: React.ReactNode;
}) {
  if (!screens) return <>{children}</>;
  const { ScreenStack, ScreenStackItem, ScreenStackHeaderLeftView, ScreenStackHeaderRightView } = screens;
  return <ScreenStack style={{ flex: 1 }}>
    <ScreenStackItem screenId={screenId} activityState={2} freezeOnBlur={false}
      onHeaderHeightChange={({ nativeEvent }) => {
        if (__DEV__) console.info("[SDA UI] native header height", screenId, nativeEvent.headerHeight);
        onHeaderHeightChange?.(nativeEvent.headerHeight);
      }}
      headerConfig={{ title, largeTitle: false, translucent: true, hideShadow: true, hideBackButton: true,
        color: accent, backgroundColor: "transparent", experimental_userInterfaceStyle: theme,
        children: <>
          {!!headerLeft && <ScreenStackHeaderLeftView hidesSharedBackground style={{ width: 44, height: 44 }}>{headerLeft}</ScreenStackHeaderLeftView>}
          {!!headerRight && <ScreenStackHeaderRightView hidesSharedBackground style={{ width: headerRightWidth, height: 44 }}>
            <View style={{ flexDirection: "row", alignItems: "center", width: headerRightWidth, height: 44 }}>{headerRight}</View>
          </ScreenStackHeaderRightView>}
        </>,
      }}
      contentStyle={{ flex: 1, backgroundColor }} style={StyleSheet.absoluteFill}>
      {children}
    </ScreenStackItem>
  </ScreenStack>;
}
