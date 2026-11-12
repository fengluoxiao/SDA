import React, { createContext, useContext, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { AppState } from "react-native";
import * as FileSystem from "expo-file-system/legacy";
import * as SecureStore from "expo-secure-store";
import { AI_SCENE_DEFAULTS, aiSceneEndpoint, normalizeAiSceneSettings, type AiSceneSettings, type AiSceneSong } from "./ai-scene";
import { AiSceneSession } from "./ai-scene-session";
import { AiSceneSettingsModal } from "./AiSceneSettings";
const KEY_ID = "sda.ai-scene.api-key.v1";
const KEY_OPTIONS = { keychainService: "app.sda.ai-scene", keychainAccessible: SecureStore.WHEN_UNLOCKED_THIS_DEVICE_ONLY };
export type AiSceneContextValue = ReturnType<typeof useSceneProvider>;
const Context = createContext<AiSceneContextValue | null>(null);
export const useAiScene = () => {
  const value = useContext(Context);
  if (!value) throw new Error("AI scene provider missing");
  return value;
};
function useSceneProvider(song: AiSceneSong) {
  const [settings, setSettings] = useState(AI_SCENE_DEFAULTS);
  const [apiKey, setApiKey] = useState("");
  const [ready, setReady] = useState(false), [saving, setSaving] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [preferenceError, setPreferenceError] = useState("");
  const [session] = useState(() => new AiSceneSession());
  const state = useSyncExternalStore(session.subscribe, session.snapshot, session.snapshot);
  const songRef = useRef(song); songRef.current = song;
  const credentials = useRef({ settings, apiKey });
  const writing = useRef(false);
  const file = FileSystem.documentDirectory ? FileSystem.documentDirectory + "sda-ai-scene-settings.json" : null;
  useEffect(() => {
    let active = true;
    void (async () => {
      let saved: AiSceneSettings = AI_SCENE_DEFAULTS, key = "";
      if (file) {
        try { saved = normalizeAiSceneSettings(JSON.parse(await FileSystem.readAsStringAsync(file))); } catch { /* New installation or malformed settings: default off. */ }
      }
      try {
        const stored = await SecureStore.getItemAsync(KEY_ID, KEY_OPTIONS);
        if (stored) {
          const credential = JSON.parse(stored);
          if (credential.baseUrl === saved.baseUrl && typeof credential.apiKey === "string") key = credential.apiKey;
          else if (active) setPreferenceError("AI 服务地址已变化，请重新输入 API Key");
        }
      }
      catch { if (active) setPreferenceError("暂时无法读取安全存储，请重新输入 API Key"); }
      if (active) { credentials.current = { settings: saved, apiKey: key }; setSettings(saved); setApiKey(key); setReady(true); }
    })();
    return () => { active = false; };
  }, [file]);
  useEffect(() => {
    if (!ready) return;
    session.configure(settings, apiKey, { id: song.id, title: song.title, artist: song.artist, album: song.album, year: song.year });
    if (AppState.currentState !== "active") session.cancel(); else session.resume();
  }, [ready, settings, apiKey, song.id, song.title, song.artist, song.album, song.year, session]);
  useEffect(() => {
    const subscription = AppState.addEventListener("change", value => value === "active" ? session.resume() : session.cancel());
    return () => { subscription.remove(); session.cancel(); };
  }, [session]);
  const save = async (input: AiSceneSettings, replacementKey?: string, forget = false): Promise<boolean> => {
    if (!ready || writing.current) return false;
    const next = normalizeAiSceneSettings(input), before = credentials.current;
    const key = forget ? "" : replacementKey?.trim() || before.apiKey;
    try {
      if (next.baseUrl) aiSceneEndpoint(next.baseUrl);
      if (next.enabled && (!next.baseUrl || !next.model || !key)) throw new Error("请填写 Base URL、模型和 API Key 后开启");
      if (key.length > 2048 || /[\r\n]/.test(key)) throw new Error("API Key 格式不正确");
    } catch (error) { setPreferenceError(error instanceof Error ? error.message : "请检查设置"); return false; }
    writing.current = true; setSaving(true); setPreferenceError("");
    let changedKey = false;
    try {
      if (forget || key !== before.apiKey || (!!key && next.baseUrl !== before.settings.baseUrl)) {
        try {
          if (key) await SecureStore.setItemAsync(KEY_ID, JSON.stringify({ baseUrl: next.baseUrl, apiKey: key }), KEY_OPTIONS); else await SecureStore.deleteItemAsync(KEY_ID, KEY_OPTIONS);
          changedKey = true;
        } catch { throw new Error("无法安全保存 API Key，请检查系统安全存储"); }
      }
      if (!file) throw new Error("当前环境无法保存 AI 设置");
      try { await FileSystem.writeAsStringAsync(file, JSON.stringify(next)); }
      catch { throw new Error("AI 设置保存失败，请重试"); }
      credentials.current = { settings: next, apiKey: key }; setSettings(next); setApiKey(key);
      session.configure(next, key, songRef.current);
      if (AppState.currentState !== "active") session.cancel();
      return true;
    } catch (error) {
      if (changedKey) {
        try { if (before.apiKey) await SecureStore.setItemAsync(KEY_ID, JSON.stringify({ baseUrl: before.settings.baseUrl, apiKey: before.apiKey }), KEY_OPTIONS); else await SecureStore.deleteItemAsync(KEY_ID, KEY_OPTIONS); }
        catch { const disabled = { ...before.settings, enabled: false }; credentials.current = { settings: disabled, apiKey: "" }; setSettings(disabled); setApiKey(""); session.configure(disabled, "", songRef.current); }
      }
      setPreferenceError(error instanceof Error ? error.message : "保存失败，请重试"); return false;
    } finally { writing.current = false; setSaving(false); }
  };
  return { settings, ready, saving, hasApiKey: !!apiKey, settingsOpen, preferenceError,
    scene: settings.enabled && state.trackId === song.id ? state.scene : null,
    pending: state.pending, error: state.error, songTitle: song.title || "",
    openSettings: () => setSettingsOpen(true), closeSettings: () => { if (!writing.current) setSettingsOpen(false); },
    save, regenerate: session.generate, cancel: session.cancel };
}
export function AiSceneProvider({ song, children }: React.PropsWithChildren<{ song: AiSceneSong }>) {
  const value = useSceneProvider(song);
  return <Context.Provider value={value}>{children}<AiSceneSettingsModal value={value} /></Context.Provider>;
}
