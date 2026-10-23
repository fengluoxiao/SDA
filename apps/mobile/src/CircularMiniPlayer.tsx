import React, { memo, useState } from "react";
import { Image, Pressable, StyleSheet, Text, View } from "react-native";
import { IOSPlaybackSymbol } from "./IOSPlaybackSymbol";
import { formatPlaybackTime } from "./ios-ui-model";

export const CIRCULAR_MINI_PLAYER_LAYOUT = { face: 48, height: 54 } as const;

// Two clipped native semicircles form a smooth arc without a canvas or another
// rendering engine. Rotating the whole ring leaves the elapsed-time gap below.
function ProgressArc({ degrees, color }: { degrees: number; color: string }) {
  if (degrees <= 0) return null;
  const first = Math.min(180, degrees);
  const second = Math.max(0, degrees - 180);
  const radians = degrees * Math.PI / 180;
  const stroke = { borderTopColor: color, borderRightColor: color };
  return <View style={s.arc}>
    <View style={[s.half, { left: 24 }]}><View style={[s.stroke, stroke, { left: -24, transform: [{ rotate: (first - 135) + "deg" }] }]} /></View>
    {second > 0 && <View style={s.half}><View style={[s.stroke, stroke, { transform: [{ rotate: (second + 45) + "deg" }] }]} /></View>}
    <View style={[s.cap, { left: 23, top: 0, backgroundColor: color }]} />
    <View style={[s.cap, { left: 23 + 23 * Math.sin(radians), top: 23 - 23 * Math.cos(radians), backgroundColor: color }]} />
  </View>;
}

type Props = {
  title: string; artist: string; artworkUri: string; color: string;
  positionMs: number; durationMs: number; playing: boolean; disabled: boolean;
  onTogglePlayback(): void; onOpenPlayer(): void;
};

export const CircularMiniPlayer = memo(function CircularMiniPlayer(p: Props) {
  const [failedArtwork, setFailedArtwork] = useState("");
  const duration = Number.isFinite(p.durationMs) ? Math.max(0, p.durationMs) : 0;
  const elapsed = Number.isFinite(p.positionMs) ? Math.max(0, p.positionMs) : 0;
  const progress = duration > 0 ? Math.min(1, elapsed / duration) : 0;
  const hasArtwork = !!p.artworkUri && p.artworkUri !== failedArtwork;
  return <Pressable testID="circular-mini-player" accessibilityRole="button"
    accessibilityLabel={(p.playing ? "暂停" : "播放") + "：" + p.title + "，" + p.artist}
    accessibilityHint="轻点暂停或继续，长按打开正在播放"
    accessibilityState={{ disabled: p.disabled }}
    accessibilityValue={{ text: formatPlaybackTime(elapsed) + " / " + (duration > 0 ? formatPlaybackTime(duration) : "未知时长") }}
    disabled={p.disabled} onPress={p.onTogglePlayback} onLongPress={p.onOpenPlayer}
    delayLongPress={400} style={({ pressed }) => [s.control, { opacity: p.disabled ? .4 : pressed ? .7 : 1, transform: [{ scale: pressed ? .94 : 1 }] }]}>
    <View pointerEvents="none" accessible={false} accessibilityElementsHidden importantForAccessibility="no-hide-descendants" style={s.face}>
      <View style={[StyleSheet.absoluteFill, { opacity: .24 }]}><ProgressArc degrees={270} color={p.color} /></View>
      <ProgressArc degrees={270 * progress} color={p.color} />
      <View style={s.artwork}>
        {hasArtwork && <Image key={p.artworkUri} source={{ uri: p.artworkUri }} resizeMode="cover" onError={() => setFailedArtwork(p.artworkUri)} style={StyleSheet.absoluteFill} />}
        <View style={[s.playback, { backgroundColor: hasArtwork ? "rgba(0,0,0,.3)" : "transparent" }]}>
          <IOSPlaybackSymbol playing={p.playing} color={hasArtwork ? "#ffffff" : p.color} size={12} />
        </View>
      </View>
      <Text numberOfLines={1} adjustsFontSizeToFit minimumFontScale={.75} maxFontSizeMultiplier={1.2}
        style={[s.elapsed, { color: p.color }]}>{formatPlaybackTime(elapsed)}</Text>
    </View>
  </Pressable>;
});

const s = StyleSheet.create({
  control: { width: CIRCULAR_MINI_PLAYER_LAYOUT.face, height: CIRCULAR_MINI_PLAYER_LAYOUT.height, alignItems: "center", justifyContent: "flex-start", flexShrink: 0 },
  face: { width: CIRCULAR_MINI_PLAYER_LAYOUT.face, height: CIRCULAR_MINI_PLAYER_LAYOUT.face },
  arc: { position: "absolute", width: 48, height: 48, transform: [{ rotate: "225deg" }] },
  half: { position: "absolute", top: 0, left: 0, width: 24, height: 48, overflow: "hidden" },
  stroke: { position: "absolute", top: 0, left: 0, width: 48, height: 48, borderRadius: 24, borderWidth: 2, borderColor: "transparent" },
  cap: { position: "absolute", width: 2, height: 2, borderRadius: 1 },
  artwork: { position: "absolute", left: 7, top: 7, width: 34, height: 34, borderRadius: 17, overflow: "hidden" },
  playback: { position: "absolute", top: 0, right: 0, bottom: 0, left: 0, alignItems: "center", justifyContent: "center" },
  elapsed: { position: "absolute", bottom: -6, left: 7, right: 7, fontSize: 9, lineHeight: 11, fontWeight: "600", fontVariant: ["tabular-nums"], textAlign: "center" },
});
