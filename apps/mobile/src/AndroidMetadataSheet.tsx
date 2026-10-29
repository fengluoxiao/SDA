import React, { useRef } from "react";
import { Modal, Platform, View, useWindowDimensions } from "react-native";
import { useAppTheme } from "./app-theme";
import { androidMaterialTheme } from "./android-material-theme";

const compose: typeof import("@expo/ui/jetpack-compose") | null = Platform.OS === "android" && (globalThis as any).expo?.modules?.ExpoUI
  ? require("@expo/ui/jetpack-compose") : null;

const composeModifiers: typeof import("@expo/ui/jetpack-compose/modifiers") | null = compose
  ? require("@expo/ui/jetpack-compose/modifiers") : null;

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
      <compose.ModalBottomSheet.DragHandle>
        <compose.Box contentAlignment="center" modifiers={[composeModifiers!.fillMaxWidth(), composeModifiers!.height(28)]}>
          <compose.Box modifiers={[composeModifiers!.size(32, 4), composeModifiers!.clip({ type: "roundedCorner", radius: 2 }), composeModifiers!.background(c.muted)]} />
        </compose.Box>
      </compose.ModalBottomSheet.DragHandle>
      {/* Let Compose measure the sheet width, then pass those bounds to Yoga.
          Content-sized hosting leaves an intrinsic-width column and an empty right strip. */}
      <compose.Box modifiers={[composeModifiers!.fillMaxWidth(), composeModifiers!.height(Math.max(180, height - 100))]}>
        <compose.RNHostView><View style={{ flex: 1 }}>{content}</View></compose.RNHostView>
      </compose.Box>
    </compose.ModalBottomSheet>
  </compose.Host> : null;
}
