export interface ListeningProfile { rear: number; height: number; bassDb: number; trebleDb: number }
export const DEFAULT_LISTENING_PROFILE: Readonly<ListeningProfile> = Object.freeze({ rear: 1, height: 1, bassDb: 0, trebleDb: 0 });
export function listeningProfile(value: unknown): ListeningProfile {
  let v = value;
  try { if (typeof v === "string") v = JSON.parse(v); } catch { return { ...DEFAULT_LISTENING_PROFILE }; }
  const s = v as Partial<ListeningProfile> | null;
  if (!s || Object.keys(DEFAULT_LISTENING_PROFILE).some(key => {
    const k = key as keyof ListeningProfile; const n = s[k];
    return typeof n !== "number" || !Number.isFinite(n) || (k === "rear" || k === "height" ? n < .5 || n > 1.5 : n < -3 || n > 3);
  })) return { ...DEFAULT_LISTENING_PROFILE };
  return { rear: s.rear!, height: s.height!, bassDb: s.bassDb!, trebleDb: s.trebleDb! };
}
