import {memo, useSyncExternalStore, type ComponentProps} from "react";
import {ObjectActivityFilter, type VisualObject} from "@sda/player";
import {ObjectView} from "./ObjectView";

export function createObjectViewStore() {
  const activity = new ObjectActivityFilter();
  let lastStreamTime = -Infinity;
  let snapshot = {objects: [] as VisualObject[], activeObjects: [] as VisualObject[],
    soundingIds: new Set<number>() as ReadonlySet<number>};
  const listeners = new Set<() => void>();
  return {
    getSnapshot: () => snapshot,
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    publish(objects: VisualObject[], soundingIds: ReadonlySet<number>, streamTimeSec = 0,
        clockSec = performance.now() / 1000) {
      if (objects.length === 0 || streamTimeSec < lastStreamTime || streamTimeSec - lastStreamTime > 0.5) activity.reset();
      lastStreamTime = streamTimeSec;
      const activeObjects = activity.project(objects, soundingIds, clockSec);
      if (snapshot.objects === objects && snapshot.soundingIds === soundingIds
          && snapshot.activeObjects === activeObjects) return;
      snapshot = {objects, activeObjects, soundingIds};
      listeners.forEach(listener => listener());
    },
  };
}

type Store = ReturnType<typeof createObjectViewStore>;
// Keep activity/fade updates inside the scene, without rebuilding settings.
export const LiveObjectView = memo(function LiveObjectView({store, showAllObjects = false, ...props}:
  Omit<ComponentProps<typeof ObjectView>, "objects" | "soundingIds"> & {store: Store; showAllObjects?: boolean}) {
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot);
  return <ObjectView {...props} objects={showAllObjects ? snapshot.objects : snapshot.activeObjects} soundingIds={snapshot.soundingIds}/>;
});

export function LiveObjectCount({store, showAllObjects = false}: {store: Store; showAllObjects?: boolean}) {
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot);
  return <>{snapshot.objects.length && !showAllObjects
    ? `${snapshot.activeObjects.length} 显示 / ${snapshot.objects.length} 对象`
    : `${snapshot.objects.length} 对象`}</>;
}
