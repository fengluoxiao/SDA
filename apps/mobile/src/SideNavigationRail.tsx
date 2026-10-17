import React, { useEffect, useRef, useState } from "react";
import { Platform, PanResponder, StyleSheet, View, useWindowDimensions } from "react-native";
import { createSideRailGesture } from "./side-navigation-gesture";
import { IOS_TABS } from "./ios-ui-model";

export const SIDE_NAVIGATION_WIDTH = 60;
const nativeUI: typeof import("@expo/ui/swift-ui") | null = Platform.OS === "ios" &&
  Number(Platform.Version) >= 26 && (globalThis as any).expo?.modules?.ExpoUI &&
  ["HostView", "VStackView", "Button"].every(name => (globalThis as any).expo?.getViewConfig?.("ExpoUI", name))
    ? require("@expo/ui/swift-ui") : null;
const symbols = ["play.circle.fill", "music.note.list", "cube.transparent"] as const;

export function SideNavigationRail({ selected, onChange, accent, dark, renderIcon }: {
  selected: number; onChange(index: number): void; accent: string; dark: boolean;
  renderIcon(index: number, color: string): React.ReactNode;
}) {
  const view = useRef<View>(null);
  const origin = useRef({ x: 0, y: 0 });
  const gesture = useRef(createSideRailGesture());
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pressed = useRef(false);
  const touchSequence = useRef(0);
  const touch = useRef({ pageX: 0, pageY: 0 });
  const callback = useRef(onChange); callback.current = onChange;
  const [hover, setHover] = useState<number | null>(null);
  const highlighted = hover ?? selected;
  const window = useWindowDimensions();
  const clearTimer = () => { if (timer.current) clearTimeout(timer.current); timer.current = null; };
  const measure = () => view.current?.measureInWindow((x, y) => { origin.current = { x, y }; });
  const point = (event: { pageX: number; pageY: number }) => ({ x: event.pageX - origin.current.x, y: event.pageY - origin.current.y });
  useEffect(() => { clearTimer(); pressed.current = false; touchSequence.current++; gesture.current.cancel(); setHover(null); measure(); }, [window.width, window.height]);
  useEffect(() => () => { clearTimer(); pressed.current = false; gesture.current.cancel(); }, []);
  const pan = useRef(PanResponder.create({
    onStartShouldSetPanResponderCapture: () => true,
    onMoveShouldSetPanResponderCapture: () => true,
    onPanResponderTerminationRequest: () => false,
    onShouldBlockNativeResponder: () => true,
    onPanResponderGrant: event => {
      clearTimer();
      if (event.nativeEvent.touches.length !== 1) { pressed.current = false; gesture.current.cancel(); setHover(null); return; }
      pressed.current = true; touch.current = event.nativeEvent;
      const started = Date.now(); const initialTouch = event.nativeEvent; const sequence = ++touchSequence.current;
      setHover(gesture.current.begin(point(event.nativeEvent), started));
      view.current?.measureInWindow((x, y) => {
        if (touchSequence.current !== sequence) return;
        origin.current = { x, y };
        if (pressed.current) { gesture.current.begin(point(initialTouch), started); setHover(gesture.current.move(point(touch.current), Date.now())); }
      });
      timer.current = setTimeout(() => { if (pressed.current) setHover(gesture.current.hold(Date.now())); }, 300);
    },
    onPanResponderMove: event => {
      touch.current = event.nativeEvent;
      if (event.nativeEvent.touches.length > 1) { clearTimer(); pressed.current = false; gesture.current.cancel(); setHover(null); return; }
      setHover(gesture.current.move(point(event.nativeEvent), Date.now()));
    },
    onPanResponderRelease: event => {
      clearTimer(); pressed.current = false;
      const target = gesture.current.end(point(event.nativeEvent), Date.now());
      setHover(null); if (target !== null) callback.current(target);
    },
    onPanResponderTerminate: () => { clearTimer(); pressed.current = false; gesture.current.cancel(); setHover(null); },
  })).current;
  let visual: React.ReactNode;
  if (nativeUI) {
    const { frame, padding, glassEffect, buttonStyle, labelStyle, foregroundStyle, font, accessibilityLabel, accessibilityValue } =
      require("@expo/ui/swift-ui/modifiers") as typeof import("@expo/ui/swift-ui/modifiers");
    visual = <nativeUI.Host style={{ width: 48, height: 140 }} colorScheme={dark ? "dark" : "light"}>
      <nativeUI.VStack spacing={2} modifiers={[padding({ all: 2 }), frame({ width: 48, height: 140 }), glassEffect({ glass: { variant: "clear" }, shape: "capsule" })]}>
        {IOS_TABS.map((label, index) => <nativeUI.Button key={label} label={label} systemImage={symbols[index]} onPress={() => onChange(index)}
          modifiers={[buttonStyle("plain"), labelStyle("iconOnly"), font({ size: 20 }), frame({ width: 44, height: 44 }),
            foregroundStyle(highlighted === index ? accent : dark ? "#f5f5f7" : "#1c1c1e"),
            accessibilityLabel(label), accessibilityValue(highlighted === index ? "已选择" : "未选择"),
            ...(highlighted === index ? [glassEffect({ glass: { variant: "clear", interactive: true }, shape: "circle" })] : [])]} />)}
      </nativeUI.VStack>
    </nativeUI.Host>;
  } else visual = <View style={[s.rail, { backgroundColor: dark ? "rgba(55,55,60,.65)" : "rgba(245,245,250,.65)" }]}>
    {IOS_TABS.map((label, index) => <View key={label} style={[s.tab, { backgroundColor: highlighted === index ? (dark ? "#ffffff22" : "#0000000a") : "transparent" }]}>
      {renderIcon(index, highlighted === index ? accent : dark ? "#f5f5f7" : "#1c1c1e")}
    </View>)}
  </View>;
  return <View ref={view} collapsable={false} onLayout={measure} style={{ width: 48, height: 140 }} {...pan.panHandlers}>
    <View pointerEvents="none" accessibilityElementsHidden importantForAccessibility="no-hide-descendants">{visual}</View>
    <View pointerEvents="box-none" style={[StyleSheet.absoluteFill, { padding: 2, gap: 2 }]}>
      {IOS_TABS.map((label, index) => <View key={label} accessible accessibilityRole="tab" accessibilityLabel={label}
        accessibilityState={{ selected: index === selected }} accessibilityActions={[{ name: "activate" }]}
        onAccessibilityTap={() => onChange(index)} onAccessibilityAction={() => onChange(index)} style={s.tab} />)}
    </View>
  </View>;
}
const s = StyleSheet.create({
  rail: { width: 48, height: 140, borderRadius: 24, padding: 2, gap: 2 },
  tab: { width: 44, height: 44, borderRadius: 22, alignItems: "center", justifyContent: "center" },
});
