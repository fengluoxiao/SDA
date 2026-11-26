import { useCallback, useEffect, useState } from "react";

export type ArtworkSelection = "static" | "motion" | "pending";

export function artworkSelection(enabled: boolean, uri: string, available?: boolean): ArtworkSelection {
  if (!enabled || !uri.startsWith("file:")) return "static";
  return available === undefined ? "pending" : available ? "motion" : "static";
}

export function useMotionArtworkSelection(enabled: boolean, sourceUri: string, nextSourceUri: string) {
  const native = (globalThis as any).expo?.modules?.SdaMotionArtwork as
    { hasMotionArtwork?(uri: string): Promise<boolean> } | undefined;
  const canProbe = enabled && typeof native?.hasMotionArtwork === "function";
  const [available, setAvailable] = useState<Record<string, boolean>>({});
  useEffect(() => {
    let live = true;
    if (!canProbe) { setAvailable({}); return; }
    const sources = [...new Set([sourceUri, nextSourceUri].filter(uri => uri.startsWith("file:")))];
    setAvailable(previous => Object.fromEntries(sources.filter(uri => previous[uri] !== undefined).map(uri => [uri, previous[uri]!])));
    for (const uri of sources) {
      if (available[uri] !== undefined) continue;
      void native!.hasMotionArtwork!(uri).then(value => {
        if (live) setAvailable(previous => ({ ...previous, [uri]: value === true }));
      }).catch(() => {
        if (live) setAvailable(previous => ({ ...previous, [uri]: false }));
      });
    }
    return () => { live = false; };
  }, [canProbe, sourceUri, nextSourceUri]);
  const onUnavailable = useCallback((uri: string) => {
    if (canProbe && (uri === sourceUri || uri === nextSourceUri)) setAvailable(previous => ({ ...previous, [uri]: false }));
  }, [canProbe, sourceUri, nextSourceUri]);
  return {
    current: artworkSelection(canProbe, sourceUri, available[sourceUri]),
    next: artworkSelection(canProbe, nextSourceUri, available[nextSourceUri]),
    onUnavailable,
  };
}
