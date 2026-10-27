import {memo, useSyncExternalStore, type ComponentProps} from "react";
import {ObjectMotionFilter, type VisualObject} from "@sda/player";
import {ObjectView} from "./ObjectView";

export function createObjectViewStore() {
  const motion = new ObjectMotionFilter();
  let snapshot = {objects: [] as VisualObject[], motionObjects: [] as VisualObject[],
    soundingIds: new Set<number>() as ReadonlySet<number>, authored: false};
  const listeners = new Set<() => void>();
  return {
    getSnapshot: () => snapshot,
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    publish(objects: VisualObject[], soundingIds: ReadonlySet<number>, timeSec = 0, authored = false) {
      if (!authored || snapshot.authored !== authored || objects.length === 0) motion.reset();
      const motionObjects = authored ? motion.project(objects, timeSec) : objects;
      if (snapshot.objects === objects && snapshot.soundingIds === soundingIds
          && snapshot.motionObjects === motionObjects && snapshot.authored === authored) return;
      snapshot = {objects, motionObjects, soundingIds, authored};
      listeners.forEach(listener => listener());
    },
  };
}

type Store = ReturnType<typeof createObjectViewStore>;
// Keep movement and fade updates inside the scene, without rebuilding settings.
export const LiveObjectView = memo(function LiveObjectView({store, showStaticObjects = false, ...props}:
  Omit<ComponentProps<typeof ObjectView>, "objects" | "soundingIds"> & {store: Store; showStaticObjects?: boolean}) {
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot);
  return <ObjectView {...props} objects={showStaticObjects ? snapshot.objects : snapshot.motionObjects} soundingIds={snapshot.soundingIds}/>;
});

export function LiveObjectCount({store, showStaticObjects = false}: {store: Store; showStaticObjects?: boolean}) {
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot);
  return <>{snapshot.authored && !showStaticObjects
    ? `${snapshot.motionObjects.length} 显示 / ${snapshot.objects.length} 对象`
    : `${snapshot.objects.length} 对象`}</>;
}
