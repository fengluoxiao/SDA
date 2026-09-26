/** Keep one frame's metadata, declarations and PCM together. The caller's
 * in-flight sample limit bounds this queue; failures must not block a retry. */
export class NativeFrameQueue {
  private tail: Promise<void> = Promise.resolve();
  private generation = 0;

  /**
   * Drop work which has not begun yet when a renderer generation is replaced.
   */
  invalidatePending(): void {
    this.generation++;
  }

  submit(task: () => Promise<void>): Promise<void> {
    const generation = this.generation;
    const result = this.tail.then(() => {
      if (generation !== this.generation) return;
      return task();
    });
    this.tail = result.catch(() => {});
    return result;
  }
}
