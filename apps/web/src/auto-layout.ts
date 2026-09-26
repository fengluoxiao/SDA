import {detectLayoutId, type LayoutId} from "@sda/renderer";

/** MPEG-H 360 Reality Audio has an authored below-ear layer. */
export function uses360RaLowerLayer(codec?: string): boolean {
  return ["mpegh", "mha1", "mhm1"].includes(codec ?? "");
}

export function formatAutoLayout(codec?: string): LayoutId | undefined {
  if (uses360RaLowerLayer(codec)) return "360RA-13";
  if (["truehd", "eac3", "ac4", "iamf"].includes(codec ?? "")) return "7.1.4";
  return undefined;
}
/** ALAC upmix is an opt-in listening transform, not a container/codec claim. */
export function resolveAutoLayout(labels: readonly string[], hasDynamics: boolean, codec?: string, alacStereoUpmix = false): LayoutId {
  if (alacStereoUpmix && codec === "alac" && !hasDynamics
      && labels.length === 2 && labels[0] === "L" && labels[1] === "R") return "7.1.4";
  return formatAutoLayout(codec) ?? detectLayoutId(labels, hasDynamics);
}
