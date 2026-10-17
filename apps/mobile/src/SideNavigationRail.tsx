import React, { useEffect, useRef, useState } from "react";
import { Animated, Platform, PanResponder, StyleSheet, View, useWindowDimensions } from "react-native";
import { createSideRailGesture, sideRailIndicatorOffset } from "./side-navigation-gesture";
import { IOS_TABS } from "./ios-ui-model";

export const SIDE_NAVIGATION_WIDTH = 48;
const nativeUI: typeof import("@expo/ui/swift-ui") | null = Platform.OS === "ios" &&
  Number(Platform.Version) >= 26 && (globalThis as any).expo?.modules?.ExpoUI &&
  ["HostView", "VStackView", "ImageView"].every(name => (globalThis as any).expo?.getViewConfig?.("ExpoUI", name))
    ? require("@expo/ui/swift-ui") : null;
const symbols = ["play.circle.fill", "music.note.list", "cube.transparent"] as const;

export function SideNavigationRail({ selected, onChange, accent, dark, renderIcon }: {
  selected: number; onChange(index: number): void; accent: string; dark: boolean;
  renderIcon(index: number, color: string): React.ReactNode;
}) {
  const gesture = useRef(createSideRailGesture());
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pressed = useRef(false);
  const startPoint = useRef({ x: 0, y: 0 });
  const callback = useRef(onChange); callback.current = onChange;
  const selectedRef = useRef(selected); selectedRef.current = selected;
  const indicatorY = useRef(new Animated.Value(selected * 46)).current;
  const indicatorScale = useRef(new Animated.Value(1)).current;
  const barScale = useRef(new Animated.Value(1)).current;
  const settledIndex = useRef(selected);
  const inside = useRef(true);
  const [hover, setHover] = useState<number | null>(null);
  const highlighted = hover ?? selected;
  const window = useWindowDimensions();
  const clearTimer = () => { if (timer.current) clearTimeout(timer.current); timer.current = null; };
  const settle = (index: number) => {
    settledIndex.current = index;
    Animated.parallel([
      Animated.spring(indicatorY, { toValue: index * 46, stiffness: 320, damping: 26, mass: 0.8, useNativeDriver: true }),
      Animated.spring(indicatorScale, { toValue: 1, stiffness: 300, damping: 22, mass: 0.7, useNativeDriver: true }),
      Animated.spring(barScale, { toValue: 1, stiffness: 280, damping: 24, mass: 0.8, useNativeDriver: true }),
    ]).start();
  };
  const lift = () => Animated.parallel([
    Animated.spring(indicatorScale, { toValue: 1.18, stiffness: 300, damping: 22, mass: 0.7, useNativeDriver: true }),
    Animated.spring(barScale, { toValue: 1.04, stiffness: 280, damping: 24, mass: 0.8, useNativeDriver: true }),
  ]).start();
  const cancel = () => { clearTimer(); pressed.current = false; gesture.current.cancel(); setHover(null); settle(selectedRef.current); };
  useEffect(cancel, [window.width, window.height]);
  useEffect(() => { if (!pressed.current && settledIndex.current !== selected) settle(selected); }, [selected]);
  useEffect(() => () => { clearTimer(); pressed.current = false; gesture.current.cancel(); indicatorY.stopAnimation(); indicatorScale.stopAnimation(); barScale.stopAnimation(); }, []);
  // The topmost RN view is the only physical touch target. Its local start
  // coordinates plus PanResponder's translation stay in one coordinate space;
  // no asynchronous window measurement or SwiftUI Button recognizer competes.
  const pan = useRef(PanResponder.create({
    onStartShouldSetPanResponder: () => true,
    onMoveShouldSetPanResponder: () => true,
    onPanResponderTerminationRequest: () => false,
    onShouldBlockNativeResponder: () => true,
    onPanResponderGrant: (event, state) => {
      cancel(); if (state.numberActiveTouches > 1) return;
      const point = { x: event.nativeEvent.locationX, y: event.nativeEvent.locationY };
      if (!Number.isFinite(point.x) || !Number.isFinite(point.y)) return;
      pressed.current = true; startPoint.current = point;
      setHover(gesture.current.begin(point, Date.now()));
      inside.current = true;
      Animated.spring(indicatorY, { toValue: sideRailIndicatorOffset(point.y), stiffness: 320, damping: 26, mass: 0.8, useNativeDriver: true }).start();
      lift();
      if (__DEV__) console.info("[SDA SideNav] touch", point);
      timer.current = setTimeout(() => { if (pressed.current) setHover(gesture.current.hold(Date.now())); }, 300);
    },
    onPanResponderMove: (_event, state) => {
      if (!pressed.current) return;
      if (state.numberActiveTouches > 1) { cancel(); return; }
      const point = { x: startPoint.current.x + state.dx, y: startPoint.current.y + state.dy };
      const target = gesture.current.move(point, Date.now());
      setHover(target);
      if (target !== null) {
        if (!inside.current) lift();
        indicatorY.stopAnimation(); indicatorY.setValue(sideRailIndicatorOffset(point.y));
      } else if (inside.current) settle(selectedRef.current);
      inside.current = target !== null;
    },
    onPanResponderRelease: (_event, state) => {
      clearTimer(); pressed.current = false;
      const target = gesture.current.end({ x: startPoint.current.x + state.dx, y: startPoint.current.y + state.dy }, Date.now());
      if (__DEV__) console.info("[SDA SideNav] release", { dx: state.dx, dy: state.dy, target });
      setHover(null); settle(target ?? selectedRef.current);
      if (target !== null) callback.current(target);
    },
    onPanResponderTerminate: cancel,
  })).current;
  const material = nativeUI ? require("@expo/ui/swift-ui/modifiers") as typeof import("@expo/ui/swift-ui/modifiers") : null;
  // The dock owns screen insets. Both native Hosts must ignore container
  // safe areas so the backdrop and symbols share the RN selection/touch frame
  // even when the entire rail is repositioned near a screen edge.
  // Only the stationary outer capsule uses a native glass backdrop. Moving
  // an independent glass Host over it produced an opaque disk on real devices.
  // Match the upper settings/tracking buttons: let the glass inherit the
  // system appearance. Symbol colors are configured separately below.
  const backdrop = nativeUI && material ?
    <nativeUI.Host ignoreSafeArea="container" style={{ width: 48, height: 140, backgroundColor: "transparent" }}>
      <nativeUI.VStack modifiers={[material.frame({ width: 48, height: 140 }), material.glassEffect({ glass: { variant: "regular" }, shape: "capsule" })]}>{null}</nativeUI.VStack>
    </nativeUI.Host> : <View style={{ width: 48, height: 140, borderRadius: 24, backgroundColor: dark ? "rgba(55,55,60,.65)" : "rgba(245,245,250,.65)" }} />;
  const visual = <Animated.View testID="side-tab-glass-bar" style={{ width: 48, height: 140, transform: [{ scale: barScale }] }}>
    {backdrop}
    {/* Neutral selection fill below the icons, with no extra backdrop sampling. */}
    <Animated.View testID="side-tab-glass-selection" style={{ position: "absolute", left: 2, top: 2, width: 44, height: 44,
      transform: [{ translateY: indicatorY }, { scale: indicatorScale }] }}>
      <View testID="side-tab-neutral-selection" style={{ width: 44, height: 44, borderRadius: 22, backgroundColor: dark ? "rgba(255,255,255,0.14)" : "rgba(118,118,128,0.18)" }} />
    </Animated.View>
    <View testID="side-tab-symbols" style={[StyleSheet.absoluteFill, { zIndex: 1 }]}>
      {nativeUI && material ? <nativeUI.Host ignoreSafeArea="container" style={{ width: 48, height: 140 }} colorScheme={dark ? "dark" : "light"} seedColor={accent}>
        <nativeUI.VStack spacing={2} modifiers={[material.padding({ all: 2 }), material.frame({ width: 48, height: 140 })]}>
          {IOS_TABS.map((label, index) => <nativeUI.VStack key={label} modifiers={[material.frame({ width: 44, height: 44 }),
            // Apply color on the native container as well as the host tint:
            // SF Symbols must not fall back to the host's white/black label color.
            material.foregroundColor(highlighted === index ? accent : dark ? "#FFFFFF" : "#000000"),
            material.tint(highlighted === index ? accent : dark ? "#FFFFFF" : "#000000")]}>
            <nativeUI.Image systemName={symbols[index]} size={hover === index ? 22 : 20} />
          </nativeUI.VStack>)}
        </nativeUI.VStack>
      </nativeUI.Host> : <View style={{ padding: 2, gap: 2 }}>
        {IOS_TABS.map((label, index) => <View key={label} style={s.tab}>{renderIcon(index, highlighted === index ? accent : dark ? "#FFFFFF" : "#000000")}</View>)}
      </View>}
    </View>
  </Animated.View>;
  return <View collapsable={false} style={{ width: 48, height: 140 }}>
    <View pointerEvents="none" accessibilityElementsHidden importantForAccessibility="no-hide-descendants">{visual}</View>
    <View pointerEvents="box-none" style={[StyleSheet.absoluteFill, { padding: 2, gap: 2 }]}>
      {IOS_TABS.map((label, index) => <View key={label} accessible accessibilityRole="tab" accessibilityLabel={label}
        accessibilityState={{ selected: index === selected }} accessibilityActions={[{ name: "activate" }]}
        onAccessibilityTap={() => onChange(index)} onAccessibilityAction={() => onChange(index)} style={s.tab} />)}
    </View>
    <View collapsable={false} accessible={false} pointerEvents="auto" style={[StyleSheet.absoluteFill, { zIndex: 2 }]} {...pan.panHandlers} />
  </View>;
}
const s = StyleSheet.create({
  rail: { width: 48, height: 140, borderRadius: 24, padding: 2, gap: 2 },
  tab: { width: 44, height: 44, borderRadius: 22, alignItems: "center", justifyContent: "center" },
});
