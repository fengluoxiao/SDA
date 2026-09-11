/** Overlap one bounded file read with decoding, never an unbounded file queue. */
export async function* readAhead(
  size: number, chunkSize: number,
  read: (offset: number, length: number) => Promise<Uint8Array>,
): AsyncGenerator<Uint8Array> {
  if (!Number.isSafeInteger(size) || size < 0 || !Number.isSafeInteger(chunkSize) || chunkSize <= 0) {
    throw new Error("Invalid file read bounds");
  }
  const request = (offset: number) => Promise.resolve()
    .then(() => read(offset, Math.min(chunkSize, size - offset)))
    .then(bytes => ({ok: true as const, bytes}), error => ({ok: false as const, error}));
  let pending = size ? request(0) : undefined;
  try {
    for (let offset = 0; offset < size; offset += chunkSize) {
      const result = await pending!;
      if (!result.ok) throw result.error;
      if (result.bytes.byteLength !== Math.min(chunkSize, size - offset)) {
        throw new Error(`File ended early at byte ${offset}`);
      }
      pending = offset + chunkSize < size ? request(offset + chunkSize) : undefined;
      yield result.bytes;
    }
  } finally {
    // A cancelled player must let its one pending read close before releasing
    // the file handle. Settled results also prevent abandoned read rejections.
    await pending;
  }
}
