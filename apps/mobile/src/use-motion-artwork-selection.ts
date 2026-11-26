import { useCallback, useEffect, useState } from "react";

export type ArtworkSelection = "static" | "motion" | "pending";
type MotionArtworkInfo = { present: boolean; width?: number; height?: number };

export function artworkSelection(enabled: boolean, uri: string, available?: boolean): ArtworkSelection {
  if (!enabled || !uri.startsWith("file:")) return "static";
  return available === undefined ? "pending" : available ? "motion" : "static";
}

export function motionArtworkAspectRatio(info?: MotionArtworkInfo): number {
  return typeof info?.width === "number" && typeof info.height === "number" && Number.isFinite(info.width) && Number.isFinite(info.height) && info.width > 0 && info.height > 0 ? info.width / info.height : .75;
}

export function useMotionArtworkSelection(enabled: boolean, sourceUri: string, nextSourceUri: string) {
  const native = (globalThis as any).expo?.modules?.SdaMotionArtwork as
    { hasMotionArtwork?(uri: string): Promise<boolean>; motionArtworkInfo?(uri: string): Promise<MotionArtworkInfo> } | undefined;
  const canProbe = enabled && (typeof native?.motionArtworkInfo === "function" || typeof native?.hasMotionArtwork === "function");
  const [available, setAvailable] = useState<Record<string, MotionArtworkInfo>>({});
  useEffect(() => {
    let live = true;
    if (!canProbe) { setAvailable({}); return; }
    const sources = [...new Set([sourceUri, nextSourceUri].filter(uri => uri.startsWith("file:")))];
    setAvailable(previous => Object.fromEntries(sources.filter(uri => previous[uri] !== undefined).map(uri => [uri, previous[uri]!])));
    for (const uri of sources) {
      if (available[uri] !== undefined) continue;
      const info = native!.motionArtworkInfo ? native!.motionArtworkInfo(uri) : native!.hasMotionArtwork!(uri).then(present => ({ present }));
      void info.then(value => {
        if (live) setAvailable(previous => ({ ...previous, [uri]: { ...value, present: value.present === true } }));
      }).catch(() => {
        if (live) setAvailable(previous => ({ ...previous, [uri]: { present: false } }));
      });
    }
    return () => { live = false; };
  }, [canProbe, sourceUri, nextSourceUri]);
  const onUnavailable = useCallback((uri: string) => {
    if (canProbe && (uri === sourceUri || uri === nextSourceUri)) setAvailable(previous => ({ ...previous, [uri]: { present: false } }));
  }, [canProbe, sourceUri, nextSourceUri]);
  return {
    current: artworkSelection(canProbe, sourceUri, available[sourceUri]?.present),
    next: artworkSelection(canProbe, nextSourceUri, available[nextSourceUri]?.present),
    aspectRatio: motionArtworkAspectRatio(available[sourceUri]),
    onUnavailable,
  };
}
