import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Animated, PanResponder, type PanResponderGestureState } from "react-native";

type Options = { width: number; enabled: boolean; visible: boolean; initialPage?: 0 | 1; selectedPage?: 0 | 1; onPageChange?(page: 0 | 1): void; onPageSettled?(page: 0 | 1): void };
/** Store a logical page fraction, so a rotation cannot retain an old pixel offset. */
export function useLyricsSwipe(options: Options) {
  const latest = useRef(options); latest.current = options;
  const initialPage = options.selectedPage ?? options.initialPage ?? 0;
  const selected = useRef<0 | 1>(initialPage), origin = useRef<0 | 1>(initialPage), blocked = useRef(false);
  const transition = useRef(0);
  const position = useRef(new Animated.Value(initialPage)).current;
  const translation = useMemo(() => position.interpolate({ inputRange: [0, 1], outputRange: [0, -options.width], extrapolate: "clamp" }), [position, options.width]);
  const [page, setPage] = useState<0 | 1>(initialPage);
  const setBlocked = useCallback((value: boolean) => { blocked.current = value; }, []);
  const settle = useCallback((next: 0 | 1) => {
    const id = ++transition.current;
    selected.current = next; setPage(next); latest.current.onPageChange?.(next);
    position.stopAnimation();
    Animated.spring(position, { toValue: next, damping: 24, stiffness: 260, mass: 1, overshootClamping: true, useNativeDriver: true }).start(({ finished }) => { if (finished && id === transition.current && latest.current.enabled && latest.current.visible) latest.current.onPageSettled?.(next); });
  }, [position]);
  const gesture = useMemo(() => {
    const canClaim = (_event: unknown, g: PanResponderGestureState) => {
      const p = latest.current;
      return p.enabled && p.visible && !blocked.current && g.numberActiveTouches === 1 &&
        Math.abs(g.dx) >= 12 && Math.abs(g.dx) > Math.abs(g.dy) * 1.5 &&
        (selected.current === 0 ? g.dx < 0 : g.dx > 0);
    };
    return PanResponder.create({
      onStartShouldSetPanResponder: () => false,
      onMoveShouldSetPanResponder: canClaim,
      onMoveShouldSetPanResponderCapture: canClaim,
      onPanResponderGrant: () => { ++transition.current; origin.current = selected.current; position.stopAnimation(); },
      onPanResponderMove: (_event, g) => {
        if (!latest.current.enabled || !latest.current.visible) return;
        position.setValue(Math.max(0, Math.min(1, origin.current - g.dx / Math.max(1, latest.current.width))));
      },
      onPanResponderRelease: (_event, g) => {
        if (!latest.current.enabled || !latest.current.visible) return;
        const forward = origin.current === 0 ? g.dx < 0 : g.dx > 0;
        const distance = Math.abs(g.dx), flick = distance >= 12 && Math.abs(g.vx) >= .3 && g.vx * g.dx > 0;
        settle(forward && (distance >= 28 || flick) ? (origin.current === 0 ? 1 : 0) : origin.current);
      },
      onPanResponderTerminationRequest: () => false,
      onPanResponderTerminate: () => { if (latest.current.enabled && latest.current.visible) settle(origin.current); },
      onShouldBlockNativeResponder: () => false,
    });
  }, [settle, position]);
  useLayoutEffect(() => {
    const next = options.selectedPage;
    if (next !== undefined && next !== selected.current) {
      ++transition.current; selected.current = next; setPage(next); position.stopAnimation(); position.setValue(next);
    }
  }, [options.selectedPage, position]);
  useLayoutEffect(() => {
    if (!options.enabled || !options.visible) blocked.current = false;
    ++transition.current; position.stopAnimation(); position.setValue(selected.current);
  }, [options.enabled, options.visible, position]);
  useEffect(() => () => { ++transition.current; position.stopAnimation(); }, [position]);
  return { panHandlers: gesture.panHandlers, translation, position, page, setBlocked, back: useCallback(() => settle(0), [settle]) };
}
