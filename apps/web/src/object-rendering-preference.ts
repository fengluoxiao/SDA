const keys = ["sda-object-rendering-v1", "sda-direct-object-hrtf", "sda-directional-hrtf-v1"] as const;
export function enableObjectRenderingDefaults(): void {
  const changed = keys.some(key => localStorage.getItem(key) !== "true");
  if (!changed) return;
  for (const key of keys) localStorage.setItem(key, "true");
  window.dispatchEvent(new Event("sda-object-rendering-change"));
}
export function readObjectRendering(): boolean {
  enableObjectRenderingDefaults();
  return true;
}
