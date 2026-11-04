import React from "react";
import { ActivityIndicator, Animated, Image, Pressable, ScrollView, StyleSheet, Text, View, useWindowDimensions, type ViewProps } from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";
import type { PlayerProps } from "./RemotePlayer";
import { lyricPresentationDocument, lyricSpeakerAlignments, type LyricLine } from "./mobile-lyrics";
import { useTrackLyrics } from "./use-track-lyrics";
import { useLyricsSwipe } from "./use-lyrics-swipe";
import { useLyricsFollow } from "./use-lyrics-follow";

const LyricsSwipeLock = React.createContext<((blocked: boolean) => void) | null>(null);
/** Same native View and geometry, but slider touches cannot become a page swipe. */
export function LyricsGestureBoundary({ children, ...props }: ViewProps) {
  const lock = React.useContext(LyricsSwipeLock);
  return <View {...props} onTouchStart={event => { lock?.(true); props.onTouchStart?.(event); }}
    onTouchEnd={event => { if (!event.nativeEvent.touches.length) lock?.(false); props.onTouchEnd?.(event); }}
    onTouchCancel={event => { lock?.(false); props.onTouchCancel?.(event); }}>{children}</View>;
}

/** Metadata shares the lyric page backdrop; no separate rectangular panel. */
export function LyricsTrackHeader({ player: p }: { player: PlayerProps }) {
  const uri = p.metadata.coverUri || "", [failedUri, setFailedUri] = React.useState("");
  return <View testID="lyrics-track-header" pointerEvents="none" style={styles.trackHeader}>
    {uri && failedUri !== uri ? <Image testID="lyrics-track-artwork" source={{ uri }} resizeMode="cover" onError={() => setFailedUri(uri)} style={styles.trackArtwork} /> : <View style={[styles.trackArtwork, styles.trackPlaceholder]}><Text style={{ color: "rgba(255,255,255,.65)", fontSize: 24 }}>♪</Text></View>}
    <View style={{ flex: 1, minWidth: 0 }}>
      <Text testID="lyrics-track-title" numberOfLines={1} style={styles.trackTitle}>{p.metadata.title || p.fileName || "未知歌曲"}</Text>
      <Text testID="lyrics-track-artist" numberOfLines={1} style={styles.trackArtist}>{p.metadata.artist || "未知歌手"}</Text>
    </View>
  </View>;
}

/** A single full-page cover backdrop, released after the exit fade completes. */
export function LyricsBackdrop({ visible, artworkUri, backgroundColor }: { visible: boolean; artworkUri: string; backgroundColor: string }) {
  const window = useWindowDimensions();
  // Rotation can momentarily leave the native screen at its previous size.
  // Keep one orientation-independent image large enough to cover either axis,
  // even before the parent has delivered its new bounds. Never crop to them.
  const extent = Math.ceil(Math.max(window.width, window.height) * 2 + 64);
  const backdropFrame = { position: "absolute" as const, left: "50%" as const, top: "50%" as const, width: extent, height: extent, marginLeft: -extent / 2, marginTop: -extent / 2, backgroundColor };
  const opacity = React.useRef(new Animated.Value(visible ? 1 : 0)).current;
  const [mounted, setMounted] = React.useState(visible);
  React.useEffect(() => {
    if (visible) setMounted(true);
    const animation = Animated.timing(opacity, { toValue: visible ? 1 : 0, duration: 240, useNativeDriver: true });
    animation.start(({ finished }) => { if (finished && !visible) setMounted(false); });
    return () => animation.stop();
  }, [visible, opacity]);
  if (!mounted && !visible) return null;
  return <Animated.View testID="lyrics-album-backdrop" pointerEvents="none" accessible={false} accessibilityElementsHidden importantForAccessibility="no-hide-descendants"
    style={[StyleSheet.absoluteFill, { zIndex: -1, overflow: "visible", opacity }]}>
    <View testID="lyrics-rotation-background" style={backdropFrame}>
      {!!artworkUri && <Image testID="lyrics-blurred-artwork" source={{ uri: artworkUri }} resizeMode="cover" blurRadius={64}
        style={[StyleSheet.absoluteFill, { opacity: .65 }]} />}
      <View style={[StyleSheet.absoluteFill, { backgroundColor: "rgba(0,0,0,.25)" }]} />
    </View>
  </Animated.View>;
}

function LyricsPanel({ player: p, active, compact = false }: { player: PlayerProps; active: boolean; compact?: boolean }) {
  const lyrics = useTrackLyrics({ uri: p.selectedUri, metadataLyrics: p.metadata.lyrics, active, attach: p.attachLyrics });
  const document = React.useMemo(() => lyrics.document ? lyricPresentationDocument(lyrics.document) : null, [lyrics.document]);
  const speakerAlignments = React.useMemo(() => document ? lyricSpeakerAlignments(document) : new Map(), [document]);
  const follow = useLyricsFollow({ active, document, positionMs: p.positionMs });
  const disabled = p.busy || p.preparingAudio === true;
  const seek = (line: LyricLine) => {
    if (disabled || !document?.timed || !p.seekTo) return;
    follow.resetManual();
    p.seekTo(Math.max(0, Math.min(line.startMs, Math.max(0, p.durationMs - 1))));
  };
  const renderWords = (line: LyricLine, sounding: boolean) => {
    const wordsMatch = line.words.length > 0 && line.words.map(word => word.text).join("").replace(/\s+/g, " ").trim() === line.text.replace(/\s+/g, " ").trim();
    return sounding && wordsMatch ? line.words.map((word, n) => <Text key={n} style={{ color: p.positionMs >= word.startMs ? "#ffffff" : "rgba(255,255,255,.5)" }}>{word.text}</Text>) : line.text || "♪";
  };
  const clock = (ms: number) => `${Math.floor(ms / 60000)}:${String(Math.floor(ms / 1000) % 60).padStart(2, "0")}`;
  return <View testID="portrait-lyrics-page" style={styles.panel}>
    {lyrics.loading ? <View style={styles.empty}><ActivityIndicator color="#ffffff" /></View> : !document?.lines.length ? <View style={styles.empty}><Text style={styles.emptyTitle}>暂无歌词</Text></View> : <ScrollView ref={follow.scrollRef} testID="lyrics-scroll" style={{ flex: 1 }} contentContainerStyle={{ paddingTop: compact ? 0 : 12, paddingBottom: follow.viewportHeight * .6 }} showsVerticalScrollIndicator={false} contentInsetAdjustmentBehavior="never" automaticallyAdjustContentInsets={false} directionalLockEnabled nestedScrollEnabled
      onLayout={e => follow.onViewportLayout(e.nativeEvent.layout.height)} onContentSizeChange={follow.onContentSizeChange} {...follow.scrollHandlers}>
      {document.lines.map((line, i) => {
        const sounding = document.timed && p.positionMs >= line.startMs && p.positionMs < line.endMs;
        const tappable = document.timed && !!p.seekTo && !disabled && p.durationMs > 0;
        const alignment = { textAlign: line.agentIds?.length === 1 ? speakerAlignments.get(line.agentIds[0]!) ?? "left" : "left" };
        const supporting = line.vocal === "background" || line.vocal === "chorus";
        return <Pressable key={line.id} testID={`lyric-line-${i}`} accessibilityRole={tappable ? "button" : "text"} accessibilityLabel={line.text ? `${line.text}${tappable ? `，跳转到 ${clock(line.startMs)}` : ""}` : "间奏"} accessibilityState={{ disabled: !tappable, selected: sounding }} disabled={!tappable} onPress={() => seek(line)} onLayout={e => follow.onLineLayout(i, e.nativeEvent.layout.y)} style={({ pressed }) => [styles.line, compact && styles.compactLine, compact && i === 0 && { paddingTop: 0 }, { opacity: pressed ? .55 : 1 }]}>
          <Text testID={`lyric-text-${i}`} style={[supporting ? styles.backingText : styles.lineText, alignment, { color: sounding || !document.timed ? "#ffffff" : "rgba(255,255,255,.38)" }]}>
            {renderWords(line, sounding)}
          </Text>
          {!!line.translation && <Text testID={`lyric-translation-${i}`} style={[styles.translation, supporting && styles.backingTranslation, alignment, { color: sounding ? "rgba(255,255,255,.8)" : "rgba(255,255,255,.35)" }]}>{line.translation}</Text>}
          {line.background?.map((backing, n) => {
            const singing = p.positionMs >= backing.startMs && p.positionMs < backing.endMs;
            return <Pressable key={backing.id || n} testID={`lyric-backing-${i}-${n}`} accessibilityRole={tappable ? "button" : "text"} accessibilityLabel={`${backing.text}${tappable ? `，跳转到 ${clock(backing.startMs)}` : ""}`} accessibilityState={{ disabled: !tappable, selected: singing }} disabled={!tappable}
              onPress={event => { event.stopPropagation(); seek(backing); }} style={{ paddingTop: 10, minHeight: 44 }}>
              <Text testID={`lyric-backing-text-${i}-${n}`} style={[styles.backingText, alignment, { color: singing ? "#ffffff" : "rgba(255,255,255,.38)" }]}>{renderWords(backing, singing)}</Text>
              {!!backing.translation && <Text style={[styles.translation, styles.backingTranslation, alignment, { color: singing ? "rgba(255,255,255,.8)" : "rgba(255,255,255,.35)" }]}>{backing.translation}</Text>}
            </Pressable>;
          })}
        </Pressable>;
      })}
    </ScrollView>}
  </View>;
}
/** Playback is on the left; swipe left to reveal lyrics, right to return. */
export function PortraitLyricsPager({ player, enabled, visible, children, playbackHeader, lyricsSelected, onLyricsPageChange, headerHeight = 0, preservePlaybackBottomInset = false }: { player: PlayerProps; enabled: boolean; visible: boolean; children: React.ReactNode; playbackHeader?: React.ReactNode; lyricsSelected?: boolean; onLyricsPageChange?(selected: boolean): void; headerHeight?: number; preservePlaybackBottomInset?: boolean }) {
  const window = useWindowDimensions(), [width, setWidth] = React.useState(window.width);
  const swipe = useLyricsSwipe({ width, enabled, visible, selectedPage: lyricsSelected === undefined ? undefined : lyricsSelected ? 1 : 0, onPageChange: next => onLyricsPageChange?.(next === 1) });
  const headerInset = Number.isFinite(headerHeight) ? Math.max(0, headerHeight) : 0;
  // The compact metadata row is 46pt tall with 8pt top padding. Reserve only
  // its visible bottom plus 6pt, rather than the taller playback header box.
  const lyricsInset = Math.min(headerInset, (headerInset + 46 + 8) / 2 + 6);
  React.useEffect(() => { if (__DEV__) console.info("[SDA UI] lyrics gesture: corrected column width and rotation artwork v7"); }, []);
  if (!enabled) return preservePlaybackBottomInset ? <SafeAreaView edges={["bottom"]} style={{ flex: 1 }}>{children}</SafeAreaView> : <>{children}</>;
  const PlaybackView = preservePlaybackBottomInset ? SafeAreaView : View;
  return <LyricsSwipeLock.Provider value={swipe.setBlocked}>
    <View testID="portrait-lyrics-pager" {...swipe.panHandlers} onTouchStartCapture={() => swipe.setBlocked(false)}
      onLayout={event => { const next = event.nativeEvent.layout.width; if (next > 0) setWidth(next); }}
      style={{ flex: 1, display: visible ? "flex" : "none", overflow: "hidden" }}>
      <Animated.View style={{ width: width * 2, flex: 1, flexDirection: "row", transform: [{ translateX: swipe.translation }] }}>
        <PlaybackView testID="album-page-safe-area" edges={preservePlaybackBottomInset ? ["bottom"] : undefined} accessibilityElementsHidden={swipe.page !== 0} importantForAccessibility={swipe.page === 0 ? "auto" : "no-hide-descendants"} style={{ width, flexShrink: 0, height: "100%", paddingTop: headerInset }}>
          <View testID="album-page-header" style={{ position: "absolute", top: 0, left: 0, right: 0, height: headerInset }}>{playbackHeader}</View>
          {children}
        </PlaybackView>
        <View accessibilityElementsHidden={swipe.page !== 1} importantForAccessibility={swipe.page === 1 ? "auto" : "no-hide-descendants"} style={{ width, flexShrink: 0, height: "100%", paddingTop: lyricsInset }}>
          <View testID="lyrics-page-header" style={{ position: "absolute", top: 0, left: 0, right: 0, height: headerInset }}><LyricsTrackHeader player={player} /></View>
          <LyricsPanel key={player.selectedUri} player={player} active={enabled && visible && swipe.page === 1} />
        </View>
      </Animated.View>
    </View>
  </LyricsSwipeLock.Provider>;
}
/** Only the landscape controls column participates; artwork is a sibling outside. */
export function LandscapeLyricsPager({ player, enabled, visible, lyricsSelected, onLyricsPageChange, style, children, topClearance = 0, columnRef, onGeometryChange, onLyricsPageSettled }: {
  player: PlayerProps; enabled: boolean; visible: boolean; lyricsSelected: boolean; onLyricsPageChange(selected: boolean): void;
  style?: ViewProps["style"]; children: React.ReactNode; topClearance?: number; columnRef?: React.Ref<React.ComponentRef<typeof View>>; onGeometryChange?(): void; onLyricsPageSettled?(): void;
}) {
  const window = useWindowDimensions(), [width, setWidth] = React.useState(window.width / 2);
  const swipe = useLyricsSwipe({ width, enabled, visible, selectedPage: lyricsSelected ? 1 : 0, onPageChange: next => onLyricsPageChange(next === 1), onPageSettled: onLyricsPageSettled });
  if (!enabled) return <View ref={columnRef} onLayout={onGeometryChange} style={style}>{children}</View>;
  const originalTop = StyleSheet.flatten(style)?.paddingTop;
  const controlsTop = typeof originalTop === "number" ? originalTop : 0;
  const headroom = Number.isFinite(topClearance) ? Math.max(0, topClearance) : 0;
  return <LyricsSwipeLock.Provider value={swipe.setBlocked}>
    <View ref={columnRef} onLayout={onGeometryChange} testID="landscape-controls-lyrics" style={[style, { height: "100%", minHeight: 0, minWidth: 0, paddingTop: 0 }]}>
      <View testID="landscape-lyrics-viewport" {...swipe.panHandlers} onTouchStartCapture={() => swipe.setBlocked(false)}
        onLayout={event => { const next = event.nativeEvent.layout.width; if (next > 0) setWidth(next); }}
        style={{ flex: 1, marginTop: -headroom, minHeight: 0, minWidth: 0, overflow: "hidden" }}>
        <Animated.View style={{ width: width * 2, flex: 1, flexDirection: "row", transform: [{ translateX: swipe.translation }] }}>
          <View testID="landscape-transport-page" accessibilityElementsHidden={swipe.page !== 0} importantForAccessibility={swipe.page === 0 ? "auto" : "no-hide-descendants"}
            style={{ width, height: "100%", flexShrink: 0, justifyContent: "center", paddingTop: controlsTop + headroom }}>{children}</View>
          <View testID="landscape-lyrics-page" accessibilityElementsHidden={swipe.page !== 1} importantForAccessibility={swipe.page === 1 ? "auto" : "no-hide-descendants"}
            style={{ width, height: "100%", flexShrink: 0 }}>
            <LyricsPanel key={player.selectedUri} player={player} active={visible && swipe.page === 1} compact />
          </View>
        </Animated.View>
      </View>
    </View>
  </LyricsSwipeLock.Provider>;
}
const styles = StyleSheet.create({
  panel: { flex: 1, paddingHorizontal: 6 },
  trackHeader: { position: "absolute", top: 0, bottom: 0, left: 0, right: 0, flexDirection: "row", alignItems: "center", gap: 12, paddingTop: 8, paddingHorizontal: 6, backgroundColor: "transparent" },
  trackArtwork: { width: 46, height: 46, borderRadius: 8 }, trackPlaceholder: { backgroundColor: "rgba(255,255,255,.08)", alignItems: "center", justifyContent: "center" },
  trackTitle: { color: "#ffffff", fontSize: 17, lineHeight: 23, fontWeight: "600", textAlign: "left" },
  trackArtist: { color: "rgba(255,255,255,.66)", fontSize: 14, lineHeight: 20, marginTop: 2, textAlign: "left" },
  empty: { flex: 1, justifyContent: "center", alignItems: "flex-start" }, emptyTitle: { color: "rgba(255,255,255,.6)", fontSize: 25, fontWeight: "600", textAlign: "left" },
  line: { paddingVertical: 13, minHeight: 58, alignSelf: "stretch" },
  compactLine: { paddingVertical: 8, minHeight: 56 },
  lineText: { fontSize: 28, lineHeight: 40, fontWeight: "700", letterSpacing: .1, textAlign: "left" },
  backingText: { fontSize: 20, lineHeight: 29, fontWeight: "600", textAlign: "left" },
  backingTranslation: { fontSize: 15, lineHeight: 22 },
  translation: { fontSize: 17, lineHeight: 26, marginTop: 5, textAlign: "left" },
});
