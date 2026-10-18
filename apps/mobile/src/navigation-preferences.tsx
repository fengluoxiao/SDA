import React, { createContext, useContext, useEffect, useRef, useState } from "react";
import * as FileSystem from "expo-file-system/legacy";

export type SideTabPosition = "left" | "right";
// Keep the saved key stable: this style switch also controls phone landscape blur.
type Preferences = { landscapeSideTabs: boolean; tabletPortraitLayout: boolean; sideTabPosition: SideTabPosition };
const defaults: Preferences = { landscapeSideTabs: false, tabletPortraitLayout: false, sideTabPosition: "right" };
const Context = createContext({ ...defaults, setLandscapeSideTabs: (_enabled: boolean) => {}, setTabletPortraitLayout: (_enabled: boolean) => {}, setSideTabPosition: (_position: SideTabPosition) => {} });
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
      if (!changed.current.has("landscapeSideTabs") && typeof saved.landscapeSideTabs === "boolean") restored.landscapeSideTabs = saved.landscapeSideTabs;
      if (!changed.current.has("sideTabPosition") && (saved.sideTabPosition === "left" || saved.sideTabPosition === "right")) restored.sideTabPosition = saved.sideTabPosition;
      if (!changed.current.has("tabletPortraitLayout") && typeof saved.tabletPortraitLayout === "boolean") restored.tabletPortraitLayout = saved.tabletPortraitLayout;
      const restoredUntouchedField = restored.landscapeSideTabs !== current.current.landscapeSideTabs || restored.sideTabPosition !== current.current.sideTabPosition || restored.tabletPortraitLayout !== current.current.tabletPortraitLayout;
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
  const update = (key: keyof Preferences, value: boolean | SideTabPosition) => {
    changed.current.add(key);
    const next = { ...current.current, [key]: value };
    current.current = next; setPreferences(next); save(next);
  };
  const setLandscapeSideTabs = (enabled: boolean) => update("landscapeSideTabs", enabled);
  const setTabletPortraitLayout = (enabled: boolean) => update("tabletPortraitLayout", enabled);
  const setSideTabPosition = (position: SideTabPosition) => { if (position === "left" || position === "right") update("sideTabPosition", position); };
  return <Context.Provider value={{ ...preferences, setLandscapeSideTabs, setSideTabPosition, setTabletPortraitLayout }}>{children}</Context.Provider>;
}
