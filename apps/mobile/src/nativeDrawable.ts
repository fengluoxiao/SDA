/** Expo exposes a GL context without the browser gl.canvas property.
 * Three's resetState reads it even when a separate canvas was supplied to
 * WebGLRenderer. Reuse Fiber's canvas, keeping resize ownership with Fiber.
 */
export function resetNativeDrawable(
  renderer: { domElement: unknown; resetState(): void; setViewport(x: number, y: number, width: number, height: number): void },
  context: { canvas?: unknown; FRAMEBUFFER: number; bindFramebuffer(target: number, buffer: null): void },
  width: number, height: number,
) {
  if (!context.canvas) context.canvas = renderer.domElement;
  renderer.resetState();
  context.bindFramebuffer(context.FRAMEBUFFER, null);
  renderer.setViewport(0, 0, width, height);
}

/** Fiber replaces its root-state snapshot on layout. Read it for every frame:
 * capturing onCreated's state.size stretches and offsets the scene after rotation.
 */
export function resetNativeSceneDrawable(
  getState: () => { gl: Parameters<typeof resetNativeDrawable>[0]; size: { width: number; height: number } },
  context: Parameters<typeof resetNativeDrawable>[1],
) {
  const current = getState();
  resetNativeDrawable(current.gl, context, current.size.width, current.size.height);
}
