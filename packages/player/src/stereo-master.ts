/** Classify decoded PCM, never the container's (possibly stereo-core) header. */
export function isStereoMasterFrame(frame: {
  codec: string; channels: readonly unknown[]; labels: readonly string[];
  objectChannels: readonly unknown[]; events: readonly unknown[];
}): boolean {
  return frame.codec !== "adm" && frame.channels.length === 2 && frame.labels.length === 2
    && frame.objectChannels.length === 0 && frame.events.length === 0
    && ["L", "Left", "FrontLeft"].includes(frame.labels[0]!)
    && ["R", "Right", "FrontRight"].includes(frame.labels[1]!);
}

/** Audio Vivid's verified speaker order; never apply channel-bed loudness to objects. */
export const AV3A_714_WEIGHTS = [1, 1, 1, 0, 1.41, 1.41, 1.41, 1.41, 1, 1, 1, 1] as const;
export function isAv3aBedFrame(frame: {
  codec: string; channels: readonly unknown[]; labels: readonly string[];
  objectChannels: readonly unknown[]; events: readonly unknown[];
}): boolean {
  const labels = ["L","R","C","LFE","Lb","Rb","Ls","Rs","Tfl","Tfr","Tbl","Tbr"];
  return frame.codec === "av3a" && frame.channels.length === 12 && frame.labels.length === 12
    && frame.objectChannels.length === 0 && frame.events.length === 0
    && frame.labels.every((label, i) => label === labels[i]);
}
