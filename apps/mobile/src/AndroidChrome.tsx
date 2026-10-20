import { useSafeAreaInsets } from "react-native-safe-area-context";
import { androidMaterialTheme } from "./android-material-theme";
import { useAppTheme } from "./app-theme";
import React from "react";
import { Platform, type NativeSyntheticEvent, type StyleProp, type ViewStyle } from "react-native";
import { requireNativeView } from "expo";
import type { PlayerProps } from "./RemotePlayer";
import { ANDROID_BOTTOM_TAB_WIDTH, androidChromeState, androidHeaderLayout, isAndroidTabIndex } from "./android-chrome-model";

interface ChromeProps {
  preserveAccentColor?: boolean; tabBarWidth?: number; selected?: number; dark: boolean; title?: string; artist?: string; coverUri?: string;
  accentColorValue?: string; hasTrack?: boolean; playing?: boolean; busy?: boolean; backgroundColorValue?: string;
  presentation?: "bottom" | "rail" | "mini" | "header" | "toggle" | "button";
  topInset?: number; checked?: boolean; blurred?: boolean; headerAction?: boolean; backButton?: boolean;
  style: StyleProp<ViewStyle>;
  onSelect?(event: NativeSyntheticEvent<{ index: number }>): void;
  onExpand?(): void; onPlayback?(): void;
  onToggle?(event: NativeSyntheticEvent<{ value: boolean }>): void;
}

const NativeChrome = Platform.OS === "android" && (globalThis as any).expo?.modules?.SdaAndroidChrome
  ? (requireNativeView as unknown as <P>(name: string) => React.ComponentType<P>)<ChromeProps>("SdaAndroidChrome")
  : null;
export const hasAndroidChrome = NativeChrome !== null;

export function AndroidChrome({ player, selected, onSelect, dark, backgroundColor, presentation = "bottom" }: {
  player: PlayerProps; selected: number; onSelect(index: number): void; dark: boolean; backgroundColor: string; presentation?: "bottom" | "mini";
}) {
  const { id, accent: themeAccent } = useAppTheme();
  // Library/space pages use white surfaces in light mode. Keep their native
  // tab foreground at the selected character's actual color, as on iOS.
  const preserveAccentColor = !dark && selected !== 0;
  const accent = preserveAccentColor ? themeAccent : androidMaterialTheme(id, dark).primary;
  if (!NativeChrome) return null;
  const state = androidChromeState(player, selected);
  const compact = presentation === "bottom" && !state.showMini;
  return <NativeChrome preserveAccentColor={preserveAccentColor} tabBarWidth={ANDROID_BOTTOM_TAB_WIDTH} presentation={presentation} accentColorValue={accent} selected={selected} dark={dark}
    title={player.metadata.title || player.fileName || "等待选择歌曲"}
    artist={player.metadata.artist || player.metadata.albumArtist || "未知艺人"}
    coverUri={player.metadata.coverUri || ""} hasTrack={state.hasTrack}
    playing={state.playing} busy={player.busy}
    backgroundColorValue={backgroundColor} style={{ width: "100%", height: presentation === "mini" ? 48 : state.height, ...(compact ? { maxWidth: ANDROID_BOTTOM_TAB_WIDTH, alignSelf: "center" as const } : {}) }}
    onSelect={event => { if (isAndroidTabIndex(event.nativeEvent.index)) onSelect(event.nativeEvent.index); }} onExpand={() => onSelect(0)}
    onPlayback={() => player.playing ? player.togglePause() : player.play()} />;
}

// A real vertical Compose layout: upright icons, vertical hit targets and the
// upstream lens/drag/highlight passes all share the same coordinate system.
export function AndroidSideTabs({ selected, onChange, accent, dark }: {
  selected: number; onChange(index: number): void; accent: string; dark: boolean;
}) {
  return NativeChrome ? <NativeChrome preserveAccentColor={!dark && selected !== 0} presentation="rail" selected={selected} dark={dark}
    accentColorValue={accent} title="" artist="" coverUri="" hasTrack={false} playing={false} busy={false}
    backgroundColorValue="transparent" style={{ width: 48, height: 140 }}
    onSelect={event => { if (isAndroidTabIndex(event.nativeEvent.index)) onChange(event.nativeEvent.index); }}
    onExpand={() => {}} onPlayback={() => {}} /> : null;
}

export function AndroidGlassToggle({ value, onChange, label, disabled = false, dark, accent }: {
  value: boolean; onChange(value: boolean): void; label: string; disabled?: boolean; dark: boolean; accent: string;
}) {
  return NativeChrome ? <NativeChrome presentation="toggle" checked={value} title={label} busy={disabled} dark={dark}
    accentColorValue={accent} style={{ width: 72, height: 48 }}
    onToggle={event => { if (!disabled && typeof event.nativeEvent.value === "boolean") onChange(event.nativeEvent.value); }} /> : null;
}

export function AndroidPageHeader({ title, dark, backgroundColor, scrolled, onAction, back = false, style }: {
  title: string; dark: boolean; backgroundColor: string; scrolled: boolean; onAction?(): void; back?: boolean; style?: StyleProp<ViewStyle>;
}) {
  const header = androidHeaderLayout(useSafeAreaInsets().top);
  return NativeChrome ? <NativeChrome topInset={header.top} presentation="header" title={title} dark={dark} backgroundColorValue={backgroundColor}
    blurred={scrolled} headerAction={!!onAction} backButton={back} onExpand={onAction}
    style={[{ height: header.height, width: "100%" }, style]} /> : null;
}

export function AndroidGlassAction({ title, onPress, dark, accent }: {
  title: string; onPress(): void; dark: boolean; accent: string;
}) {
  return NativeChrome ? <NativeChrome presentation="button" title={title} onExpand={onPress} dark={dark}
    accentColorValue={accent} style={{ width: 76, height: 44 }} /> : null;
}
