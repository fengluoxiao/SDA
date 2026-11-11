/** Serialize lock/restore operations, including rapid enter/exit/re-enter. */
let orientationQueue: Promise<unknown> = Promise.resolve();
export function acquireCardboardOrientation(api: {
  getOrientationLockAsync(): Promise<number>; lockAsync(lock: number): Promise<void>;
}, landscapeLock: number, onFailure: () => void) {
  let active = true, original: number | undefined;
  orientationQueue = orientationQueue.catch(() => {}).then(async () => {
    if (!active) return;
    try {
      original = await api.getOrientationLockAsync();
      if (active) await api.lockAsync(landscapeLock);
    } catch { if (active) onFailure(); }
  });
  return () => {
    active = false;
    orientationQueue = orientationQueue.catch(() => {}).then(async () => {
      if (original !== undefined) { try { await api.lockAsync(original); } catch {} }
    });
  };
}
