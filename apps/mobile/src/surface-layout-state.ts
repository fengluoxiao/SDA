export type SurfaceSize = { width: number; height: number };
export type SurfaceLayoutState = {
  window: SurfaceSize; display: SurfaceSize; measured: SurfaceSize | null;
  active: boolean; revision: number; settling: boolean;
};
const valid = (size: SurfaceSize) => Number.isFinite(size.width) && Number.isFinite(size.height) && size.width > 0 && size.height > 0;
const same = (a: SurfaceSize, b: SurfaceSize) => Math.abs(a.width - b.width) < .5 && Math.abs(a.height - b.height) < .5;
const landscape = (size: SurfaceSize) => size.width > size.height;

export function createSurfaceLayoutState(initial: SurfaceSize, active: boolean, notify: (state: SurfaceLayoutState) => void) {
  let state: SurfaceLayoutState = { window: initial, display: initial, measured: null, active, revision: 0, settling: false };
  let candidate: SurfaceSize | null = null;
  const publish = (next: SurfaceLayoutState) => { state = next; notify(state); };
  return {
    get: () => state,
    windowChanged(window: SurfaceSize) {
      if (!valid(window) || same(window, state.window)) return;
      candidate = null;
      // Foreground rotation needs the same atomic handoff as resume. Keep
      // the last display while the root catches up to the new window axis.
      const rotating = state.active && landscape(window) !== landscape(state.display);
      publish({ ...state, window, revision: state.revision + 1,
        settling: state.settling || rotating, measured: rotating ? null : state.measured });
    },
    activityChanged(active: boolean, window: SurfaceSize) {
      const nextWindow = valid(window) ? window : state.window;
      if (state.active === active) { this.windowChanged(nextWindow); return; }
      candidate = null;
      // Keep one committed layout while the OS captures/restores its snapshot.
      publish({ ...state, window: nextWindow, active, measured: null, settling: active, revision: state.revision + 1 });
    },
    measured(size: SurfaceSize, revision: number): boolean {
      if (!state.active || revision !== state.revision || !valid(size)) return false;
      // Native root layout can arrive before the Dimensions notification.
      if (!state.settling && landscape(size) !== landscape(state.display)) {
        candidate = null;
        publish({ ...state, measured: null, settling: true });
      }
      if (state.settling) {
        // Once the window signals a new orientation, an old root size must not
        // switch the presentation back. Delayed window metrics may still be
        // superseded by two consistent, untransformed native layout reads.
        if (landscape(state.window) !== landscape(state.display) && landscape(size) !== landscape(state.window)) {
          candidate = null; return false;
        }
        if (!candidate || !same(candidate, size)) { candidate = size; return false; }
      }
      candidate = null;
      if (state.measured && same(size, state.measured) && !state.settling) return true;
      publish({ ...state, measured: size, display: size, settling: false });
      return true;
    },
  };
}
export function surfaceLayoutSize(state: SurfaceLayoutState) { return state.display; }
