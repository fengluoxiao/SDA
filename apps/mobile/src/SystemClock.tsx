import React, { memo, useEffect, useState } from "react";
import { AppState, StyleSheet, Text } from "react-native";

export const SYSTEM_CLOCK_LINE_HEIGHT = 18;

function localTime() {
  return new Date().toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
}

// Keep minute updates local to the clock, outside the player's audio/status state.
export const SystemClock = memo(function SystemClock({ color, sideDock }: { color: string; sideDock: boolean }) {
  const [time, setTime] = useState(localTime);
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const clear = () => { if (timer !== undefined) clearTimeout(timer); timer = undefined; };
    const refresh = () => {
      clear();
      setTime(localTime());
      if (AppState.currentState === "active") {
        timer = setTimeout(refresh, 60_000 - Date.now() % 60_000 + 20);
      }
    };
    const activity = AppState.addEventListener("change", state => state === "active" ? refresh() : clear());
    refresh();
    return () => { clear(); activity.remove(); };
  }, []);
  return <Text testID="landscape-system-clock" accessibilityLabel={"系统时间 " + time} numberOfLines={1}
    maxFontSizeMultiplier={1.3} adjustsFontSizeToFit minimumFontScale={0.8}
    style={[styles.clock, { color, marginRight: sideDock ? 0 : 8 }]}>{time}</Text>;
});

const styles = StyleSheet.create({
  clock: { width: 48, flexShrink: 0, textAlign: "center", fontSize: 13, lineHeight: SYSTEM_CLOCK_LINE_HEIGHT, fontWeight: "600", fontVariant: ["tabular-nums"] },
});
