import { useCallback, useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { AccessibilityInfo, Animated, AppState, Easing, Text, View } from "react-native";
import { MINI_MOTION_DURATION, relativeMotionRect, validMotionRect, type MiniPlayerFrames, type MotionRect } from "./mini-player-motion";

type Measurable = { measureInWindow(callback: (x: number, y: number, width: number, height: number) => void): void };
function readRect(node: Measurable | null): Promise<MotionRect | null> {
  return new Promise(resolve => {
    if (!node) { resolve(null); return; }
    const timeout = setTimeout(() => resolve(null), 150);
    try { node.measureInWindow((x, y, width, height) => { clearTimeout(timeout); resolve({ x, y, width, height }); }); }
    catch { clearTimeout(timeout); resolve(null); }
  });
}

type Motion = { id: number; frames: MiniPlayerFrames; target: number; ready: boolean };
export function useSharedMiniPlayerTransition({ enabled, page, layoutKey, mediaKey, artworkUri, rootRef }: {
  enabled: boolean; page: number; layoutKey: string; mediaKey: string; artworkUri: string; rootRef: RefObject<View | null>;
}) {
  const artworkRef = useRef<View>(null), progressRef = useRef<View>(null), timeRef = useRef<Text>(null), miniRef = useRef<View>(null);
  const target = page === 0 ? 0 : 1;
  const travel = useRef(new Animated.Value(target)).current;
  const cached = useRef<{ key: string; frames: MiniPlayerFrames } | null>(null);
  const [prepared, setPrepared] = useState<MiniPlayerFrames | null>(null);
  const [motion, setMotion] = useState<Motion | null>(null);
  const moving = useRef<Motion | null>(null);
  const [readyUri, setReadyUri] = useState("");
  const [reduceMotion, setReduceMotion] = useState(false);
  const previous = useRef({ target, enabled, layoutKey, mediaKey });
  const current = useRef({ target, layoutKey, enabled }); current.current = { target, layoutKey, enabled };
  const generation = useRef(0), readGeneration = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const clearTimer = useCallback(() => { if (timer.current !== null) clearTimeout(timer.current); timer.current = null; }, []);
  const settle = useCallback((destination: number) => {
    ++generation.current; clearTimer(); travel.stopAnimation(); travel.setValue(destination);
    moving.current = null; setMotion(null);
  }, [travel, clearTimer]);
  const onArtworkReady = useCallback((uri: string) => setReadyUri(uri), []);
  const capture = useCallback(() => {
    if (!current.current.enabled || current.current.target !== 0 || moving.current) return;
    const read = ++readGeneration.current;
    void Promise.all([readRect(rootRef.current), readRect(artworkRef.current), readRect(progressRef.current), readRect(timeRef.current), readRect(miniRef.current)]).then(([root, artwork, progress, time, mini]) => {
      if (read !== readGeneration.current || current.current.target !== 0 || current.current.layoutKey !== layoutKey || moving.current ||
          !validMotionRect(root ?? null) || !validMotionRect(artwork ?? null) || !validMotionRect(progress ?? null) || !validMotionRect(time ?? null) || !validMotionRect(mini ?? null)) return;
      const frames = { artwork: relativeMotionRect(artwork!, root!), progress: relativeMotionRect(progress!, root!), time: relativeMotionRect(time!, root!), mini: relativeMotionRect(mini!, root!) };
      cached.current = { key: layoutKey, frames };
      setPrepared(old => JSON.stringify(old) === JSON.stringify(frames) ? old : frames);
    });
  }, [layoutKey, rootRef]);
  useEffect(() => {
    if (!enabled || target !== 0 || motion) return;
    const frame = requestAnimationFrame(capture); return () => cancelAnimationFrame(frame);
  }, [enabled, target, layoutKey, mediaKey, motion, capture]);
  useEffect(() => {
    let live = true;
    AccessibilityInfo.isReduceMotionEnabled().then(value => { if (live) setReduceMotion(value); }).catch(() => {});
    const preference = AccessibilityInfo.addEventListener("reduceMotionChanged", setReduceMotion);
    const activity = AppState.addEventListener("change", state => { if (state !== "active") settle(current.current.target); });
    return () => { live = false; ++generation.current; ++readGeneration.current; clearTimer(); preference.remove(); activity.remove(); travel.stopAnimation(); };
  }, [travel, settle, clearTimer]);
  useLayoutEffect(() => {
    const old = previous.current; previous.current = { target, enabled, layoutKey, mediaKey };
    const reset = !enabled || !old.enabled || old.layoutKey !== layoutKey || old.mediaKey !== mediaKey;
    if (reset || reduceMotion || AppState.currentState !== "active") {
      settle(target);
      if (reset) { ++readGeneration.current; cached.current = null; setPrepared(null); }
      return;
    }
    if (old.target === target) return;
    const frames = moving.current?.frames ?? (cached.current?.key === layoutKey ? cached.current.frames : null);
    if (!frames || (!!artworkUri && readyUri !== artworkUri)) { settle(target); return; }
    const id = ++generation.current;
    clearTimer(); ++readGeneration.current;
    const pending = { id, frames, target, ready: false }; moving.current = pending; setMotion(pending);
    // Read the native position before reversing; JS's last value may lag it.
    travel.stopAnimation(value => {
      if (generation.current !== id) return;
      travel.setValue(Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : old.target);
      const next = { id, frames, target, ready: true }; moving.current = next; setMotion(next);
    });
    // Never leave the real player hidden if a native callback is lost.
    timer.current = setTimeout(() => { if (generation.current === id) settle(current.current.target); }, MINI_MOTION_DURATION + 180);
  }, [enabled, target, layoutKey, mediaKey, artworkUri, readyUri, reduceMotion, travel, settle, clearTimer]);
  useEffect(() => {
    if (!motion?.ready) return;
    // Attach the frozen overlay's native nodes before starting its clock.
    const frame = requestAnimationFrame(() => {
      if (generation.current !== motion.id) return;
      Animated.timing(travel, { toValue: motion.target, duration: MINI_MOTION_DURATION, easing: Easing.bezier(.3, 0, .2, 1), useNativeDriver: true, isInteraction: false })
        .start(() => { if (generation.current === motion.id) settle(motion.target); });
    });
    return () => cancelAnimationFrame(frame);
  }, [motion, travel, settle]);
  // Idle visibility is ordinary React state, independent of native animation state.
  return { artworkRef, progressRef, timeRef, miniRef, travel, frames: motion?.frames ?? prepared, active: motion !== null, motionId: motion?.id ?? 0,
    capture, onArtworkReady, mainOpacity: motion ? 0 : 1, miniOpacity: motion ? 0 : target === 0 ? 0 : 1 };
}
