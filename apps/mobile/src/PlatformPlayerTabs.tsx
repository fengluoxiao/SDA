import { androidMaterialTheme } from "./android-material-theme";
import { useAppTheme } from "./app-theme";
import React from "react";
import { Platform, View } from "react-native";
import { hasSystemIOSTabs, IOSSystemTabs } from "./IOSSystemTabs";
import { AndroidChrome, hasAndroidChrome } from "./AndroidChrome";
import type { PlayerProps } from "./RemotePlayer";
import { androidChromeState } from "./android-chrome-model";

export const hasNativePlayerTabs = hasSystemIOSTabs || hasAndroidChrome;

export function PlatformPlayerTabs({ player, ...props }: React.ComponentProps<typeof IOSSystemTabs> & { player: PlayerProps }) {
  const appTheme = useAppTheme();
  const md3 = androidMaterialTheme(appTheme.id, props.theme === "dark");
  if (Platform.OS !== "android") return <IOSSystemTabs {...props} />;
  const footerHeight = androidChromeState(player, props.selected).height;
  return <View style={{ flex: 1, marginHorizontal: -18 }}>
    <View style={{ flex: 1, paddingHorizontal: 18 }}>{React.Children.toArray(props.children).map((page, index) =>
      <View key={index} style={{ flex: 1, paddingBottom: footerHeight, display: props.selected === index ? "flex" : "none" }}>{index === 1 && React.isValidElement<{ contentContainerStyle?: object }>(page) ? React.cloneElement(page, { contentContainerStyle: [page.props.contentContainerStyle, { paddingBottom: 20 }] as unknown as object }) : page}</View>)}</View>
    <View style={{ position: "absolute", bottom: 0, left: 0, right: 0, paddingHorizontal: 16, overflow: "visible" }}><AndroidChrome player={player} selected={props.selected} onSelect={props.onChange}
      dark={props.selected === 0 || props.theme === "dark"} backgroundColor={props.selected === 0 ? "#111113" : md3.bg} /></View>
  </View>;
}
