/** Expo GL may return null/undefined for an empty shader diagnostic log.
 * Three 0.159 expects strings and calls trim() before checking link status.
 * Normalize only missing logs; retain real diagnostics and native exceptions.
 */
interface NativeShaderLogs {
  getProgramInfoLog(program: unknown): string | null | undefined;
  getShaderInfoLog(shader: unknown): string | null | undefined;
}
const prepared = new WeakSet<NativeShaderLogs>();

export function normalizeNativeShaderLogs(context: NativeShaderLogs): void {
  if (prepared.has(context)) return;
  const readProgram = context.getProgramInfoLog.bind(context);
  const readShader = context.getShaderInfoLog.bind(context);
  context.getProgramInfoLog = program => readProgram(program) ?? "";
  context.getShaderInfoLog = shader => readShader(shader) ?? "";
  prepared.add(context);
}
