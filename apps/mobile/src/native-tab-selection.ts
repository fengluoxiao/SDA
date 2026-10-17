type TabEvent = { selectedScreenKey: string; provenance: number; actionOrigin?: string };
// Mounting, rotation and controlled updates can emit implicit/programmatic
// selections. Acknowledge them, but only user gestures may navigate the app.
export function resolveNativeTabSelection(event: TabEvent, lastProvenance: number, selected: number, active: boolean, host = "unknown") {
  if (typeof __DEV__ !== "undefined" && __DEV__) {
    const debug = globalThis as typeof globalThis & { __SDA_TAB_EVENTS?: unknown[] };
    debug.__SDA_TAB_EVENTS = [...(debug.__SDA_TAB_EVENTS ?? []), {
      host, selected, active, lastProvenance, key: event.selectedScreenKey,
      provenance: event.provenance, origin: event.actionOrigin ?? null,
    }].slice(-24);
  }
  if (!Number.isInteger(event.provenance) || event.provenance < lastProvenance) return null;
  const index = Number(event.selectedScreenKey);
  const userSelection = active && event.actionOrigin === "user" && ["0", "1", "2"].includes(event.selectedScreenKey) && index !== selected;
  return { provenance: event.provenance, index: userSelection ? index : null };
}
