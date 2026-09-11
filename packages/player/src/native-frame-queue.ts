/** Keep one frame's metadata, declarations and PCM together. The caller's
 * in-flight sample limit bounds this queue; failures must not block a retry. */
export class NativeFrameQueue {
  private tail: Promise<void> = Promise.resolve();
  submit(task: () => Promise<void>): Promise<void> {
    const result = this.tail.then(task);
    this.tail = result.catch(() => {});
    return result;
  }
}
