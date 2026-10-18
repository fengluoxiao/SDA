import { resolveNativeTabSelection } from "./native-tab-selection";
import { iosTabAppearance } from "./ios-tab-appearance";
import { IOSNativePage, hasNativePageHeader } from "./IOSNativePage";
import React, { useRef, useState } from "react";
import type { NativeSyntheticEvent, ScrollViewProps } from "react-native";
import type { TabSelectedEvent } from "react-native-screens";
import { AppState, Platform, UIManager, View } from "react-native";
import { SafeAreaProvider, SafeAreaView, useSafeAreaInsets } from "react-native-safe-area-context";
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

export function IOSUIKitTabs({ selected, onChange, pages, accessory, nativeAccessory, accent, theme, backgroundColor, pageBackgroundColors, pageHeaders, libraryHeaderActions, sideBar, sideBarExternal = false, sideBarPosition = "right", sideBarWidth = 48, rightContentGap = 8, leftContentOffset = 0, sideBarInsetReserved = false }: {
  pageHeaders?: React.ReactNode[];
  libraryHeaderActions?: React.ReactNode;
  sideBarInsetReserved?: boolean; sideBar?: React.ReactNode; sideBarExternal?: boolean; sideBarPosition?: "left" | "right"; sideBarWidth?: number; rightContentGap?: number; leftContentOffset?: number;
  selected: number; onChange(index: number): void; pages: React.ReactNode[];
  accessory: React.ReactNode; nativeAccessory?: (environment: "regular" | "inline") => React.ReactNode; accent: string; theme: "light" | "dark"; backgroundColor: string; pageBackgroundColors?: string[];
}) {
  const [provenance, setProvenance] = useState(0);
  const lastProvenance = useRef(0);
  const [libraryHeaderHeight, setLibraryHeaderHeight] = useState(0);
  const screenInsets = useSafeAreaInsets();
  const libraryBottomInset = sideBar ? Math.max(0, screenInsets.bottom) : 0;
  if (!screens) return null;
  const { Tabs } = screens;
  const symbols = ["play.circle.fill", "music.note.list", "cube.transparent"];
  // ScrollView's automatic content inset keeps rows reachable while its viewport
  // extends behind the native floating bar/accessory. Reserving the same inset
  // on this wrapper leaves a solid page-colored strip behind the glass instead.
  const libraryUnderlaysNativeBar = !sideBar && (!accessory || (hasUIKitTabAccessory && !!nativeAccessory));
  const floatingLibraryAccessory = !!sideBar && !!accessory;
  const pageContent = (index: number) => {
    const page = pages[index];
    if (index !== 1) return page;
    const scroll = React.isValidElement<ScrollViewProps>(page)
      ? React.cloneElement(page, {
          contentContainerStyle: [page.props.contentContainerStyle, { paddingBottom: floatingLibraryAccessory ? 76 + libraryBottomInset : 24 + libraryBottomInset }],
          ...(sideBar && !sideBarInsetReserved && sideBarPosition === "left" && hasNativePageHeader ? {
            // Its viewport extends through the right safe inset for the native
            // edge effect. Apply indicator safety once, using the measured bar.
            automaticallyAdjustsScrollIndicatorInsets: false,
            scrollIndicatorInsets: { ...page.props.scrollIndicatorInsets, top: libraryHeaderHeight + 10 },
          } : {}),
        }) : page;
    return floatingLibraryAccessory ? <View style={{ flex: 1, backgroundColor: "transparent" }}>
      {scroll}
      <View testID="floating-library-accessory" style={{ position: "absolute", bottom: libraryBottomInset, left: 0, right: !sideBarInsetReserved && sideBarPosition === "left" ? 12 : 0, [sideBarPosition === "left" ? "marginLeft" : "marginRight"]: sideBarInsetReserved ? 0 : Math.max(0, sideBarWidth + (sideBarPosition === "right" ? rightContentGap : 8 + leftContentOffset) - 18) }}>{accessory}</View>
    </View> : scroll;
  };
  const renderPage = (index: number) => {
    const nativeHeader = index === 1 && hasNativePageHeader;
    const viewport = <SafeAreaProvider style={{ flex: 1, backgroundColor: "transparent" }}>
        <SafeAreaView edges={nativeHeader ? [] : ["top", "left", "right"]} style={{ flex: 1 }}>
          {!nativeHeader && <View style={{ paddingHorizontal: 18 }}>{pageHeaders?.[index]}</View>}
          <SafeAreaView edges={index === 1 ? [...(hasNativePageHeader ? ["left", "right"] as const : []), ...(libraryUnderlaysNativeBar || sideBar ? [] : ["bottom"] as const)] : ["bottom"]} style={{ flex: 1, paddingHorizontal: 18, ...(sideBar && !sideBarInsetReserved && sideBarPosition === "left" ? { paddingLeft: 30, paddingRight: 6 } : {}), backgroundColor: "transparent" }}>
            {pageContent(index)}{index !== 0 && !(index === 1 && floatingLibraryAccessory) && (sideBar || !(hasUIKitTabAccessory && nativeAccessory)) && (sideBar ? <View style={{ [sideBarPosition === "left" ? "marginLeft" : "marginRight"]: sideBarInsetReserved ? 0 : Math.max(0, sideBarWidth + (sideBarPosition === "right" ? rightContentGap : 8 + leftContentOffset) - 18) }}>{accessory}</View> : accessory)}
          </SafeAreaView>
        </SafeAreaView>
        </SafeAreaProvider>;
    return index === 1 && hasNativePageHeader ? <IOSNativePage screenId="sda-library-content" title="资料库" theme={theme} accent={accent}
      onHeaderHeightChange={height => setLibraryHeaderHeight(previous => Math.abs(previous - height) < 0.5 ? previous : height)}
      headerRight={!sideBar ? libraryHeaderActions : undefined} backgroundColor={pageBackgroundColors?.[index] ?? backgroundColor}>{viewport}</IOSNativePage> : viewport;
  };
  // These properties configure UITabBarAppearance itself. Painting a background
  // behind the TabView cannot override its opaque native bar appearance.
  // Keep the system glass effect; remove only the explicit fill and divider.
  const appearance = iosTabAppearance;
  return <View style={{ flex: 1, marginHorizontal: -18, backgroundColor }}>
    <Tabs.Host
      tabBarHidden={!!sideBar}
      navStateRequest={{ selectedScreenKey: String(selected), baseProvenance: provenance }}
      nativeContainerStyle={{ backgroundColor }} colorScheme={theme}
      ios={{ tabBarTintColor: accent, tabBarControllerMode: "tabBar", tabBarMinimizeBehavior: "never",
        // The factory is mounted twice by screens. Its content has no local
        // playback state/effects; both variants act on the same parent engine.
        bottomAccessory: !sideBar && hasUIKitTabAccessory && nativeAccessory ? nativeAccessory : undefined,
        bottomAccessoryHidden: !!sideBar || selected === 0 || !nativeAccessory,
      }}
      onTabSelected={({ nativeEvent }: NativeSyntheticEvent<TabSelectedEvent>) => {
        const update = resolveNativeTabSelection(nativeEvent, lastProvenance.current, selected, AppState.currentState === "active", "main");
        if (!update) return;
        lastProvenance.current = update.provenance;
        setProvenance(update.provenance);
        if (update.index !== null) onChange(update.index);
      }}>
      {IOS_TABS.map((label, index) => <Tabs.Screen key={label} screenKey={String(index)} title={label}
        style={{ backgroundColor: pageBackgroundColors?.[index] ?? backgroundColor }} tabBarItemAccessibilityLabel={label}
        ios={{
          experimental_userInterfaceStyle: theme,
          icon: { type: "sfSymbol", name: symbols[index] },
          standardAppearance: appearance, scrollEdgeAppearance: appearance,
        }}>
        {renderPage(index)}
      </Tabs.Screen>)}
    </Tabs.Host>
    {sideBar && !sideBarExternal && <SafeAreaView edges={["bottom"]} pointerEvents="box-none" style={{ position: "absolute", left: 0, right: 0, top: 0, bottom: 0, alignItems: sideBarPosition === "left" ? "flex-start" : "flex-end", justifyContent: "flex-end", paddingRight: 0, paddingBottom: 8 }}><View pointerEvents="box-none" style={{ width: sideBarWidth, alignItems: "center" }}>{sideBar}</View></SafeAreaView>}
  </View>;
}
