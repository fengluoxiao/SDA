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

export function IOSSystemTabs({ selected, onChange, children, accessory, fallback, accent, theme, backgroundColor }: {
  selected: number; onChange(index: number): void; children: React.ReactNode;
  accessory: React.ReactNode; fallback: React.ReactNode; accent: string; theme: "light" | "dark"; backgroundColor: string;
}) {
  const pages = React.Children.toArray(children);
  if (hasUIKitIOSTabs) return <IOSUIKitTabs selected={selected} onChange={onChange} pages={pages} accessory={accessory} accent={accent} theme={theme} backgroundColor={backgroundColor} />;
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
        <VStack spacing={0} modifiers={[background(backgroundColor, { ignoresSafeAreaEdges: "all" })]}>
          <RNHostView><View style={{ flex: 1, paddingHorizontal: 18, backgroundColor }}>{pages[index]}{index !== 0 && accessory}</View></RNHostView>
        </VStack>
      </TabView.Tab>)}
    </TabView>
  </Host>;
}
