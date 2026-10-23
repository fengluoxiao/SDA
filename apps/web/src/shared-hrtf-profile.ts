/** Shared playback defaults; no song identity or audio measurements enter here. */
export const SHARED_HRTF_PROFILE = "apple-hrtfhq-542-v1";
interface Preferences { getItem(key: string): string | null; setItem(key: string, value: string): void }
export function migrateSharedHrtf(storage: Preferences, profile: string): boolean {
  if (profile !== SHARED_HRTF_PROFILE || storage.getItem("sda-shared-hrtf-profile") === profile) return false;
  const previous = storage.getItem("sda-binaural-head");
  // Keep an explicitly chosen human/personal profile. The new built-in default
  // replaces the old default KU100 path on existing installations.
  const explicitOther = previous !== null && /^(?:d2|h(?:[3-9]|1[0-9]|20)|personal-[a-f0-9]{64})$/.test(previous);
  if (!explicitOther) {
    for (const [key, value] of Object.entries({
      "sda-binaural-head": "hrtfhq", "sda-dense-binaural-objects": "1",
      "sda-ku100-calibration": "1", "sda-direct-object-hrtf": "true",
      "sda-directional-hrtf-v1": "true",
      "sda-near-field-v1": JSON.stringify({enabled: false, metresPerUnit: 1}),
    })) storage.setItem(key, value);
  }
  storage.setItem("sda-shared-hrtf-profile", profile);
  return true;
}
export function playbackHrtfSet(head: string, dense: boolean, calibrated: boolean, shared: boolean): string {
  if (head === "hrtfhq") return dense ? "hrtf-dense" : "hrtf";
  if (head === "ku100") {
    if (shared && calibrated) return `hrtf-ku100-reference${dense ? "-dense" : ""}`;
    return `${dense ? "hrtf-dense" : "hrtf"}${calibrated ? "" : "-raw"}`;
  }
  return `hrtf-${head}${dense && !head.startsWith("personal-") ? "-dense" : ""}`;
}
