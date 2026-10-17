import { useCallback, useEffect, useRef, useState } from "react";
import { AppState, Dimensions, View } from "react-native";
import { createSurfaceLayoutState, surfaceLayoutSize, type SurfaceLayoutState } from "./surface-layout-state";

export function usePlayerSurfaceLayout() {
  const [snapshot, setSnapshot] = useState<SurfaceLayoutState>(() => ({
    window: Dimensions.get("window"), display: Dimensions.get("window"), measured: null, active: AppState.currentState === "active", revision: 0, settling: false,
  }));
  const model = useRef<ReturnType<typeof createSurfaceLayoutState> | null>(null);
  if (!model.current) model.current = createSurfaceLayoutState(snapshot.window, snapshot.active, setSnapshot);
  const controller = model.current;
  const surfaceRef = useRef<View>(null);
  const pendingFrame = useRef<number | null>(null);
  const mounted = useRef(false);
  const latestRead = useRef(0);
  const measureSurface = useCallback(function requestMeasure(retries = 12) {
    if (!mounted.current || !controller.get().active || pendingFrame.current !== null) return;
    pendingFrame.current = requestAnimationFrame(() => {
      pendingFrame.current = null;
      if (!mounted.current || !controller.get().active) return;
      const revision = controller.get().revision;
      const read = ++latestRead.current;
      const node = surfaceRef.current as (View & { offsetWidth?: number; offsetHeight?: number }) | null;
      if (!node) return;
      const accept = (width: number, height: number) => {
        if (!mounted.current || read !== latestRead.current) return;
        const accepted = controller.measured({ width, height }, revision);
        if (!accepted && controller.get().active && controller.get().settling && revision === controller.get().revision && retries > 0) requestMeasure(retries - 1);
      };
      // Fabric's offset size excludes transforms. measure() includes them and
      // may feed transition bounds back into the layout during app restoration.
      const width = node.offsetWidth, height = node.offsetHeight;
      if (typeof width === "number" && typeof height === "number") accept(width, height);
      else node.measureLayout(node, (_x, _y, w, h) => accept(w, h), () => {});
    });
  }, [controller]);
  useEffect(() => {
    mounted.current = true;
    const window = Dimensions.addEventListener("change", ({ window }) => {
      controller.windowChanged(window); measureSurface();
    });
    const activity = AppState.addEventListener("change", value => {
      controller.activityChanged(value === "active", Dimensions.get("window"));
      if (value !== "active" && pendingFrame.current !== null) {
        cancelAnimationFrame(pendingFrame.current); pendingFrame.current = null;
      }
      measureSurface();
    });
    controller.activityChanged(AppState.currentState === "active", Dimensions.get("window"));
    measureSurface();
    return () => {
      mounted.current = false; ++latestRead.current; window.remove(); activity.remove();
      if (pendingFrame.current !== null) cancelAnimationFrame(pendingFrame.current);
      pendingFrame.current = null;
    };
  }, [controller, measureSurface]);
  return { ...surfaceLayoutSize(snapshot), surfaceRef, onSurfaceLayout: () => measureSurface(),
    layoutReady: snapshot.active && snapshot.measured !== null };
}
