import React from "react";
import { Animated, AppState, Easing, StyleSheet, Text, View, type StyleProp, type TextStyle } from "react-native";
import type { LyricLine } from "./mobile-lyrics";
import { karaokeClockNeedsSync, karaokeLayout, karaokeWordRanges, type KaraokeClockAnchor, type KaraokeNativeLine, type KaraokePiece, type KaraokeRange } from "./lyric-karaoke";

type Props = { line: LyricLine; positionMs: number; playing: boolean; active: boolean; style: StyleProp<TextStyle>; testID: string };
/** A single native clock drives every authored word, including overlapping voices. */
function useKaraokeClock(positionMs: number, endMs: number, playing: boolean) {
  const valueRef = React.useRef<Animated.Value | null>(null);
  if (valueRef.current === null) valueRef.current = new Animated.Value(positionMs);
  const value = valueRef.current;
  const anchor = React.useRef<KaraokeClockAnchor | null>(null);
  const animation = React.useRef<Animated.CompositeAnimation | null>(null);
  const [foreground, setForeground] = React.useState(AppState.currentState !== "background" && AppState.currentState !== "inactive");
  React.useEffect(() => {
    const subscription = AppState.addEventListener("change", state => setForeground(state === "active"));
    return () => subscription.remove();
  }, []);
  React.useLayoutEffect(() => {
    const running = playing && foreground && Number.isFinite(endMs) && positionMs < endMs;
    const now = performance.now();
    if (!karaokeClockNeedsSync(anchor.current, positionMs, now, running, endMs)) { anchor.current!.reportMs = positionMs; return; }
    animation.current?.stop(); value.setValue(positionMs);
    anchor.current = { positionMs, reportMs: positionMs, at: now, running, endMs };
    if (running) {
      animation.current = Animated.timing(value, { toValue: endMs, duration: endMs - positionMs, easing: Easing.linear, useNativeDriver: true });
      animation.current.start();
    }
  }, [positionMs, endMs, playing, foreground, value]);
  React.useEffect(() => () => animation.current?.stop(), []);
  return value;
}
/** Two opposite native translations reveal a stationary duplicate of the text. */
function WordFill({ piece, clock, text, style, paragraphWidth }: { piece: KaraokePiece; clock: Animated.Value; text: string; style: StyleProp<TextStyle>; paragraphWidth: number }) {
  const [reveal, compensate] = React.useMemo(() => {
    const inputRange = [piece.startMs, Math.max(piece.startMs + .01, piece.endMs)];
    return [
      clock.interpolate({ inputRange, outputRange: [-piece.width, 0], extrapolate: "clamp" }),
      clock.interpolate({ inputRange, outputRange: [piece.width, 0], extrapolate: "clamp" }),
    ];
  }, [clock, piece.startMs, piece.endMs, piece.width]);
  const glow = React.useMemo(() => piece.hold ? clock.interpolate({ ...piece.hold, extrapolate: "clamp" }) : null, [clock, piece.hold]);
  const glowRadius = (StyleSheet.flatten(style)?.fontSize ?? 28) * .25;
  return <View testID="lyric-word-fill" pointerEvents="none" style={{ position: "absolute", left: piece.x, top: piece.y, width: piece.width, height: piece.height, overflow: "hidden" }}>
    <Animated.View style={{ width: piece.width, height: piece.height, overflow: "hidden", transform: [{ translateX: reveal }] }}>
      <Animated.View style={{ width: piece.width, height: piece.height, transform: [{ translateX: compensate }] }}>
        <Text style={[style, { position: "absolute", left: -piece.x, top: -piece.y, width: paragraphWidth, color: "#ffffff" }]}>{text}</Text>
        {glow && <Animated.Text testID="lyric-held-note-glow" style={[style, { position: "absolute", left: -piece.x, top: -piece.y, width: paragraphWidth,
          color: "#ffffff", opacity: glow, textShadowColor: "rgba(255,255,255,.9)", textShadowOffset: { width: 0, height: 0 }, textShadowRadius: glowRadius }]}>{text}</Animated.Text>}
      </Animated.View>
    </Animated.View>
  </View>;
}
function ActiveKaraokeText({ line, positionMs, playing, style, testID, ranges }: Props & { ranges: KaraokeRange[] }) {
  const [width, setWidth] = React.useState(0);
  const [lines, setLines] = React.useState<KaraokeNativeLine[]>([]);
  const layoutKey = JSON.stringify([line.text, width, StyleSheet.flatten(style), lines]);
  const [measured, setMeasured] = React.useState<{ key: string; widths: Record<string, number> }>({ key: "", widths: {} });
  const liveKey = React.useRef(layoutKey); liveKey.current = layoutKey;
  const layout = React.useMemo(() => karaokeLayout(line.text, ranges, lines, measured.key === layoutKey ? measured.widths : {}), [line.text, ranges, lines, measured, layoutKey]);
  const endMs = ranges.reduce((end, word) => Math.max(end, word.endMs), Number.isFinite(line.endMs) ? line.endMs : positionMs);
  const clock = useKaraokeClock(positionMs, endMs, playing);
  return <View onLayout={event => { const next = event.nativeEvent.layout.width; setWidth(old => Math.abs(old - next) < .25 ? old : next); }}>
    <Text testID={testID} style={[style, { color: "rgba(255,255,255,.5)" }]} onTextLayout={event => {
      const next = event.nativeEvent.lines.map(({ text, x, y, width, height }) => ({ text, x, y, width, height }));
      setLines(old => JSON.stringify(old) === JSON.stringify(next) ? old : next);
    }}>{line.text}</Text>
    <View pointerEvents="none" accessible={false} accessibilityElementsHidden importantForAccessibility="no-hide-descendants" style={StyleSheet.absoluteFill}>
      {layout.requests.map(request => <Text key={`${layoutKey}:${request.id}`} numberOfLines={1}
        style={[style, { position: "absolute", left: 0, top: 0, width: 10000, opacity: 0, textAlign: "left" }]} onTextLayout={event => {
          if (liveKey.current !== layoutKey) return;
          const advance = event.nativeEvent.lines[0]?.width;
          if (!Number.isFinite(advance) || advance === undefined || advance < 0) return;
          setMeasured(old => ({ key: layoutKey, widths: { ...(old.key === layoutKey ? old.widths : {}), [request.id]: advance } }));
        }}>{request.text}</Text>)}
      {width > 0 && layout.pieces.map(piece => <WordFill key={`${layoutKey}:${piece.id}`} piece={piece} clock={clock} text={line.text} style={style} paragraphWidth={width} />)}
    </View>
  </View>;
}
export function KaraokeText(props: Props) {
  const ranges = React.useMemo(() => karaokeWordRanges(props.line), [props.line]);
  if (!props.active || !ranges.length) return <Text testID={props.testID} style={props.style}>{props.line.text}</Text>;
  return <ActiveKaraokeText key={`${props.line.id}:${props.line.text}`} {...props} ranges={ranges} />;
}
