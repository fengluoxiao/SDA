import React from "react";
import { Image, Platform, StyleSheet } from "react-native";

function artworkMask() {
  if (Platform.OS !== "ios" && Platform.OS !== "android") return null;
  try {
    const expo = (globalThis as any).expo;
    if (!expo?.modules?.ExpoUI || (expo.getViewConfig && !expo.getViewConfig("ExpoUI", "MaskView"))) return null;
    return (require("@expo/ui/community/masked-view") as typeof import("@expo/ui/community/masked-view")).MaskedView;
  } catch { return null; }
}

// Fade only the artwork edge, revealing the actual screen-aligned blurred
// backdrop underneath. The cover's dimensions, aspect ratio and center stay intact.
export function ArtworkEdgeBlend({ children }: { children: React.ReactElement }) {
  const MaskedView = artworkMask();
  if (!MaskedView) return children;
  return <MaskedView testID="tablet-artwork-edge-blend" style={{ width: "100%", height: "100%" }}
    maskElement={<Image source={require("../assets/artwork-edge-blend-mask.png")} resizeMode="stretch" style={StyleSheet.absoluteFill} />}>
    {children}
  </MaskedView>;
}
