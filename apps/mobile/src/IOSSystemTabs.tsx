import { SafeAreaView } from "react-native-safe-area-context";
import React from "react";
import { Platform, View } from "react-native";
import { hasUIKitIOSTabs, IOSUIKitTabs } from "./IOSUIKitTabs";
import { IOS_TABS } from "./ios-ui-model";

// Expo Go SDK 57 includes ExpoUI. Older SDA binaries keep their existing tabs
// until rebuilt. Android never loads SwiftUI native view managers.
export const swiftUI: typeof import("@expo/ui/swift-ui") | null =
  Platform.OS === "ios" && (globalThis as any).expo?.modules?.ExpoUI
    ? require("@expo/ui/swift-ui") : null;
export const hasSystemIOSTabs = hasUIKitIOSTabs || swiftUI !== null;

export function IOSSystemTabs({ selected, onChange, children, accessory, nativeAccessory, fallback, accent, theme, backgroundColor, pageBackgroundColors, pageHeaders, libraryHeaderActions, sideBar, sideBarExternal = false, sideBarPosition = "right", sideBarWidth = 48, rightContentGap = 8, leftContentOffset = 0 }: {
  pageHeaders?: React.ReactNode[];
  libraryHeaderActions?: React.ReactNode;
  sideBar?: React.ReactNode; sideBarExternal?: boolean; sideBarPosition?: "left" | "right"; sideBarWidth?: number; rightContentGap?: number; leftContentOffset?: number;
  selected: number; onChange(index: number): void; children: React.ReactNode;
  accessory: React.ReactNode; nativeAccessory?: (environment: "regular" | "inline") => React.ReactNode; fallback: React.ReactNode; accent: string; theme: "light" | "dark"; backgroundColor: string; pageBackgroundColors?: string[];
}) {
  const pages = React.Children.toArray(children);
  const sidePageContent = (page: React.ReactNode, index: number) => {
    if (index === 1 && accessory) {
      const scroll = React.isValidElement<{ contentContainerStyle?: object }>(page)
        ? React.cloneElement(page, { contentContainerStyle: [page.props.contentContainerStyle, { paddingBottom: 76 }] }) : page;
      return <View style={{ flex: 1, backgroundColor: "transparent" }}>{scroll}
        <View style={{ position: "absolute", bottom: 0, left: 0, right: sideBarPosition === "left" ? 12 : 0, [sideBarPosition === "left" ? "marginLeft" : "marginRight"]: Math.max(0, sideBarWidth + (sideBarPosition === "right" ? rightContentGap : 8 + leftContentOffset) - 18) }}>{accessory}</View>
      </View>;
    }
    return <>{page}{index !== 0 && <View style={{ [sideBarPosition === "left" ? "marginLeft" : "marginRight"]: Math.max(0, sideBarWidth + (sideBarPosition === "right" ? rightContentGap : 8 + leftContentOffset) - 18) }}>{accessory}</View>}</>;
  };
  if (hasUIKitIOSTabs) return <IOSUIKitTabs selected={selected} onChange={onChange} pages={pages} pageHeaders={pageHeaders} accessory={accessory} nativeAccessory={nativeAccessory} accent={accent} theme={theme} backgroundColor={backgroundColor} pageBackgroundColors={pageBackgroundColors} libraryHeaderActions={libraryHeaderActions} sideBar={sideBar} sideBarExternal={sideBarExternal} sideBarPosition={sideBarPosition} sideBarWidth={sideBarWidth} rightContentGap={rightContentGap} leftContentOffset={leftContentOffset} />;
  // Compatibility hosts without UIKit's tab-bar visibility API still use the
  // same page content and player callbacks; the current native path stays mounted.
  if (sideBar) return <View style={{ flex: 1, marginHorizontal: -18 }}>
    <View style={{ flex: 1, paddingHorizontal: 18, ...(sideBarPosition === "left" ? { paddingLeft: 30, paddingRight: 6 } : {}) }}>{pages.map((page, index) => <View key={index} style={{ flex: 1, display: selected === index ? "flex" : "none" }}>{sidePageContent(page, index)}</View>)}</View>
    {!sideBarExternal && <SafeAreaView edges={hasSystemIOSTabs ? ["bottom"] : []} pointerEvents="box-none" style={{ position: "absolute", left: 0, right: 0, top: 0, bottom: 0, alignItems: sideBarPosition === "left" ? "flex-start" : "flex-end", justifyContent: "flex-end", paddingRight: 0, paddingBottom: 8 }}><View pointerEvents="box-none" style={{ width: sideBarWidth, alignItems: "center" }}>{sideBar}</View></SafeAreaView>}
  </View>;
  if (!swiftUI) return <><View style={{ flex: 1 }}>{pages}</View>{selected !== 0 && accessory}{fallback}</>;
  const { Host, TabView, RNHostView, VStack } = swiftUI;
  const { tabViewStyle, background } = require("@expo/ui/swift-ui/modifiers") as typeof import("@expo/ui/swift-ui/modifiers");
  const symbols = ["play.circle.fill", "music.note.list", "cube.transparent"] as const;
  return <Host style={{ flex: 1, marginHorizontal: -18, backgroundColor }} colorScheme={theme} seedColor={accent}>
    <TabView modifiers={[tabViewStyle({ type: "automatic" }), background(backgroundColor, { ignoresSafeAreaEdges: "all" })]} selection={String(selected)} onSelectionChange={value => {
      const index = Number(value);
      if (Number.isInteger(index) && index >= 0 && index < IOS_TABS.length) onChange(index);
    }}>
      {IOS_TABS.map((label, index) => <TabView.Tab key={label} value={String(index)} label={label} systemImage={symbols[index]}>
        {/* Paint inside each native tab, not just underneath TabView itself.
            The page background must continue behind the floating system bar. */}
        <VStack spacing={0}>
          <RNHostView><View style={{ flex: 1, paddingHorizontal: 18, backgroundColor: "transparent" }}>{pages[index]}{index !== 0 && accessory}</View></RNHostView>
        </VStack>
      </TabView.Tab>)}
    </TabView>
  </Host>;
}
