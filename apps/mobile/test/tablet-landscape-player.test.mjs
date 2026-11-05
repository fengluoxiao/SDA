import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync } from "node:fs";
import { build } from "esbuild";
const layoutBundle = await build({ entryPoints: ["apps/mobile/src/tablet-player-layout.ts"], bundle: true, platform: "node", format: "esm", write: false });
const { isTabletPlayer, tabletImmersivePlayerLayout } = await import("data:text/javascript;base64," + Buffer.from(layoutBundle.outputFiles[0].text).toString("base64"));
const uiBundle = await build({ entryPoints: ["apps/mobile/src/TabletLandscapePlayer.tsx"], bundle: true, platform: "node", format: "cjs", external: ["*"], write: false });
const React = { createElement: (type, props, ...children) => ({ type, props: { ...props, children } }) };
const mod = { exports: {} };
new Function("require", "module", "exports", uiBundle.outputFiles[0].text)(name => name === "react" ? React : name === "react-native" ? { View: "View", Pressable: "Pressable", StyleSheet: { create: x => x } } : { LyricsPanel: "LyricsPanel" }, mod, mod.exports);
const childrenOf = node => node.props.children.flat(Infinity).filter(Boolean);

test("tablet landscape keeps one transport in the lower album column and live shared lyrics on the right", () => {
  const transport = { type: "ExistingTransport", props: {} };
  for (const visible of [false, true]) for (const selectedUri of ["file:///first", "file:///next"]) {
    const calls = [], player = { selectedUri, chooseFile: () => calls.push("choose"), metadata: {}, busy: false };
    const tree = mod.exports.TabletLandscapePlayer({ player, visible, children: transport, albumWidth: 560, controlsWidth: 400, horizontalPadding: 32, verticalPadding: 20, coverLabel: "专辑封面" });
    const [album, lyrics] = childrenOf(tree), [cover, controls] = childrenOf(album), [viewport] = childrenOf(lyrics), [panel] = childrenOf(viewport);
    assert.equal(childrenOf(controls)[0], transport);
    assert.equal(album.props.style.at(-1).width, 560, "album column retains the artwork width independently of transport width");
    assert.equal(controls.props.style.width, 400);
    assert.equal(controls.props.style.alignSelf, "center", "transport centers on the artwork with equal left and right space");
    assert.equal(cover.props.style.flex, 1, "cover takes the free space above the anchored controls");
    assert.equal(panel.type, "LyricsPanel");
    assert.equal(panel.props.player, player, "lyrics use the same playback clock and seek callback");
    assert.equal(panel.props.active, visible, "tab hiding suspends follow, tab return resumes the latest clock");
    assert.equal(panel.props.key, selectedUri, "track change cannot keep the previous song's lyrics");
    assert.equal(panel.props.compact, true);
    cover.props.onPress(); assert.deepEqual(calls, ["choose"]);
  }
});

test("tablet lyric clipping boundary aligns to the measured now-playing title without moving the album", () => {
  for (const [headerHeight, titleTop, columnTop] of [[104, 26, 0], [96, 30, 12], [118, 44, 0]]) {
    const clearance = headerHeight + columnTop - titleTop;
    const rowRef = {}, lyricsRef = {}, measure = () => {};
    const tree = mod.exports.TabletLandscapePlayer({ player: {metadata:{}}, visible: true, albumWidth: 560, controlsWidth: 400, horizontalPadding: 32, verticalPadding: 20, topClearance: clearance, rowRef, lyricsRef, onLyricsGeometryChange: measure });
    const [album, lyrics] = childrenOf(tree), [viewport] = childrenOf(lyrics);
    assert.equal(tree.props.ref, rowRef); assert.equal(tree.props.onLayout, measure);
    assert.equal(lyrics.props.ref, lyricsRef); assert.equal(lyrics.props.onLayout, measure);
    assert.equal(viewport.props.style.overflow, "hidden");
    assert.equal(headerHeight + columnTop + viewport.props.style.marginTop, titleTop, "manual scrolling clips at the same row as the title");
    assert.equal(lyrics.props.style.at(-1).paddingTop, 0);
    assert.equal(album.props.style.at(-1).width, 560);
  }
});

test("moving artwork to the left edge preserves its pre-move dimensions with centered controls", () => {
  for (const [width, height] of [[1024, 600], [1133, 650], [1194, 720], [1280, 680], [1366, 920]]) {
    for (const navigationSide of [undefined, "left", "right"]) {
      const layout = tabletImmersivePlayerLayout(width, height, navigationSide);
      const previousViewport = width - (navigationSide ? 192 : 0);
      assert.equal(layout.coverSize, previousViewport / 2 + 48, "left-edge positioning must not zoom or revert to the smaller framed cover");
      assert(layout.controlsWidth <= layout.coverSize - layout.horizontalPadding * 2, "controls leave equal space on either side inside the album width");
      assert.equal(layout.columnWidth + layout.lyricsWidth, width, "lyrics get the remaining width without overlapping playback");
      assert(layout.controlsWidth <= layout.columnWidth);
      assert(layout.controlsWidth > 0);
    }
  }
});

test("permanent lyrics are restricted to physical tablet landscape and do not replace the phone swipe pages", () => {
  const source = readFileSync("apps/mobile/src/IOSPlayer.tsx", "utf8");
  const expression = source.match(/const tabletLandscape = ([^;]+);/)[1];
  const enabled = new Function("tabletDevice", "landscapePlayer", "return " + expression);
  for (const [os, pad, width, height, expected] of [["ios", true, 1194, 834, true], ["android", false, 1280, 800, true], ["ios", true, 834, 1194, false], ["android", false, 800, 1280, false], ["ios", false, 932, 430, false], ["android", false, 932, 430, false]]) {
    assert.equal(enabled(isTabletPlayer(os, pad, width, height), width > height), expected);
  }
  assert.match(source, /tabletLandscape \? <TabletLandscapePlayer/);
  assert.match(source, /tabletSideNavigation && !tabletLandscape && tabletNavigationPageInsets/);
  assert.match(source, /tabletPageInsets=\{tabletLandscape && tabletSideNavigation \?/);
  assert.doesNotMatch(source, /tabletImmersivePageMargin/);
  assert.match(source, /<LandscapeLyricsPager[^>]*enabled=\{landscapePlayer\}/);
  assert.match(source, /<PortraitLyricsPager[^>]*enabled=\{!landscapePlayer\}/);
});
