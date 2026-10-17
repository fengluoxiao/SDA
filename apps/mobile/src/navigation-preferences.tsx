import React, { createContext, useContext, useEffect, useRef, useState } from "react";
import * as FileSystem from "expo-file-system/legacy";

const Context = createContext({ landscapeSideTabs: false, setLandscapeSideTabs: (_enabled: boolean) => {} });
export const useNavigationPreferences = () => useContext(Context);

export function NavigationPreferencesProvider({ children }: { children: React.ReactNode }) {
  const [landscapeSideTabs, setEnabled] = useState(false);
  const changed = useRef(false);
  const writes = useRef(Promise.resolve());
  const file = FileSystem.documentDirectory ? FileSystem.documentDirectory + "sda-navigation.json" : null;
  useEffect(() => {
    let active = true;
    if (file) FileSystem.readAsStringAsync(file).then(text => {
      const value = JSON.parse(text).landscapeSideTabs;
      if (active && !changed.current && typeof value === "boolean") setEnabled(value);
    }).catch(() => {});
    return () => { active = false; };
  }, [file]);
  const setLandscapeSideTabs = (enabled: boolean) => {
    changed.current = true; setEnabled(enabled);
    if (file) writes.current = writes.current.catch(() => {}).then(() =>
      FileSystem.writeAsStringAsync(file, JSON.stringify({ landscapeSideTabs: enabled }))).catch(() => {});
  };
  return <Context.Provider value={{ landscapeSideTabs, setLandscapeSideTabs }}>{children}</Context.Provider>;
}
