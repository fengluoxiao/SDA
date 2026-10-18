import React from "react";
import { Image, Platform, StyleSheet, View } from "react-native";
import { phoneLandscapeBlurEdge } from "./phone-landscape-blur-model";

type Props = {
  uri: string; surfaceWidth: number; surfaceHeight: number;
  artworkLeft: number; artworkTop: number; artworkWidth: number; artworkHeight: number;
  edge?: boolean;
};

function nativeMask() {
  if (Platform.OS !== "ios" && Platform.OS !== "android") return null;
  try {
    const expo = (globalThis as any).expo;
    if (!expo?.modules?.ExpoUI) return null;
    if (expo.getViewConfig && !expo.getViewConfig("ExpoUI", "MaskView")) return null;
    return (require("@expo/ui/community/masked-view") as typeof import("@expo/ui/community/masked-view")).MaskedView;
  } catch { return null; }
}

// The edge samples the same screen-aligned backdrop as the background. A small
// alpha mask blends into the sharp cover without moving or resizing that cover.
export function PhoneLandscapeBlur(p: Props) {
  const frame = phoneLandscapeBlurEdge(p.artworkLeft, p.artworkTop, p.artworkWidth, p.artworkHeight);
  const background = <View style={{ position: "absolute", left: p.edge ? -frame.left : 0, top: p.edge ? -frame.top : 0, width: p.surfaceWidth, height: p.surfaceHeight }}>
    <Image source={{ uri: p.uri }} resizeMode="cover" blurRadius={48} style={[StyleSheet.absoluteFill, { opacity: .45 }]} />
    <View style={[StyleSheet.absoluteFill, { backgroundColor: "rgba(0,0,0,.24)" }]} />
  </View>;
  if (!p.edge) return <View testID="phone-landscape-blur-background" pointerEvents="none" style={StyleSheet.absoluteFill}>{background}</View>;
  const MaskedView = nativeMask();
  if (!MaskedView || frame.width <= 0 || frame.height <= 0) return null;
  return <MaskedView testID="phone-landscape-blur-edge" pointerEvents="none" style={{ position: "absolute", ...frame }}
    maskElement={<Image source={require("../assets/artwork-color-fade-horizontal.png")} resizeMode="stretch" style={{ width: frame.width, height: frame.height }} />}>
    {background}
  </MaskedView>;
}
