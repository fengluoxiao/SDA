import React from "react";
import { Modal, Platform, Pressable, Text, View } from "react-native";
import { swiftUI } from "./IOSSystemTabs";

export function IOSMetadataDoneButton({ onPress, theme, accent }: { onPress(): void; theme: "light" | "dark"; accent: string }) {
  if (swiftUI && !!(globalThis as any).expo?.getViewConfig?.("ExpoUI", "Button")) {
    const { Host, Button, Text: NativeText } = swiftUI;
    const { buttonStyle, buttonBorderShape, accessibilityLabel, frame, opacity } = require("@expo/ui/swift-ui/modifiers") as typeof import("@expo/ui/swift-ui/modifiers");
    return <View style={{ width: 76, height: 44 }}>
      <Host style={{ width: 76, height: 44 }} colorScheme={theme}>
        <Button onPress={onPress} modifiers={[buttonStyle(Platform.OS === "ios" && Number(Platform.Version) >= 26 ? "glass" : "bordered"), buttonBorderShape("capsule"), accessibilityLabel("关闭歌曲元数据")]}><NativeText modifiers={[frame({ width: 44, height: 28 }), opacity(0)]}>完成</NativeText></Button>
      </Host>
      <View pointerEvents="none" accessible={false} accessibilityElementsHidden importantForAccessibility="no-hide-descendants" style={{ position: "absolute", top: 0, left: 0, right: 0, bottom: 0, alignItems: "center", justifyContent: "center" }}>
        <Text style={{ color: accent, fontSize: 17, fontWeight: "600" }}>完成</Text>
      </View>
    </View>;
  }
  return <Pressable accessibilityRole="button" accessibilityLabel="关闭歌曲元数据" onPress={onPress} style={{ minWidth: 76, minHeight: 44, alignItems: "center", justifyContent: "center" }}><Text style={{ color: accent, fontWeight: "600" }}>完成</Text></Pressable>;
}

export function IOSMetadataSheet({ open, onChange, children, theme }: {
  open: boolean; onChange(value: boolean): void; children: React.ReactNode; theme: "light" | "dark";
}) {
  const native = !!swiftUI && ["BottomSheetView", "GroupView", "RNHostView"].every(name =>
    !!(globalThis as any).expo?.getViewConfig?.("ExpoUI", name));
  if (native && swiftUI) {
    const { Host, BottomSheet, Group, RNHostView } = swiftUI;
    const { presentationDetents, presentationDragIndicator } = require("@expo/ui/swift-ui/modifiers") as typeof import("@expo/ui/swift-ui/modifiers");
    return <Host style={{ position: "absolute", width: 1, height: 1 }} colorScheme={theme}>
      <BottomSheet isPresented={open} onIsPresentedChange={onChange}>
        <Group modifiers={[presentationDetents(["medium", "large"]), presentationDragIndicator("visible")]}>
          <RNHostView><View style={{ flex: 1 }}>{children}</View></RNHostView>
        </Group>
      </BottomSheet>
    </Host>;
  }
  return <Modal visible={open} animationType="slide" presentationStyle="pageSheet" onRequestClose={() => onChange(false)}>{children}</Modal>;
}
