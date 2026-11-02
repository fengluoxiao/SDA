import { useEffect, useMemo, useRef, useState } from "react";
import { Animated, PanResponder, type LayoutChangeEvent } from "react-native";

type Options = {
  sourceKey: string;
  durationMs: number;
  positionMs: number;
  disabled: boolean;
  seekTo?: (positionMs: number) => void;
};

// Hold/drag changes only the preview. Native playback receives one seek on release.
export function useProgressScrubber(options: Options) {
  const latest = useRef(options);
  latest.current = options;
  const width = useRef(0);
  const drag = useRef<{ sourceKey: string; durationMs: number; startX: number } | null>(null);
  const [preview, setPreview] = useState<{ sourceKey: string; positionMs: number } | null>(null);
  const scale = useRef(new Animated.Value(1)).current;
  const gesture = useMemo(() => {
    const enabled = () => {
      const p = latest.current;
      return !p.disabled && !!p.sourceKey && !!p.seekTo && Number.isFinite(p.durationMs) && p.durationMs > 0 && width.current > 0;
    };
    const animate = (active: boolean) => {
      scale.stopAnimation();
      Animated.spring(scale, { toValue: active ? 1.75 : 1, speed: 28, bounciness: 0, useNativeDriver: true }).start();
    };
    const cancel = () => { drag.current = null; setPreview(null); animate(false); };
    const position = (x: number) => Math.max(0, Math.min(latest.current.durationMs - 1, x / width.current * latest.current.durationMs));
    const valid = () => enabled() && drag.current?.sourceKey === latest.current.sourceKey && drag.current?.durationMs === latest.current.durationMs;
    const responder = PanResponder.create({
      onStartShouldSetPanResponder: enabled,
      onStartShouldSetPanResponderCapture: enabled,
      onPanResponderTerminationRequest: () => false,
      onPanResponderGrant: (event, state) => {
        if (!enabled() || state.numberActiveTouches > 1 || !Number.isFinite(event.nativeEvent.locationX)) { cancel(); return; }
        const p = latest.current;
        drag.current = { sourceKey: p.sourceKey, durationMs: p.durationMs, startX: event.nativeEvent.locationX };
        setPreview({ sourceKey: p.sourceKey, positionMs: position(event.nativeEvent.locationX) });
        animate(true);
      },
      onPanResponderMove: (_, state) => {
        if (!valid() || state.numberActiveTouches > 1 || !Number.isFinite(state.dx)) { cancel(); return; }
        setPreview({ sourceKey: latest.current.sourceKey, positionMs: position(drag.current!.startX + state.dx) });
      },
      onPanResponderRelease: (_, state) => {
        const target = valid() && Number.isFinite(state.dx) ? position(drag.current!.startX + state.dx) : null;
        cancel();
        if (target !== null) latest.current.seekTo?.(target);
      },
      onPanResponderTerminate: cancel,
    });
    return { ...responder, cancel };
  }, [scale]);
  useEffect(() => { gesture.cancel(); }, [options.sourceKey, options.durationMs, options.disabled, gesture]);
  useEffect(() => () => { drag.current = null; scale.stopAnimation(); }, [scale]);
  const onLayout = (event: LayoutChangeEvent) => {
    const next = event.nativeEvent.layout.width;
    if (width.current !== next && drag.current) gesture.cancel();
    width.current = Number.isFinite(next) ? Math.max(0, next) : 0;
  };
  const isScrubbing = preview !== null && preview.sourceKey === options.sourceKey && !options.disabled;
  const positionMs = isScrubbing ? preview.positionMs : options.positionMs;
  const progress = options.durationMs > 0 ? Math.max(0, Math.min(1, positionMs / options.durationMs)) : 0;
  return { panHandlers: gesture.panHandlers, onLayout, scale, isScrubbing, positionMs, progress };
}
