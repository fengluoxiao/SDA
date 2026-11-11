import { CARDBOARD_DEFAULTS, normalizeCardboardConfig, type CardboardConfig } from "./cardboard-config";
import React, { createContext, useContext, useEffect, useRef, useState } from "react";
import * as FileSystem from "expo-file-system/legacy";

export type SideTabPosition = "left" | "right";
// Keep the saved key stable: this style switch also controls phone landscape blur.
type Preferences = { cardboard: CardboardConfig; landscapeSideTabs: boolean; tabletPortraitLayout: boolean; immersiveSpace: boolean; showChannelSpeakers: boolean; space120Fps: boolean; sideTabPosition: SideTabPosition };
const defaults: Preferences = { cardboard: CARDBOARD_DEFAULTS, landscapeSideTabs: false, tabletPortraitLayout: false, immersiveSpace: false, showChannelSpeakers: false, space120Fps: false, sideTabPosition: "right" };
const Context = createContext({ ...defaults, setCardboard: (_value: CardboardConfig) => {}, setLandscapeSideTabs: (_enabled: boolean) => {}, setTabletPortraitLayout: (_enabled: boolean) => {}, setImmersiveSpace: (_enabled: boolean) => {}, setShowChannelSpeakers: (_enabled: boolean) => {}, setSpace120Fps: (_enabled: boolean) => {}, setSideTabPosition: (_position: SideTabPosition) => {} });
export const useNavigationPreferences = () => useContext(Context);

export function NavigationPreferencesProvider({ children }: { children: React.ReactNode }) {
  const [preferences, setPreferences] = useState(defaults);
  const current = useRef(preferences);
  const changed = useRef(new Set<keyof Preferences>());
  const writes = useRef(Promise.resolve());
  const file = FileSystem.documentDirectory ? FileSystem.documentDirectory + "sda-navigation.json" : null;
  useEffect(() => {
    let active = true;
    if (file) FileSystem.readAsStringAsync(file).then(text => {
      const saved = JSON.parse(text);
      if (!active || !saved || typeof saved !== "object") return;
      const restored = { ...current.current };
      if (!changed.current.has("cardboard") && saved.cardboard) restored.cardboard = normalizeCardboardConfig(saved.cardboard);
      if (!changed.current.has("landscapeSideTabs") && typeof saved.landscapeSideTabs === "boolean") restored.landscapeSideTabs = saved.landscapeSideTabs;
      if (!changed.current.has("sideTabPosition") && (saved.sideTabPosition === "left" || saved.sideTabPosition === "right")) restored.sideTabPosition = saved.sideTabPosition;
      if (!changed.current.has("tabletPortraitLayout") && typeof saved.tabletPortraitLayout === "boolean") restored.tabletPortraitLayout = saved.tabletPortraitLayout;
      if (!changed.current.has("immersiveSpace") && typeof saved.immersiveSpace === "boolean") restored.immersiveSpace = saved.immersiveSpace;
      if (!changed.current.has("space120Fps") && typeof saved.space120Fps === "boolean") restored.space120Fps = saved.space120Fps;
      if (!changed.current.has("showChannelSpeakers") && typeof saved.showChannelSpeakers === "boolean") restored.showChannelSpeakers = saved.showChannelSpeakers;
      const restoredUntouchedField = JSON.stringify(restored.cardboard) !== JSON.stringify(current.current.cardboard) || restored.landscapeSideTabs !== current.current.landscapeSideTabs || restored.sideTabPosition !== current.current.sideTabPosition || restored.tabletPortraitLayout !== current.current.tabletPortraitLayout || restored.immersiveSpace !== current.current.immersiveSpace || restored.space120Fps !== current.current.space120Fps || restored.showChannelSpeakers !== current.current.showChannelSpeakers;
      current.current = restored; setPreferences(restored);
      // Preserve untouched saved fields when a user acted before the read finished.
      if (changed.current.size && restoredUntouchedField) save(restored);
    }).catch(() => {});
    return () => { active = false; };
  }, [file]);
  const save = (value: Preferences) => {
    if (file) writes.current = writes.current.catch(() => {}).then(() =>
      FileSystem.writeAsStringAsync(file, JSON.stringify(value))).catch(() => {});
  };
  const update = (key: keyof Preferences, value: boolean | SideTabPosition | CardboardConfig) => {
    changed.current.add(key);
    const next = { ...current.current, [key]: value };
    current.current = next; setPreferences(next); save(next);
  };
  const setCardboard = (value: CardboardConfig) => update("cardboard", normalizeCardboardConfig(value));
  const setLandscapeSideTabs = (enabled: boolean) => update("landscapeSideTabs", enabled);
  const setTabletPortraitLayout = (enabled: boolean) => update("tabletPortraitLayout", enabled);
  const setImmersiveSpace = (enabled: boolean) => update("immersiveSpace", enabled);
  const setShowChannelSpeakers = (enabled: boolean) => update("showChannelSpeakers", enabled);
  const setSpace120Fps = (enabled: boolean) => update("space120Fps", enabled);
  const setSideTabPosition = (position: SideTabPosition) => { if (position === "left" || position === "right") update("sideTabPosition", position); };
  return <Context.Provider value={{ ...preferences, setCardboard, setLandscapeSideTabs, setSideTabPosition, setTabletPortraitLayout, setImmersiveSpace, setShowChannelSpeakers, setSpace120Fps }}>{children}</Context.Provider>;
}
