import React from "react";
import { Image, Platform, StyleSheet, View } from "react-native";
import { phoneLandscapeBlurEdge, artworkBackdropBottomEdge, portraitMotionBackdropEdge, artworkRightContinuation, ARTWORK_AMBIENT_PAINT, type ArtworkAmbientPaint } from "./phone-landscape-blur-model";

type Props = {
  uri: string; surfaceWidth: number; surfaceHeight: number;
  artworkLeft: number; artworkTop: number; artworkWidth: number; artworkHeight: number;
  edge?: boolean; edgeWidth?: number; edgeDirection?: "right" | "bottom"; edgeHeight?: number; portraitMotionEdge?: boolean; testID?: string; parentLeft?: number; parentTop?: number; backgroundColor?: string;
  paint?: ArtworkAmbientPaint;
};

type MotionBackdropProps = {
  children: React.ReactNode;
  surfaceWidth: number; surfaceHeight: number;
  artworkTop: number; artworkHeight: number; fallbackColor: string;
  artworkLeft?: number; artworkWidth?: number;
  enabled?: boolean; mode?: "portrait" | "phone-landscape" | "tablet-landscape" | "none";
};

type QueuedArtworkProps = {
  uri: string; nextUri: string; resizeMode: "contain" | "cover";
  style: object; blurRadius?: number; onError?(uri: string): void;
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

export function MotionArtworkFeather(p: MotionBackdropProps) {
  if (p.enabled === false) return <>{p.children}</>;
  const MaskedView = nativeMask();
  const mode=p.mode??"portrait";
  const boundary = p.artworkTop + p.artworkHeight;
  const span = mode==="portrait" || mode==="tablet-landscape" ? Math.min(boundary,p.artworkHeight*(mode==="portrait"?.30:.35)):0;
  const start = boundary - span;
  const artworkWidth=p.artworkWidth??p.surfaceWidth;
  const rightSpan=mode==="phone-landscape"?artworkWidth*.30:mode==="tablet-landscape"?Math.min(48,artworkWidth*.10):0;
  const rightStart=(p.artworkLeft??0)+artworkWidth-rightSpan;
  if (!MaskedView) return <>
    {p.children}
    {span > 0 && <Image source={require("../assets/artwork-color-fade.png")} resizeMode="stretch"
      style={{ position: "absolute", top: start, width: p.surfaceWidth, height: span, tintColor: p.fallbackColor }} />}
  </>;
  const fullMask=<View style={{width:p.surfaceWidth,height:p.surfaceHeight,backgroundColor:"#ffffff"}} />;
  return <MaskedView testID="motion-artwork-horizontal-mask" pointerEvents="none" style={StyleSheet.absoluteFill}
    maskElement={rightSpan>0?<View style={{width:p.surfaceWidth,height:p.surfaceHeight}}>
      <View style={{position:"absolute",left:0,top:0,width:Math.max(0,rightStart),height:p.surfaceHeight,backgroundColor:"#ffffff"}} />
      <Image source={require("../assets/artwork-color-fade-horizontal.png")} resizeMode="stretch" fadeDuration={0}
        style={{position:"absolute",left:rightStart,top:0,width:rightSpan,height:p.surfaceHeight,transform:[{scaleX:-1}]}} />
    </View>:fullMask}>
  <MaskedView testID="tablet-motion-backdrop-mask" pointerEvents="none" style={StyleSheet.absoluteFill}
    maskElement={span>0?<View style={{ width: p.surfaceWidth, height: p.surfaceHeight }}>
      <View style={{ height: start, backgroundColor: "#ffffff" }} />
      <Image source={require("../assets/artwork-color-fade.png")} resizeMode="stretch" fadeDuration={0}
        style={{ width: p.surfaceWidth, height: span, transform: [{ scaleY: -1 }] }} />
    </View>:fullMask}>
    {p.children}
  </MaskedView>
  </MaskedView>;
}

export const QueuedArtwork = React.memo(function QueuedArtwork({ uri, nextUri, resizeMode, style, blurRadius, onError }: QueuedArtworkProps) {
  const sources = [...new Set([uri, nextUri].filter(Boolean))];
  return <>{sources.map(sourceUri => <Image key={sourceUri} source={{ uri: sourceUri }} resizeMode={resizeMode}
    blurRadius={blurRadius} fadeDuration={0}
    onError={sourceUri === uri && onError ? () => onError(sourceUri) : undefined}
    style={[style, { opacity: sourceUri === uri ? 1 : 0 }]} />)}</>;
});

// The edge samples the same screen-aligned backdrop as the background. A small
// alpha mask blends into the sharp cover without moving or resizing that cover.
export function PhoneLandscapeBlur(p: Props) {
  const paint=p.paint??ARTWORK_AMBIENT_PAINT;
  const frame = p.portraitMotionEdge
    ? portraitMotionBackdropEdge(p.artworkLeft, p.artworkTop, p.artworkWidth, p.artworkHeight, p.edgeHeight)
    : p.edgeDirection === "bottom"
    ? artworkBackdropBottomEdge(p.artworkLeft, p.artworkTop, p.artworkWidth, p.artworkHeight, p.edgeHeight)
    : phoneLandscapeBlurEdge(p.artworkLeft, p.artworkTop, p.artworkWidth, p.artworkHeight, p.edgeWidth);
  const background = <View style={{ position: "absolute", left: p.edge ? -frame.left : 0, top: p.edge ? -frame.top : 0, width: p.surfaceWidth, height: p.surfaceHeight, backgroundColor: p.backgroundColor ?? "#303034" }}>
    <Image source={{ uri: p.uri }} resizeMode="cover" blurRadius={48} style={[StyleSheet.absoluteFill, { opacity: paint.imageOpacity }]} />
    <View style={[StyleSheet.absoluteFill, { backgroundColor: paint.scrimColor }]} />
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
    style={{ position: "absolute", left: frame.left - (p.parentLeft ?? 0), top: frame.top - (p.parentTop ?? 0), width: frame.width, height: frame.height }}
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

/** Portrait tablet covers only need the control-side ramp; no side pane follows them. */
export function ArtworkBottomFeather(p: Props) {
  return <PhoneLandscapeBlur {...p} edge portraitMotionEdge edgeDirection="bottom" paint={ARTWORK_AMBIENT_PAINT} testID="tablet-artwork-controls-feather"
    artworkWidth={p.artworkWidth} />;
}
