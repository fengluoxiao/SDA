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
