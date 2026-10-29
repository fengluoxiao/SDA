import React, { useCallback, useRef, useState } from "react";
import { Modal, Platform, View, useWindowDimensions } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { androidMetadataBodyHeight } from "./android-metadata-layout";
import { useAppTheme } from "./app-theme";
import { androidMaterialTheme } from "./android-material-theme";

const compose: typeof import("@expo/ui/jetpack-compose") | null = Platform.OS === "android" && (globalThis as any).expo?.modules?.ExpoUI
  ? require("@expo/ui/jetpack-compose") : null;

const composeModifiers: typeof import("@expo/ui/jetpack-compose/modifiers") | null = compose
  ? require("@expo/ui/jetpack-compose/modifiers") : null;

function MeasuredMetadataBody({ height, children }: { height: number; children: React.ReactNode }) {
  const [bounds, setBounds] = useState<{ width: number; height: number } | null>(null);
  const measured = useCallback((size: { width: number; height: number }) => {
    if (!(Number.isFinite(size.width) && Number.isFinite(size.height) && size.width > 0 && size.height > 0)) return;
    setBounds(previous => previous?.width === size.width && previous.height === size.height ? previous : size);
  }, []);
  if (!compose || !composeModifiers) return null;
  return <compose.Box modifiers={[composeModifiers.fillMaxWidth(), composeModifiers.height(height), composeModifiers.onSizeChanged(measured)]}>
    {bounds && <compose.RNHostView matchContents>
      <View style={{ width: bounds.width, height: bounds.height }}>{children}</View>
    </compose.RNHostView>}
  </compose.Box>;
}

export function AndroidMetadataSheet({ open, onChange, children, theme }: {
  open: boolean; onChange(value: boolean): void; children: React.ReactNode; theme: "light" | "dark";
}) {
  const { width, height } = useWindowDimensions();
  const insets = useSafeAreaInsets();
  const bodyHeight = androidMetadataBodyHeight(height, insets.top, insets.bottom);
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
      {/* Mount each presentation only after its real native viewport is known.
          Explicit bounds avoid a first-frame Yoga/Compose size race. */}
      <MeasuredMetadataBody key={`${width}:${height}:${insets.top}:${insets.bottom}`} height={bodyHeight}>{content}</MeasuredMetadataBody>
    </compose.ModalBottomSheet>
  </compose.Host> : null;
}
