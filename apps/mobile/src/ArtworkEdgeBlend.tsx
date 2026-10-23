import React from "react";
import { View } from "react-native";
import { PhoneLandscapeBlur } from "./PhoneLandscapeBlur";

export type ArtworkFrame = { left: number; top: number; width: number; height: number };

// Keep the original sharp image intact. Landscape alone overlays the existing
// screen-aligned Gaussian backdrop into the small right-edge transition.
export function ArtworkEdgeBlend({ children, landscape, uri, surfaceWidth, surfaceHeight, frame, backgroundColor }: {
  children: React.ReactElement; landscape: boolean; uri: string;
  surfaceWidth: number; surfaceHeight: number; backgroundColor: string; frame: ArtworkFrame | null;
}) {
  if (!landscape || !frame) return children;
  return <View testID="tablet-landscape-artwork-blend" style={{ width: "100%", height: "100%" }}>
    {children}
    <PhoneLandscapeBlur edge uri={uri} surfaceWidth={surfaceWidth} surfaceHeight={surfaceHeight}
      artworkLeft={frame.left} artworkTop={frame.top} artworkWidth={frame.width} artworkHeight={frame.height}
      parentLeft={frame.left} parentTop={frame.top} backgroundColor={backgroundColor} />
  </View>;
}
