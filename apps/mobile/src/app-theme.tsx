import React, { createContext, useContext, useEffect, useRef, useState } from "react";
import * as FileSystem from "expo-file-system/legacy";

// Image Color fields verified on BanG Dream! Wiki character pages, 2026-10-10.
export const CHARACTER_THEMES = [
  { id: "sakiko", name: "丰川祥子", color: "#7799CC" },
  { id: "arale", name: "仲町阿拉蕾", color: "#FFDD33" },
  { id: "yuno", name: "千石由乃", color: "#FF6688" },
  { id: "miyako", name: "藤都子", color: "#9977DD" },
  { id: "uika", name: "三角初华", color: "#BB9955" },
] as const;
export type ThemeId = typeof CHARACTER_THEMES[number]["id"];
export function validTheme(value: unknown): value is ThemeId { return CHARACTER_THEMES.some(t => t.id === value); }
const defaultTheme = { id: "sakiko" as ThemeId, accent: "#7799CC", setTheme: (_id: ThemeId) => {} };
const Context = createContext(defaultTheme);
export const useAppTheme = () => useContext(Context);

export function AppThemeProvider({ children }: { children: React.ReactNode }) {
  const [id, setId] = useState<ThemeId>("sakiko");
  const changed = useRef(false);
  const writes = useRef(Promise.resolve());
  // User preference only, never an audio analysis/cache or an import prerequisite.
  const preferenceFile = FileSystem.documentDirectory ? FileSystem.documentDirectory + "sda-theme.json" : null;
  useEffect(() => {
    let active = true;
    if (preferenceFile) FileSystem.readAsStringAsync(preferenceFile).then(text => {
      const value = JSON.parse(text).theme;
      if (active && !changed.current && validTheme(value)) setId(value);
    }).catch(() => {});
    return () => { active = false; };
  }, [preferenceFile]);
  const setTheme = (value: ThemeId) => {
    if (!validTheme(value)) return;
    changed.current = true; setId(value);
    if (preferenceFile) writes.current = writes.current.catch(() => {}).then(() => FileSystem.writeAsStringAsync(preferenceFile, JSON.stringify({ theme: value }))).catch(() => {});
  };
  return <Context.Provider value={{ id, accent: CHARACTER_THEMES.find(t => t.id === id)!.color, setTheme }}>{children}</Context.Provider>;
}
