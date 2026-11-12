import { Platform } from "react-native";
import * as FileSystem from "expo-file-system/legacy";
import * as SecureStore from "expo-secure-store";

let complete = false;
let inFlight: Promise<boolean> | null = null;
/** Upgrade cleanup for the removed feature. Never read or migrate the old credential. */
export function cleanupRetiredAiSettings(): Promise<boolean> {
  if (complete || (Platform.OS !== "ios" && Platform.OS !== "android")) return Promise.resolve(true);
  if (inFlight) return inFlight;
  inFlight = Promise.allSettled([
    Promise.resolve().then(() => SecureStore.deleteItemAsync("sda.ai-scene.api-key.v1", { keychainService: "app.sda.ai-scene" })),
    Promise.resolve().then(() => FileSystem.documentDirectory ? FileSystem.deleteAsync(FileSystem.documentDirectory + "sda-ai-scene-settings.json", { idempotent: true }) : undefined),
  ]).then(results => { complete = results.every(result => result.status === "fulfilled"); return complete; })
    .finally(() => { inFlight = null; });
  return inFlight;
}
