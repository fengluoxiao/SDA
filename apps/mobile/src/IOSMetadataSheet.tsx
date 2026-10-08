import React from "react";
import { Modal, View } from "react-native";
import { swiftUI } from "./IOSSystemTabs";

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
