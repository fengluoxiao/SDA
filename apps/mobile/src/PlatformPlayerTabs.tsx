import { androidMaterialTheme } from "./android-material-theme";
import { useAppTheme } from "./app-theme";
import React, { useState } from "react";
import { Platform, View, type ScrollViewProps } from "react-native";
import { hasSystemIOSTabs, IOSSystemTabs } from "./IOSSystemTabs";
import { AndroidChrome, AndroidPageHeader, hasAndroidChrome } from "./AndroidChrome";
import type { PlayerProps } from "./RemotePlayer";
import { androidChromeState } from "./android-chrome-model";

export const hasNativePlayerTabs = hasSystemIOSTabs || hasAndroidChrome;

export function PlatformPlayerTabs({ player, ...props }: React.ComponentProps<typeof IOSSystemTabs> & { player: PlayerProps }) {
  const appTheme = useAppTheme();
  const [libraryScrolled, setLibraryScrolled] = useState(false);
  const md3 = androidMaterialTheme(appTheme.id, props.theme === "dark");
  if (Platform.OS !== "android") return <IOSSystemTabs {...props} />;
  if (!hasAndroidChrome) return <><View style={{ flex: 1 }}>{props.children}</View>{props.selected !== 0 && props.accessory}{props.fallback}</>;
  const side = !!props.sideBar;
  const mini = androidChromeState(player, props.selected).showMini;
  const footerHeight = side ? (mini ? 64 : 0) : androidChromeState(player, props.selected).height;
  const leftRail = side && props.sideBarPosition === "left";
  const rightRail = side && !leftRail;
  // All coordinates are inside the outer system safe area. Keep 12 dp between
  // the 48 dp capsule (8 dp from the edge) and content; the indicator uses half.
  const contentInset = (props.sideBarWidth ?? 48) + 8 + 12;
  const scrollInset = (props.sideBarWidth ?? 48) + 8 + 6;
  const parentLeft = leftRail ? 30 : 18;
  const parentRight = leftRail ? 6 : 18;
  const renderPage = (page: React.ReactNode, index: number) => {
    if (index !== 1) return <><View style={leftRail ? { marginLeft: -12, marginRight: 12 } : undefined}>{props.pageHeaders?.[index]}</View>{page}</>;
    const scroll = React.isValidElement<ScrollViewProps>(page) ? React.cloneElement(page, {
      // Reserve an actual viewport lane: Android ignores iOS scrollIndicatorInsets.
      style: [page.props.style, side && { marginLeft: (leftRail ? scrollInset : 0) - parentLeft,
        marginRight: (rightRail ? scrollInset : 0) - parentRight }],
      contentContainerStyle: [page.props.contentContainerStyle, { paddingTop: 64, paddingBottom: footerHeight + 24 },
        side && { paddingLeft: leftRail ? contentInset - scrollInset : 18, paddingRight: rightRail ? contentInset - scrollInset : 18 }],
      scrollEventThrottle: 32,
      onScroll: event => { setLibraryScrolled(event.nativeEvent.contentOffset.y > 1); page.props.onScroll?.(event); },
      scrollIndicatorInsets: { ...page.props.scrollIndicatorInsets, top: 56, bottom: footerHeight + 8 },
    }) : page;
    return <View style={{ flex: 1 }}>{scroll}
      <View pointerEvents="box-none" style={{ position: "absolute", top: 0, left: (leftRail ? contentInset : 0) - parentLeft,
        right: (rightRail ? contentInset : 0) - parentRight }}>
        <AndroidPageHeader title="资料库" dark={props.theme === "dark"} backgroundColor={md3.bg} scrolled={libraryScrolled}
          onAction={side ? undefined : props.onOpenSettings} />
      </View>
    </View>;
  };
  return <View style={{ flex: 1, marginHorizontal: -18 }}>
    <View style={{ flex: 1, paddingHorizontal: 18, ...(side && props.sideBarPosition === "left" ? { paddingLeft: 30, paddingRight: 6 } : {}) }}>{React.Children.toArray(props.children).map((page, index) =>
      <View key={index} style={{ flex: 1, paddingBottom: index === 1 ? 0 : footerHeight, display: props.selected === index ? "flex" : "none" }}>{renderPage(page, index)}</View>)}</View>
    {(!side || mini) && <View style={{ position: "absolute", bottom: 0, left: leftRail ? contentInset - 16 : 0, right: rightRail ? contentInset - 16 : 0, paddingHorizontal: 16, overflow: "visible" }}><AndroidChrome presentation={side ? "mini" : "bottom"} player={player} selected={props.selected} onSelect={props.onChange}
      dark={props.theme === "dark"} backgroundColor={props.selected === 0 ? "#111113" : md3.bg} /></View>}
  </View>;
}
