import React from "react";
import { StyleSheet, Text, View } from "react-native";
import type { QueueTrack } from "./RemotePlayer";

/** Remaining disabled region retreats left-to-right with actual import work. */
export function QueuePreparationOverlay({ preparation, dark, accent }: { preparation: QueueTrack["preparation"]; dark: boolean; accent: string }) {
  if (!preparation) return null;
  const progress = Math.max(0, Math.min(1, preparation.progress));
  const failed = preparation.state === "failed";
  return <View pointerEvents="none" style={StyleSheet.absoluteFill}>
    <View style={[StyleSheet.absoluteFill, { left: `${progress * 100}%`, backgroundColor: dark ? "rgba(18,20,24,.72)" : "rgba(215,218,223,.80)" }]} />
    <View style={{ position: "absolute", bottom: 1, left: 0, width: `${progress * 100}%`, height: 2, backgroundColor: failed ? "#d34b4b" : accent }} />
    <Text numberOfLines={1} style={{ position: "absolute", right: 8, bottom: 5, maxWidth: "90%", fontSize: 10,
      color: dark ? "#fff" : "#242730", backgroundColor: dark ? "rgba(20,22,26,.85)" : "rgba(245,246,248,.94)", borderRadius: 4, paddingHorizontal: 5, paddingVertical: 2 }}>
      {failed ? `准备失败 · ${preparation.error || "请重新导入"}` : preparation.stage}
    </Text>
  </View>;
}
