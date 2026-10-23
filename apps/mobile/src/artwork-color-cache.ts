export function validArtworkColor(value: unknown): value is string {
  if (typeof value !== "string") return false;
  const match = /^rgb\((\d{1,3}),(\d{1,3}),(\d{1,3})\)$/.exec(value);
  return !!match && match.slice(1).every(channel => Number(channel) <= 255);
}

// Memory-only, bounded and single-flight. Native metadata can prime a result
// before a queued Expo Go read starts, without waiting for image/GL creation.
export function createArtworkColorCache(sample: (uri: string) => Promise<string>, limit = 24) {
  const colors = new Map<string, string>();
  const pending = new Map<string, Promise<string>>();
  let tail: Promise<unknown> = Promise.resolve();
  const peek = (uri: string) => colors.get(uri);
  const prime = (uri: string, color: unknown) => {
    if (!uri || !validArtworkColor(color)) return;
    colors.delete(uri); colors.set(uri, color);
    while (colors.size > limit) colors.delete(colors.keys().next().value!);
  };
  const load = (uri: string): Promise<string> => {
    const cached = peek(uri);
    if (cached) return Promise.resolve(cached);
    const existing = pending.get(uri);
    if (existing) return existing;
    const request = tail.then(() => peek(uri) || sample(uri)).then(color => {
      if (!validArtworkColor(color)) throw new Error("Invalid artwork color");
      const resolved = peek(uri) || color;
      prime(uri, resolved); return resolved;
    }).finally(() => { pending.delete(uri); });
    pending.set(uri, request);
    tail = request.catch(() => undefined);
    return request;
  };
  return { peek, prime, load };
}
