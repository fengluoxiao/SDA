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
