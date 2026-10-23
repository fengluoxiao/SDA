import React, { memo, useMemo } from "react";
import { Animated, Image, StyleSheet, Text, View } from "react-native";
import { IOSPlaybackSymbol } from "./IOSPlaybackSymbol";
import { formatPlaybackTime } from "./ios-ui-model";
import { MINI_MOTION_SAMPLES, miniMotionPose, progressMotionSegments, type MiniPlayerFrames } from "./mini-player-motion";

type MotionProps = { frames: MiniPlayerFrames; value: Animated.Value; active: boolean; motionId: number; positionMs: number; durationMs: number; home: boolean; targetColor: string; backgroundColor: string };
// Blend once into an opaque stroke; overlapping joins can no longer darken.
export function motionTrackColor(background: string, ink: string) {
  const rgb = (hex: string) => { const value = hex.replace("#", ""); const full = value.length === 3 ? value.split("").map(c => c + c).join("") : value; return [0, 2, 4].map(i => parseInt(full.slice(i, i + 2), 16)); };
  const bg = rgb(background), fg = rgb(ink);
  if (![...bg, ...fg].every(Number.isFinite)) return "#8e8e93";
  return "#" + bg.map((c, i) => Math.round(c * .76 + fg[i]! * .24).toString(16).padStart(2, "0")).join("");
}
const Stroke = memo(function Stroke({ frames, fraction, value, color }: { frames: MiniPlayerFrames; fraction: number; value: Animated.Value; color: string }) {
  const styles = useMemo(() => {
    const parts = progressMotionSegments(frames, fraction);
    return { segments: parts.map(part => ({ transform: [
      { translateX: value.interpolate({ inputRange: MINI_MOTION_SAMPLES, outputRange: part.x }) },
      { translateY: value.interpolate({ inputRange: MINI_MOTION_SAMPLES, outputRange: part.y }) },
      { rotate: value.interpolate({ inputRange: MINI_MOTION_SAMPLES, outputRange: part.angle }) },
      { scaleX: value.interpolate({ inputRange: MINI_MOTION_SAMPLES, outputRange: part.width }) },
      { scaleY: value.interpolate({ inputRange: MINI_MOTION_SAMPLES, outputRange: part.height }) },
    ] })), caps: parts.length ? [parts[0]!, parts[parts.length - 1]!].map((part, i) => ({ transform: [
      { translateX: value.interpolate({ inputRange: MINI_MOTION_SAMPLES, outputRange: i === 0 ? part.ax : part.bx }) },
      { translateY: value.interpolate({ inputRange: MINI_MOTION_SAMPLES, outputRange: i === 0 ? part.ay : part.by }) },
      { scale: value.interpolate({ inputRange: MINI_MOTION_SAMPLES, outputRange: part.height }) },
    ] })) : [] };
  }, [frames, fraction, value]);
  return <View pointerEvents="none" style={StyleSheet.absoluteFill}>
    {styles.segments.map((style, i) => <Animated.View key={i} style={[s.segment, { backgroundColor: color }, style]} />)}
    {styles.caps.map((style, i) => <Animated.View key={"cap-" + i} style={[s.cap, { backgroundColor: color }, style]} />)}
  </View>;
});

// One persistent image is preloaded before hiding either real artwork view.
// Position and scale have separate containers and share the same measured center.
export const SharedMiniPlayerArtwork = memo(function SharedMiniPlayerArtwork({ frames, value, active, home, artworkUri, playing, onReady }: {
  frames: MiniPlayerFrames | null; value: Animated.Value; active: boolean; home: boolean; artworkUri: string; playing: boolean; onReady(uri: string): void;
}) {
  const styles = useMemo(() => {
    const geometry = frames ?? { artwork: { x: 0, y: 0, width: 1, height: 1 }, progress: { x: 0, y: 0, width: 1, height: 1 }, time: { x: 0, y: 0, width: 1, height: 1 }, mini: { x: 0, y: 0, width: 48, height: 54 } };
    const cover = geometry.artwork, poses = MINI_MOTION_SAMPLES.map(t => miniMotionPose(geometry, t));
    const sample = (read: (pose: ReturnType<typeof miniMotionPose>) => number) => value.interpolate({ inputRange: MINI_MOTION_SAMPLES, outputRange: poses.map(read) });
    return { position: { width: cover.width, height: cover.height, transform: [{ translateX: sample(p => p.center.x - cover.width / 2) }, { translateY: sample(p => p.center.y - cover.height / 2) }] },
      image: { left: 0, top: 0, width: cover.width, height: cover.height, borderRadius: sample(p => p.round * Math.min(cover.width, cover.height) / 2),
        transform: [{ scaleX: sample(p => p.width / cover.width) }, { scaleY: sample(p => p.height / cover.height) }] },
      shade: { opacity: sample(p => p.symbolOpacity * .3) },
      symbol: { left: cover.width / 2 - 8, top: cover.height / 2 - 8, opacity: sample(p => p.symbolOpacity) },
    };
  }, [frames, value]);
  return <View testID="shared-mini-player-artwork" pointerEvents="none" accessible={false} accessibilityElementsHidden importantForAccessibility="no-hide-descendants"
    style={[StyleSheet.absoluteFill, { zIndex: home ? 0 : 30, opacity: active ? 1 : 0 }]}>
    <Animated.View style={[s.origin, styles.position]}>
      <Animated.View style={[s.artwork, styles.image]}>
        {!!artworkUri && !!frames && <Image key={artworkUri} source={{ uri: artworkUri }} onLoad={() => onReady(artworkUri)} resizeMode="contain" style={StyleSheet.absoluteFill} />}
        <Animated.View style={[StyleSheet.absoluteFill, { backgroundColor: "#000000" }, styles.shade]} />
      </Animated.View>
      <Animated.View style={[s.symbol, styles.symbol]}><IOSPlaybackSymbol playing={playing} color="#ffffff" size={12} /></Animated.View>
    </Animated.View>
  </View>;
});

export const SharedMiniPlayerTransition = memo(function SharedMiniPlayerTransition(p: MotionProps) {
  // Freeze progress for this short transfer, so playback ticks don't rebuild its native graph.
  const progress = useMemo(() => ({ time: formatPlaybackTime(p.positionMs), ratio: p.durationMs > 0 ? Math.max(0, Math.min(1, p.positionMs / p.durationMs)) : 0 }), [p.motionId]);
  const styles = useMemo(() => {
    const from = p.frames.time, poses = MINI_MOTION_SAMPLES.map(t => miniMotionPose(p.frames, t));
    const sample = (read: (pose: ReturnType<typeof miniMotionPose>) => number) => p.value.interpolate({ inputRange: MINI_MOTION_SAMPLES, outputRange: poses.map(read) });
    return { time: { width: from.width, height: from.height, transform: [{ translateX: sample(pose => pose.timeCenter.x - from.width / 2) },
      { translateY: sample(pose => pose.timeCenter.y - from.height / 2) }, { scale: sample(pose => pose.timeScale) }] } };
  }, [p.frames, p.value]);
  const color = p.home ? "#c9c9ce" : p.targetColor;
  if (!p.active) return null;
  return <View testID="shared-mini-player-transition" pointerEvents="none" accessible={false} accessibilityElementsHidden importantForAccessibility="no-hide-descendants" style={[StyleSheet.absoluteFill, { zIndex: 30 }]}>
    <View style={StyleSheet.absoluteFill}>
      <Stroke frames={p.frames} fraction={1} value={p.value} color={motionTrackColor(p.backgroundColor, p.home ? "#ffffff" : p.targetColor)} />
      <Stroke frames={p.frames} fraction={progress.ratio} value={p.value} color={color} />
      <Animated.View style={[s.origin, styles.time]}><Text numberOfLines={1} style={{ color, fontSize: 11, lineHeight: p.frames.time.height, fontVariant: ["tabular-nums"], textAlign: "center" }}>{progress.time}</Text></Animated.View>
    </View>
  </View>;
});
const s = StyleSheet.create({
  origin: { position: "absolute", left: 0, top: 0 },
  artwork: { position: "absolute", overflow: "hidden" },
  symbol: { position: "absolute", width: 16, height: 16, alignItems: "center", justifyContent: "center" },
  segment: { position: "absolute", left: -.5, top: -.5, width: 1, height: 1 },
  cap: { position: "absolute", left: -.5, top: -.5, width: 1, height: 1, borderRadius: .5 },
});
