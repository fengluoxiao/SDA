import { androidMaterialTheme } from "./android-material-theme";
import { useAppTheme } from "./app-theme";
import React from "react";
import { Platform, type NativeSyntheticEvent } from "react-native";
import { requireNativeView } from "expo";
import type { PlayerProps } from "./RemotePlayer";
import { androidChromeState, isAndroidTabIndex } from "./android-chrome-model";

interface ChromeProps {
  selected: number; dark: boolean; title: string; artist: string; coverUri: string;
  accentColorValue: string; hasTrack: boolean; playing: boolean; busy: boolean; backgroundColorValue: string;
  style: { width: "100%"; height: number };
  onSelect(event: NativeSyntheticEvent<{ index: number }>): void;
  onExpand(): void; onPlayback(): void;
}

const NativeChrome = Platform.OS === "android" && (globalThis as any).expo?.modules?.SdaAndroidChrome
  ? (requireNativeView as unknown as <P>(name: string) => React.ComponentType<P>)<ChromeProps>("SdaAndroidChrome")
  : null;
export const hasAndroidChrome = NativeChrome !== null;

export function AndroidChrome({ player, selected, onSelect, dark, backgroundColor }: {
  player: PlayerProps; selected: number; onSelect(index: number): void; dark: boolean; backgroundColor: string;
}) {
  const { id } = useAppTheme();
  const { primary: accent } = androidMaterialTheme(id, dark);
  if (!NativeChrome) return null;
  const state = androidChromeState(player, selected);
  return <NativeChrome accentColorValue={accent} selected={selected} dark={dark}
    title={player.metadata.title || player.fileName || "等待选择歌曲"}
    artist={player.metadata.artist || player.metadata.albumArtist || "未知艺人"}
    coverUri={player.metadata.coverUri || ""} hasTrack={state.hasTrack}
    playing={state.playing} busy={player.busy}
    backgroundColorValue={backgroundColor} style={{ width: "100%", height: state.height }}
    onSelect={event => { if (isAndroidTabIndex(event.nativeEvent.index)) onSelect(event.nativeEvent.index); }} onExpand={() => onSelect(0)}
    onPlayback={() => player.playing ? player.togglePause() : player.play()} />;
}
