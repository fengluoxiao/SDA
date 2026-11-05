import type { ObjectEvent } from "@sda/core";
export interface NativeFileSnapshot {
  session: string; epoch?: number; codec: string; container?: string; sampleRate: number; durationMs: number; positionMs: number; decodedMs: number;
  paused: boolean; preparing: boolean; ended: boolean; channels: number; bedLabels: string[]; objectChannels: number;
  objects: Record<string, ObjectEvent>; soundingObjectIds: number[];
  metadata?: { binaural?: import("@sda/demux").BinauralRenderMetadata; title?: string; artist?: string; album?: string; coverArt?: { bytes: Uint8Array; mimeType: "image/jpeg" | "image/png" } };
  authored?: Record<string, unknown>;
}
export interface NativeFileEvent { kind: "state" | "error"; session: string; snapshot?: NativeFileSnapshot; error?: string; }
export interface NativeFileBridge {
  command(command: Record<string, unknown>): Promise<NativeFileSnapshot>;
  onState(callback: (event: NativeFileEvent) => void): () => void;
}
