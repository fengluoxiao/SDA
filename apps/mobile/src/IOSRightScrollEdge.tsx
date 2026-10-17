import React from "react";
import { Platform, type StyleProp, type ViewStyle } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { requireNativeView } from "expo";
import { iosRightLibraryGutter } from "./ios-ui-model";
import { SIDE_NAVIGATION_WIDTH } from "./SideNavigationRail";

function supportsNativeRightEdge() {
  if (Platform.OS !== "ios" || Number(Platform.Version) < 26) return false;
  try { return (globalThis as any).expo?.modules?.SdaScrollEdge?.isSupported?.() === true; }
  catch { return false; }
}
export const hasNativeRightScrollEdge = supportsNativeRightEdge();
const NativeEdge = hasNativeRightScrollEdge ? requireNativeView<{scrollViewTag:number;style:StyleProp<ViewStyle>;children:React.ReactNode}>("SdaScrollEdge") : null;

// This transparent container owns the actual controls. UIKit uses their edge
// association to render the ScrollView's own effect; no BlurView/tint is added.
export function IOSRightScrollEdge({ scrollViewTag, children }: { scrollViewTag: number; children: React.ReactNode }) {
  const { right } = useSafeAreaInsets();
  if (!NativeEdge) return <>{children}</>;
  const width = Math.max(0,right) + SIDE_NAVIGATION_WIDTH + 8 - iosRightLibraryGutter(right).paddingRight;
  return <NativeEdge scrollViewTag={scrollViewTag} style={{position:"absolute",top:0,bottom:0,right:0,width}}>{children}</NativeEdge>;
}
