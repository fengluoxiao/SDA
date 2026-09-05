import type { VirtualSpeaker } from "@sda/renderer";

export const WIDE_SPEAKERS = ["WideLeft", "WideRight"] as const;
const LABELS: Record<string, readonly [string, string]> = {
  FrontLeft: ["左前", "L"], FrontRight: ["右前", "R"], Center: ["中置", "C"],
  LFE: ["低音炮", "LFE"],
  WideLeft: ["左前宽", "Lw"], WideRight: ["右前宽", "Rw"],
  SurroundLeft: ["左侧环绕", "Ls"], SurroundRight: ["右侧环绕", "Rs"],
  RearLeft: ["左后环绕", "Lrs"], RearRight: ["右后环绕", "Rrs"],
  TopFrontLeft: ["左前顶", "Ltf"], TopFrontRight: ["右前顶", "Rtf"],
  TopMiddleLeft: ["左中顶", "Ltm"], TopMiddleRight: ["右中顶", "Rtm"],
  TopRearLeft: ["左后顶", "Ltr"], TopRearRight: ["右后顶", "Rtr"],
};

export function speakerLabel(name: string): string {
  const label = LABELS[name];
  return label ? `${label[0]} ${label[1]}` : name;
}

export function speakerPosition(speaker: VirtualSpeaker): string {
  if (speaker.isLfe) return "低频效果";
  const az = speaker.azimuth;
  const direction = az === 0 ? "正前方" : `${az > 0 ? "左" : "右"} ${Math.abs(az)}°`;
  return `${direction} · ${speaker.elevation > 0 ? `仰角 ${speaker.elevation}°` : "耳平面"}`;
}
