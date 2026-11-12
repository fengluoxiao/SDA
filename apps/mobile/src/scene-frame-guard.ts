/** A useFrame callback is outside React error boundaries. Latch an individual
 * visual failure, report it once, and let the native audio player continue. */
export class SceneFrameGuard {
  failed = false;
  fail(error: unknown, report?: (error: unknown) => void): void {
    if (this.failed) return;
    this.failed = true; report?.(error);
  }
  run(frame: () => void, report?: (error: unknown) => void): boolean {
    if (this.failed) return false;
    try { frame(); return true; }
    catch (error) { this.fail(error, report); return false; }
  }
}
