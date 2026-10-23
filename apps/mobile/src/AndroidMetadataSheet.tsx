import React, { useRef } from "react";
import { Modal, Platform, View, useWindowDimensions } from "react-native";
import { useAppTheme } from "./app-theme";
import { androidMaterialTheme } from "./android-material-theme";

const compose: typeof import("@expo/ui/jetpack-compose") | null = Platform.OS === "android" && (globalThis as any).expo?.modules?.ExpoUI
  ? require("@expo/ui/jetpack-compose") : null;

export function AndroidMetadataSheet({ open, onChange, children, theme }: {
  open: boolean; onChange(value: boolean): void; children: React.ReactNode; theme: "light" | "dark";
}) {
  const { height } = useWindowDimensions();
  const appTheme = useAppTheme();
  const c = androidMaterialTheme(appTheme.id, theme === "dark");
  const sheet = useRef<import("@expo/ui/jetpack-compose").ModalBottomSheetRef>(null);
  const closing = useRef(false);
  if (!compose) return <Modal visible={open} animationType="slide" onRequestClose={() => onChange(false)}>{children}</Modal>;
  const close = async () => {
    if (closing.current) return;
    closing.current = true;
    try { await sheet.current?.hide(); } catch { /* The native sheet may already be dismissed. */ } finally { closing.current = false; onChange(false); }
  };
  const content = React.isValidElement<{ onClose(): void; sheet?: boolean }>(children)
    ? React.cloneElement(children, { onClose: close, sheet: true }) : children;
  return open ? <compose.Host style={{ position: "absolute", width: 1, height: 1 }} colorScheme={theme} seedColor={appTheme.accent}>
    <compose.ModalBottomSheet ref={sheet} onDismissRequest={() => onChange(false)} containerColor={c.bg}
      skipPartiallyExpanded={false} showDragHandle>
      <compose.RNHostView matchContents><View style={{ height: Math.max(180, height - 100) }}>{content}</View></compose.RNHostView>
    </compose.ModalBottomSheet>
  </compose.Host> : null;
}
