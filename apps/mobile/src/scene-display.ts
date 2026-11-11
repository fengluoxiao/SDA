import { useEffect } from "react";
export type SceneDisplayCapabilities = { maximumFps: number; supports120: boolean };
export function sceneDisplayCapabilities(): SceneDisplayCapabilities {
  try {
    const value = (globalThis as any).expo?.modules?.SdaEngine?.sceneDisplayCapabilities?.();
    const maximumFps = Number(value?.maximumFps);
    if (Number.isFinite(maximumFps) && maximumFps >= 30) return { maximumFps, supports120: maximumFps >= 119.5 };
  } catch {}
  return { maximumFps: 60, supports120: false };
}
export function useSceneDisplayPolicy(active: boolean, requestedFps: number, fullscreen: boolean): number {
  const rate = requestedFps === 120 && sceneDisplayCapabilities().supports120 ? 120 : 60;
  useEffect(() => {
    const engine = (globalThis as any).expo?.modules?.SdaEngine;
    if (engine?.configureSceneDisplay) {
      engine.configureSceneDisplay(active, rate, active && fullscreen);
      return () => engine.configureSceneDisplay(false, 60, false);
    }
    // Expo Go exposes its own navigation-bar module. Keep its fullscreen UI
    // identical even when this host has no custom SDA display-policy module.
    const navigation = (globalThis as any).expo?.modules?.ExpoNavigationBar;
    let cancelled = false, previous: string | undefined;
    if (active && fullscreen && navigation?.setHidden) {
      void Promise.resolve(navigation.getVisibilityAsync?.() ?? "visible").then(value => {
        if (cancelled) return;
        previous = value;
        return navigation.setHidden(true);
      }).catch(() => {});
    }
    return () => {
      cancelled = true;
      if (previous !== undefined) void Promise.resolve(navigation.setHidden(previous === "hidden")).catch(() => {});
    };
  }, [active, rate, fullscreen]);
  return rate;
}
