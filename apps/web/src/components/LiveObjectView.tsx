import {memo, useSyncExternalStore, type ComponentProps} from "react";
import type {VisualObject} from "@sda/player";
import {ObjectView} from "./ObjectView";

export function createObjectViewStore() {
  let snapshot = {objects: [] as VisualObject[], soundingIds: new Set<number>() as ReadonlySet<number>};
  const listeners = new Set<() => void>();
  return {
    getSnapshot: () => snapshot,
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    publish(objects: VisualObject[], soundingIds: ReadonlySet<number>) {
      if (snapshot.objects === objects && snapshot.soundingIds === soundingIds) return;
      snapshot = {objects, soundingIds};
      listeners.forEach(listener => listener());
    },
  };
}

// Object animation must not rebuild the entire player/settings tree at 30 Hz.
// Keep the full visual cadence here; audio events use their original clocks.
export const LiveObjectView = memo(function LiveObjectView({store, ...props}:
  Omit<ComponentProps<typeof ObjectView>, "objects" | "soundingIds"> & {store: ReturnType<typeof createObjectViewStore>}) {
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot);
  return <ObjectView {...props} {...snapshot}/>;
});
