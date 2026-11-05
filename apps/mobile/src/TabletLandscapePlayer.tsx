import React from "react";
import { Pressable, StyleSheet, View } from "react-native";
import type { PlayerProps } from "./RemotePlayer";
import { LyricsPanel } from "./PortraitLyricsPager";

/** iPad and Android tablet landscape share the album/controls + lyrics split. */
export function TabletLandscapePlayer({ player, visible, children, columnRef, onGeometryChange, albumWidth, controlsWidth, horizontalPadding, verticalPadding, coverLabel, coverPlaceholder, navigationSide, rowRef, lyricsRef, onLyricsGeometryChange, topClearance = 0 }: {
  player: PlayerProps; visible: boolean; children: React.ReactNode;
  columnRef: React.Ref<React.ComponentRef<typeof View>>; onGeometryChange(): void;
  rowRef?: React.Ref<React.ComponentRef<typeof View>>; lyricsRef?: React.Ref<React.ComponentRef<typeof View>>; onLyricsGeometryChange?(): void; topClearance?: number;
  albumWidth: number; controlsWidth: number; horizontalPadding: number; verticalPadding: number; coverLabel: string; coverPlaceholder?: React.ReactNode; navigationSide?: "left" | "right";
}) {
  const headroom = Number.isFinite(topClearance) ? Math.max(0, topClearance) : 0;
  return <View ref={rowRef} onLayout={onLyricsGeometryChange} testID="tablet-landscape-player" style={styles.row}>
    {/* Sharp artwork stays in the root plane behind this column and the header.
        The flexible cover area leaves the transport anchored in the lower half. */}
    <View ref={columnRef} collapsable={false} onLayout={onGeometryChange} testID="tablet-landscape-album" style={[styles.column, { width: albumWidth, flexShrink: 0 }]}>
      <Pressable testID="tablet-landscape-cover" accessibilityRole="button" accessibilityLabel={coverLabel}
        onPress={player.chooseFile} disabled={player.busy} style={styles.coverSpace}>{coverPlaceholder}</Pressable>
      <View testID="tablet-landscape-transport" style={{ width: controlsWidth, alignSelf: "center", flexShrink: 0, paddingBottom: verticalPadding }}>
        {children}
      </View>
    </View>
    <View ref={lyricsRef} onLayout={onLyricsGeometryChange} testID="tablet-landscape-lyrics" style={[styles.column, { flex: 1, paddingLeft: horizontalPadding, paddingRight: navigationSide === "right" ? Math.max(80, horizontalPadding) : horizontalPadding, paddingTop: 0, paddingBottom: verticalPadding }]}>
      <View testID="tablet-landscape-lyrics-viewport" style={{ flex: 1, marginTop: -headroom, minHeight: 0, minWidth: 0, overflow: "hidden" }}>
        <LyricsPanel key={player.selectedUri} player={player} active={visible} compact />
      </View>
    </View>
  </View>;
}

const styles = StyleSheet.create({
  row: { flex: 1, minHeight: 0, flexDirection: "row" },
  column: { minWidth: 0, minHeight: 0 },
  coverSpace: { flex: 1, minHeight: 0, alignItems: "center", justifyContent: "center" },
});
