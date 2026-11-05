import React from "react";
import { Image, Platform, StyleSheet, View } from "react-native";
import { phoneLandscapeBlurEdge, artworkBackdropBottomEdge, artworkRightContinuation, ARTWORK_AMBIENT_PAINT } from "./phone-landscape-blur-model";

type Props = {
  uri: string; surfaceWidth: number; surfaceHeight: number;
  artworkLeft: number; artworkTop: number; artworkWidth: number; artworkHeight: number;
  edge?: boolean; edgeWidth?: number; edgeDirection?: "right" | "bottom"; edgeHeight?: number; testID?: string; parentLeft?: number; parentTop?: number; backgroundColor?: string;
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
  const frame = p.edgeDirection === "bottom"
    ? artworkBackdropBottomEdge(p.artworkLeft, p.artworkTop, p.artworkWidth, p.artworkHeight, p.edgeHeight)
    : phoneLandscapeBlurEdge(p.artworkLeft, p.artworkTop, p.artworkWidth, p.artworkHeight, p.edgeWidth);
  const background = <View style={{ position: "absolute", left: p.edge ? -frame.left : 0, top: p.edge ? -frame.top : 0, width: p.surfaceWidth, height: p.surfaceHeight, backgroundColor: p.backgroundColor ?? "#303034" }}>
    <Image source={{ uri: p.uri }} resizeMode="cover" blurRadius={48} style={[StyleSheet.absoluteFill, { opacity: ARTWORK_AMBIENT_PAINT.imageOpacity }]} />
    <View style={[StyleSheet.absoluteFill, { backgroundColor: ARTWORK_AMBIENT_PAINT.scrimColor }]} />
  </View>;
  if (!p.edge) return <View testID="phone-landscape-blur-background" pointerEvents="none" style={StyleSheet.absoluteFill}>{background}</View>;
  const MaskedView = nativeMask();
  if (!MaskedView || frame.width <= 0 || frame.height <= 0) return null;
  return <MaskedView testID={p.testID ?? "phone-landscape-blur-edge"} pointerEvents="none" style={{ position: "absolute", ...frame, left: frame.left - (p.parentLeft ?? 0), top: frame.top - (p.parentTop ?? 0) }}
    maskElement={<Image source={p.edgeDirection === "bottom" ? require("../assets/artwork-color-fade.png") : require("../assets/artwork-color-fade-horizontal.png")} resizeMode="stretch" style={{ width: frame.width, height: frame.height }} />}>
    {background}
  </MaskedView>;
}

/** The sharp album stays intact while a soft continuation fades into lyrics. */
export function ArtworkRightFeather(p: Props) {
  const frame = artworkRightContinuation(p.artworkLeft, p.artworkTop, p.artworkWidth, p.artworkHeight);
  const MaskedView = nativeMask();
  if (!MaskedView || frame.width <= 0 || frame.height <= 0) return null;
  return <MaskedView testID="tablet-artwork-lyrics-feather" pointerEvents="none"
    style={{ position: "absolute", left: frame.left, top: frame.top, width: frame.width, height: frame.height }}
    maskElement={<View style={{ flexDirection: "row", width: frame.width, height: frame.height }}>
      <Image source={require("../assets/artwork-color-fade-horizontal.png")} resizeMode="stretch" style={{ width: frame.innerWidth, height: frame.height }} />
      <Image source={require("../assets/artwork-color-fade-horizontal.png")} resizeMode="stretch" style={{ width: frame.outerWidth, height: frame.height, transform: [{ scaleX: -1 }] }} />
    </View>}>
      <View testID="tablet-artwork-continuation-paint" style={[StyleSheet.absoluteFill, { backgroundColor: p.backgroundColor ?? "#303034" }]}>
        <Image testID="tablet-artwork-blurred-continuation" source={{ uri: p.uri }} resizeMode="contain" blurRadius={48}
          style={{ position: "absolute", left: frame.imageLeft, top: 0, width: frame.sourceWidth, height: frame.height, opacity: ARTWORK_AMBIENT_PAINT.imageOpacity }} />
        <View testID="tablet-artwork-continuation-scrim" style={[StyleSheet.absoluteFill, { backgroundColor: ARTWORK_AMBIENT_PAINT.scrimColor }]} />
      </View>
  </MaskedView>;
}

/** Apply the bottom ramp once across both the cover and its side continuation. */
export function TabletArtworkFeather(p: Props) {
  const frame = artworkRightContinuation(p.artworkLeft, p.artworkTop, p.artworkWidth, p.artworkHeight);
  return <>
    <ArtworkRightFeather {...p} />
    <PhoneLandscapeBlur {...p} edge edgeDirection="bottom" testID="tablet-artwork-controls-feather"
      artworkWidth={p.artworkWidth + frame.outerWidth} />
  </>;
}
