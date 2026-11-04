import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ScrollView } from "react-native";
import type * as React from "react";
import { lyricFollowIndex, type LyricDocument } from "./mobile-lyrics";

export const LYRICS_FOLLOW_DELAY = 4000;
/** Follow the live playback clock; manual scrolling pauses follow until input settles. */
export function useLyricsFollow(options: { active: boolean; document: LyricDocument | null; positionMs: number }) {
  const scrollRef = useRef<React.ComponentRef<typeof ScrollView>>(null);
  const positions = useRef(new Map<number, number>()), previousLine = useRef(-2);
  const [viewportHeight, setViewportHeight] = useState(400);
  const index = options.document ? lyricFollowIndex(options.document, options.positionMs) : -1;
  const latest = useRef({ ...options, index, viewportHeight }); latest.current = { ...options, index, viewportHeight };
  const manual = useRef({ session: false, dragging: false, momentum: false });
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const prior = useRef<{ active: boolean; document: LyricDocument | null; height: number }>({ active: false, document: null, height: viewportHeight });
  const clearTimer = useCallback(() => { if (timer.current !== null) clearTimeout(timer.current); timer.current = null; }, []);
  const resetManual = useCallback(() => {
    clearTimer(); manual.current = { session: false, dragging: false, momentum: false }; previousLine.current = -2;
  }, [clearTimer]);
  const follow = useCallback((force = false, animated = true) => {
    const p = latest.current;
    if (!p.active || !p.document?.timed || p.index < 0 || manual.current.session && !force) return;
    const y = positions.current.get(p.index); if (y === undefined || !scrollRef.current) return;
    if (!force && previousLine.current === p.index) return;
    previousLine.current = p.index;
    scrollRef.current.scrollTo({ y: Math.max(0, y - p.viewportHeight * .34), animated });
  }, []);
  const scheduleReturn = useCallback(() => {
    clearTimer();
    if (!latest.current.active || !manual.current.session || manual.current.dragging || manual.current.momentum) return;
    timer.current = setTimeout(() => {
      timer.current = null;
      if (!latest.current.active || manual.current.dragging || manual.current.momentum) return;
      manual.current.session = false; previousLine.current = -2;
      follow(true, true);
    }, LYRICS_FOLLOW_DELAY);
  }, [clearTimer, follow]);
  const onScrollBeginDrag = useCallback(() => {
    clearTimer(); manual.current = { session: true, dragging: true, momentum: false };
  }, [clearTimer]);
  const onScrollEndDrag = useCallback(() => { manual.current.dragging = false; scheduleReturn(); }, [scheduleReturn]);
  const onMomentumScrollBegin = useCallback(() => {
    if (!manual.current.session) return;
    manual.current.momentum = true; clearTimer();
  }, [clearTimer]);
  const onMomentumScrollEnd = useCallback(() => {
    if (!manual.current.session) return;
    manual.current.momentum = false; scheduleReturn();
  }, [scheduleReturn]);
  const onLineLayout = useCallback((line: number, y: number) => {
    if (!Number.isFinite(y) || y < 0 || line < 0 || line >= (latest.current.document?.lines.length ?? 0)) return;
    positions.current.set(line, y);
    if (line === latest.current.index && !manual.current.session) follow(true, false);
  }, [follow]);
  const onContentSizeChange = useCallback(() => { if (!manual.current.session) follow(true, false); }, [follow]);
  const onViewportLayout = useCallback((height: number) => {
    if (Number.isFinite(height) && height > 0) setViewportHeight(old => Math.abs(old - height) > .5 ? height : old);
  }, []);
  useLayoutEffect(() => {
    const old = prior.current; prior.current = { active: options.active, document: options.document, height: viewportHeight };
    if (!options.active) { resetManual(); return; }
    const reset = !old.active || old.document !== options.document || old.height !== viewportHeight;
    if (reset) { resetManual(); if (!options.document) positions.current.clear(); }
    follow(reset, !reset);
  }, [options.active, options.document, options.positionMs, index, viewportHeight, follow, resetManual]);
  useEffect(() => () => clearTimer(), [clearTimer]);
  return { scrollRef, viewportHeight, index, onViewportLayout, onLineLayout, onContentSizeChange, resetManual,
    scrollHandlers: { onScrollBeginDrag, onScrollEndDrag, onMomentumScrollBegin, onMomentumScrollEnd } };
}
