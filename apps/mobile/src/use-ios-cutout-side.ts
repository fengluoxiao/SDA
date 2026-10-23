import { useEffect, useState } from "react";
import { AppState, Platform } from "react-native";
import { iosCutoutSideForOrientation, type IOSCutoutSide } from "./ios-ui-model";

// Dimensions and safe insets can be identical after a 180-degree rotation.
// Observe the interface orientation through the same Expo module in Go/release.
export function useIOSCutoutSide(): IOSCutoutSide {
  const [side, setSide] = useState<IOSCutoutSide>(null);
  useEffect(() => {
    if (Platform.OS !== "ios") return;
    let orientation: typeof import("expo-screen-orientation");
    try { orientation = require("expo-screen-orientation"); }
    catch { return; } // Older installed binaries keep conservative cutout safety.
    let active = true, revision = 0;
    const apply = (value: number) => {
      if (active) setSide(iosCutoutSideForOrientation(value));
    };
    const refresh = () => {
      const requestedRevision = ++revision;
      orientation.getOrientationAsync().then(value => {
        if (requestedRevision === revision) apply(value);
      }).catch(() => {});
    };
    const subscription = orientation.addOrientationChangeListener(event => {
      ++revision; // An older async query must not overwrite this rotation.
      apply(event.orientationInfo.orientation);
    });
    const foreground = AppState.addEventListener("change", state => {
      if (state === "active") refresh();
    });
    refresh();
    return () => { active = false; subscription.remove(); foreground.remove(); };
  }, []);
  return side;
}
