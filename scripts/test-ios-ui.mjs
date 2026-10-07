import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { build } from 'esbuild';
const result = await build({ entryPoints: ['apps/mobile/src/ios-ui-model.ts'], bundle: true, platform: 'node', format: 'esm', write: false });
const { IOS_TABS, playbackStatus, compatibleRooms, isPresetSelected, trackTitle, iosPlayerLayout } = await import('data:text/javascript;base64,' + Buffer.from(result.outputFiles[0].text).toString('base64'));
assert.deepEqual(IOS_TABS, ['播放', '资料库', '空间']);
assert.equal(trackTitle({name:'file.m4a',metadata:{title:'Song'}}),'Song');
assert.equal(trackTitle({name:'file.m4a',metadata:{}}),'file.m4a');
const ready = {busy:false,preparingAudio:false,playing:false,paused:false,ended:false,selectedUri:'file'};
assert.equal(playbackStatus(ready),'准备就绪');
assert.equal(playbackStatus({...ready,playing:true}),'播放中');
assert.equal(playbackStatus({...ready,playing:true,paused:true}),'已暂停');
assert.equal(playbackStatus({...ready,playing:true,preparingAudio:true}),'正在准备音频…');
assert.equal(playbackStatus({...ready,ended:true}),'播放结束');
const rooms=[{id:'13',layout:'360RA-13'},{id:'12',layout:'7.1.4'}];
assert.deepEqual(compatibleRooms({rooms,layout:'7.1.4',systemSpatial360RAActive:true,systemSpatial360RA:false}),[rooms[0]]);
assert.deepEqual(compatibleRooms({rooms,layout:'7.1.4',systemSpatial360RAActive:true,systemSpatial360RA:true}),[rooms[1]]);
const preset={hrtfSet:'standard',direct:true,directional:true,nearField:false,roomId:'',hrtfWetWeight:.04};
const state={...preset,directObjects:true,directionalObjects:true};
assert.ok(isPresetSelected(state,preset));
assert.ok(!isPresetSelected({...state,directionalObjects:false},preset));
assert.ok(!isPresetSelected({...state,roomId:'13'},preset));
const ui=readFileSync('apps/mobile/src/IOSPlayer.tsx','utf8');
assert.match(ui, /<MobileObjectScene layout={p.layout} objects={p.objects}/);
assert.ok(ui.includes('"gearshape", "更多设置", () => setSettings(true)'));
assert.match(ui, /accessibilityViewIsModal/);
assert.ok(ui.includes('"chevron.left", "返回", () => setSettings(false)'));
assert.match(readFileSync('apps/mobile/App.tsx','utf8'), /copyToCacheDirectory: *true/);
assert.match(readFileSync('apps/mobile/modules/sda-core/ios/GlassControls.swift','utf8'), /["播放", "资料库", "空间"]/);
const tabs=readFileSync('apps/mobile/src/IOSSystemTabs.tsx','utf8');
assert.match(tabs, /TabView.Tab/);
assert.match(tabs, /tabViewStyle\(\{ type: "automatic" \}\)/);
assert.match(tabs, /RNHostView/);
assert.match(tabs, /Platform.OS === "ios"/);
assert.match(tabs, /selection=\{String\(selected\)\}/);
assert.match(ui, /<IOSSystemTabs selected=\{page\} onChange=\{navigate\}/);
assert.doesNotMatch(tabs, /\b(?:stop|pause|resume|play|setSystemSpatial360RA)\(/);
const settings=readFileSync('apps/mobile/src/IOSNativeSettings.tsx','utf8');
assert.match(settings, /largeTitle: false/);
assert.doesNotMatch(settings, /<ToolbarItem/);
assert.match(settings, /headerConfig=\{\{ hidden: true, title \}\}/);
assert.match(ui, /<View style=\{s.header\}>/);
assert.doesNotMatch(ui, /hasNativeIOSNavigation/);
assert.match(settings, /systemImage="gearshape"/);
assert.match(settings, /<Form modifiers=\{\[[\s\S]*?tint\(PlatformColor\("systemBlue"\)\)/);
for (const callback of ['setVolumeBalance', 'setPlaybackMode', 'setSystemSpatial360RA', 'setRenderingPreset', 'setRendering']) assert.ok(settings.includes('p.' + callback));
assert.match(ui, /settings && !hasNativeIOSSettings/);
assert.match(settings, /getViewConfig/);
assert.match(settings, /if \(nativeScreens\)/);
assert.match(settings, /presentationStyle="fullScreen"/);
assert.match(readFileSync('apps/mobile/metro.config.js','utf8'), /moduleName === 'three'/);
assert.match(readFileSync('apps/mobile/src/MobileObjectScene.tsx','utf8'), /quaternion=\{facing.quaternion.toArray\(\)/);
console.log('iOS UI regression checks passed: navigation, status, original 360RA rooms, presets, real scene, safe import');

assert.match(settings, /<IOSSettingsCompatibilityPage player=\{player\}/);
assert.match(settings, /VStack alignment="leading"/);
assert.match(settings, /font\(\{ textStyle: "headline", weight: "semibold" \}\)/);
assert.match(settings, /frame\(\{ width, alignment: "leading" \}\)/);
assert.match(settings, /PlatformColor\("systemGroupedBackground"\)/);
assert.match(settings, /frame\(\{ minHeight: 44 \}\)/);
assert.doesNotMatch(settings, /<HStack><Button label="返回"/);

// Settings backgrounds extend to the screen edge; content keeps native insets.
const compatibilityPage = settings.slice(settings.indexOf('function IOSSettingsCompatibilityPage'), settings.indexOf('function NativeSettingsForm'));
assert.doesNotMatch(compatibilityPage, /<SafeAreaView/);
assert.equal((compatibilityPage.match(/<Host /g) || []).length, 1);
assert.match(compatibilityPage, /ignoresSafeAreaEdges: "all"/);
assert.doesNotMatch(compatibilityPage, /ignoreSafeArea="(?:all|container)"/);
assert.match(settings, /scrollContentBackground\("hidden"\)/);

// The home screen must not clip the native TabView at the bottom safe area
// or show a different-color native page between the top/bottom backgrounds.
assert.match(ui, /SafeAreaProvider initialMetrics=\{initialWindowMetrics\}/);
assert.match(ui, /edges=\{hasSystemIOSTabs \? \["top", "left", "right"\]/);
assert.match(ui, /hasSystemIOSTabs && \{ paddingBottom: 0 \}/);
assert.match(ui, /backgroundColor=\{c.bg\} accent=\{c.accent\}/);
assert.match(tabs, /background\(backgroundColor, \{ ignoresSafeAreaEdges: "all" \}\)/);
assert.match(tabs, /paddingHorizontal: 18, backgroundColor/);

// Never place an engine-error footer between the native tabs and home indicator.
assert.doesNotMatch(ui, /<\/IOSSystemTabs>\s*\{!!p.error/);
assert.ok(ui.indexOf('!!p.error && !engineUnavailable') < ui.indexOf('<IOSSystemTabs selected='));
assert.match(ui, /engineUnavailable && <Text/);
assert.match(tabs, /<VStack spacing=\{0\} modifiers=\{\[background\(backgroundColor, \{ ignoresSafeAreaEdges: "all" \}\)\]\}/);

// UIKit path exposes actual native bar appearances, not just page backgrounds.
const uiKitTabs = readFileSync('apps/mobile/src/IOSUIKitTabs.tsx', 'utf8');
assert.match(tabs, /if \(hasUIKitIOSTabs\) return <IOSUIKitTabs/);
assert.match(uiKitTabs, /UIManager.hasViewManagerConfig\(name\)/);
assert.match(uiKitTabs, /tabBarBackgroundColor: "transparent"/);
assert.match(uiKitTabs, /tabBarBlurEffect: "systemDefault"/);
assert.match(uiKitTabs, /standardAppearance: appearance, scrollEdgeAppearance: appearance/);
assert.match(uiKitTabs, /nativeContainerStyle=\{\{ backgroundColor \}\}/);
assert.match(uiKitTabs, /navStateRequest=\{\{ selectedScreenKey: String\(selected\), baseProvenance: provenance \}\}/);
assert.match(uiKitTabs, /<SafeAreaView edges=\{\["bottom"\]\}/);
assert.doesNotMatch(uiKitTabs, /\b(?:stop|pause|resume|play|setSystemSpatial360RA)\(/);

// Exercise the UI adapter with mocked native components (not a visual/native
// runtime test): appearances, controlled selection, capability-safe imports.
const nativeTabsBundle = await build({ entryPoints: ['apps/mobile/src/IOSUIKitTabs.tsx'], bundle: true, platform: 'node', format: 'cjs', write: false, external: ['react', 'react-native', 'react-native-screens', 'react-native-safe-area-context'] });
function loadNativeTabs(platform, available, version = 26, accessoryAvailable = true) {
  const module = { exports: {} };
  let nativeImports = 0;
  const mockReact = {
    createElement: (type, props, ...children) => ({ type, props: { ...props, children } }),
    useState: () => [0, () => {}],
  };
  const mockRequire = name => {
    if (name === 'react') return mockReact;
    if (name === 'react-native') return { Platform: { OS: platform, Version: version }, UIManager: { hasViewManagerConfig: name => available && (!name.startsWith("RNSTabsBottomAccessory") || accessoryAvailable) }, View: 'View' };
    if (name === 'react-native-safe-area-context') return { SafeAreaProvider: 'SafeAreaProvider', SafeAreaView: 'SafeAreaView' };
    if (name === 'react-native-screens') {
      nativeImports++;
      assert.equal(platform, 'ios');
      assert.ok(available, 'missing native managers must not be imported');
      return { Tabs: { Host: 'UITabBarController', Screen: 'NativeTabScreen' } };
    }
    throw new Error('Unexpected test dependency: ' + name);
  };
  new Function('require', 'module', 'exports', '__DEV__', nativeTabsBundle.outputFiles[0].text)(mockRequire, module, module.exports, false);
  return { api: module.exports, nativeImports };
}
const nativeAdapter = loadNativeTabs('ios', true);
const selections = [];
const tree = nativeAdapter.api.IOSUIKitTabs({ selected: 0, onChange: index => selections.push(index), pages: ['player', 'library', 'scene'], accent: '#28754a', theme: 'light', backgroundColor: '#f0f3f0' });
const tabHost = tree.props.children[0];
assert.deepEqual(tabHost.props.navStateRequest, { selectedScreenKey: '0', baseProvenance: 0 });
for (const screen of tabHost.props.children[0]) {
  assert.equal(screen.props.ios.standardAppearance.tabBarBackgroundColor, 'transparent');
  assert.deepEqual(screen.props.ios.standardAppearance, screen.props.ios.scrollEdgeAppearance);
  assert.equal(screen.props.ios.standardAppearance.tabBarBlurEffect, 'systemDefault');
}
for (const key of ['1', '0', 'oops', '-1', '3', '1.5']) tabHost.props.onTabSelected({ nativeEvent: { selectedScreenKey: key, provenance: 1 } });
assert.deepEqual(selections, [1]);
assert.equal(loadNativeTabs('ios', false).nativeImports, 0);
assert.equal(loadNativeTabs('android', true).nativeImports, 0);
console.log('Native tabs adapter checks passed: transparent bar appearances, controlled selection, guarded native imports');

// Requested cleanup must not reintroduce redundant import/head-direction controls.
assert.doesNotMatch(ui, /folder\.badge\.plus|头向左转|头向右转|重置头向/);
assert.match(ui, /"folder.open", "打开本机文件"/);
assert.match(settings, /Number\(Platform.Version\) >= 26 \? "glass"/);
assert.match(settings, /buttonStyle\(settingsButtonStyle\)/);
assert.match(compatibilityPage, /<HStack spacing=\{0\}/);
assert.match(compatibilityPage, /frame\(\{ width: 88, alignment: "leading" \}\)/);
const nativeActions = settings.slice(settings.indexOf('<Section title="播放操作">'), settings.indexOf('<Section title="播放"'));
assert.match(nativeActions, /<HStack spacing=\{12\}>/);
assert.equal((nativeActions.match(/<Button /g) || []).length, 2);
assert.ok(settings.indexOf('<Section title="播放操作">') < settings.indexOf('<Section title="播放"'));
assert.ok(ui.indexOf('<View style={s.settingsActions}>') < ui.indexOf('{heading("播放")}'));
console.log('Settings layout checks passed: glass style, single-row navigation/actions, no redundant home controls');

// One inline UIKit header owns the settings title and back button.
// The hosted Form must not emit its own large navigation title.
assert.match(settings, /hasUIKitSettingsNavigation = Platform.OS === "ios"/);
assert.match(settings, /"RNSScreen", "RNSScreenStack", "RNSScreenStackHeaderConfig"/);
assert.match(settings, /hasUIKitSettingsNavigation \? require\("react-native-screens"\) : null/);
assert.doesNotMatch(settings, /hasSwiftUISettingsNavigation|IOSSwiftUISettingsPage/);
assert.match(settings, /title: "设置", backButtonDisplayMode: "minimal", backTitleVisible: false/);
assert.match(settings, /onDismissed=\{\(\) => onSettingsChange\(false\)\}/);
assert.match(settings, /screenId="sda-home" activityState=\{2\} freezeOnBlur=\{false\}/);
console.log('Settings native-header checks passed: guarded UIKit stack, system title/back, mounted home');

assert.doesNotMatch(settings, /activityState=\{settings \?/);

const uiKitStack = settings.slice(settings.indexOf('return <ScreenStack style='), settings.indexOf('return <>{children}<Modal'));
assert.equal((uiKitStack.match(/style=\{StyleSheet.absoluteFill\}/g) || []).length, 2);
assert.match(uiKitStack, /contentStyle=\{\{ flex: 1/);
assert.match(uiKitStack, /largeTitle: false/);
assert.doesNotMatch(uiKitStack, /backTitle: "返回"/);
console.log('Settings fullscreen checks passed: mounted home, inline settings title');

assert.match(uiKitStack, /translucent: true/);
assert.match(uiKitStack, /backgroundColor: "transparent",\s*experimental_userInterfaceStyle/);
assert.doesNotMatch(uiKitStack, /scrollEdgeEffects=|blurEffect:/);
assert.match(uiKitStack, /<Host style=\{\{ flex: 1 \}\} colorScheme=\{theme\}>/);
assert.doesNotMatch(uiKitStack, /ignoreSafeArea=/);
assert.match(uiKitStack, /<NativeSettingsForm/);
assert.doesNotMatch(settings, /navigationTitle\(|<NavigationStack|<NavigationDestination/);
assert.doesNotMatch(settings, /scrollEdgeEffectStyle\(|blurEffect:/);
console.log('Settings header checks passed: single inline title, no large Form title or forced material');

// Execute both capability paths rather than only matching source text. The
// title must remain inline regardless of SwiftUI NavigationStack availability.
const settingsBundle = await build({ entryPoints: ['apps/mobile/src/IOSNativeSettings.tsx'], bundle: true, platform: 'node', format: 'cjs', write: false, external: ['react', 'react-native', 'react-native-screens', './IOSSystemTabs', '@expo/ui/swift-ui/modifiers'] });
function settingsAdapter(swiftNavigationAvailable) {
  const module = { exports: {} };
  const previousExpo = globalThis.expo;
  globalThis.expo = { getViewConfig: (_module, name) => !['NavigationStackView', 'SlotView'].includes(name) || swiftNavigationAvailable };
  const react = { createElement: (type, props, ...children) => ({ type, props: { ...props, children } }) };
  const mockRequire = name => {
    if (name === 'react') return react;
    if (name === 'react-native') return { Platform: { OS: 'ios', Version: 26 }, UIManager: { hasViewManagerConfig: () => true }, StyleSheet: { absoluteFill: {} }, PlatformColor: name => name, useWindowDimensions: () => ({ width: 390 }), View: 'View', Modal: 'Modal' };
    if (name === './IOSSystemTabs') return { swiftUI: Object.fromEntries(['Host','NavigationStack','NavigationDestination','Text'].map(name => [name,name])) };
    if (name === 'react-native-screens') return { ScreenStack: 'OuterStack', ScreenStackItem: 'OuterScreen' };
    throw new Error('Unexpected settings test dependency: ' + name);
  };
  try { new Function('require','module','exports','__DEV__',settingsBundle.outputFiles[0].text)(mockRequire,module,module.exports,false); }
  finally { globalThis.expo = previousExpo; }
  return module.exports;
}
for (const available of [true,false]) {
  const adapter = settingsAdapter(available);
  const closed = [];
  const tree = adapter.IOSSettingsNavigation({ children: 'approved-home', settings: true, onSettingsChange: value => closed.push(value), title: '正在播放', player: {}, theme: 'dark' });
  const [home,settingsScreen] = tree.props.children;
  assert.equal(home.props.children[0], 'approved-home');
  assert.equal(settingsScreen.props.headerConfig.title,'设置');
  assert.equal(settingsScreen.props.headerConfig.largeTitle,false);
  assert.notEqual(settingsScreen.props.headerConfig.hidden,true);
  assert.equal(settingsScreen.props.children[0].type,'Host');
  settingsScreen.props.onDismissed();
  assert.deepEqual(closed,[false]);
}
console.log('Settings adapter checks passed: inline native title, no nested large-title stack, native dismissal, unchanged home');

// Only an overflowing library may scroll; playback and scene are fixed Views.
const homePages = ui.slice(ui.indexOf('<IOSSystemTabs selected='), ui.indexOf('</IOSSystemTabs>'));
assert.equal((homePages.match(/<ScrollView /g) || []).length, 1);
assert.match(homePages, /scrollEnabled=\{libraryCanScroll\} bounces=\{false\} alwaysBounceVertical=\{false\}/);
assert.match(ui, /librarySize.content > librarySize.viewport \+ 1/);
assert.match(homePages, /onContentSizeChange=/);
assert.match(homePages, /onLayout=/);
assert.doesNotMatch(ui, /lockScroll|scrollLocks|playerScroll|sceneScroll/);
console.log('Home scrolling checks passed: fixed player/scene, overflow-only library without bounce');

// Responsive layout uses actual viewport and measured controls, distributing
// spare height rather than piling it up below the volume row.
for (const viewport of [410, 520, 650, 780]) {
  for (const blocks of [237, 285, 315]) {
    const layout = iosPlayerLayout(390, viewport, blocks);
    assert.ok(layout.coverSize >= 48 && layout.coverSize <= 260);
    assert.ok(layout.coverSize + blocks + 22 + layout.gap * 4 <= viewport + 1);
  }
}
assert.ok(iosPlayerLayout(390, 700, 237).coverSize > iosPlayerLayout(390, 410, 237).coverSize);
assert.ok(iosPlayerLayout(390, 500, 285).coverSize < iosPlayerLayout(390, 500, 237).coverSize);
assert.match(ui, /justifyContent: "space-between"/);
assert.match(ui, /measurePlayerBlock\("(?:status|info|controls|volume)"/);
assert.match(ui, /<IOSVolumeSymbol volume=\{p.volume\}/);
assert.doesNotMatch(ui, /label\("音量", true/);
const volumeSymbol = readFileSync('apps/mobile/src/IOSVolumeSymbol.tsx', 'utf8');
assert.match(volumeSymbol, /speaker.wave.2.fill/);
assert.match(volumeSymbol, /speaker.slash.fill/);
assert.match(volumeSymbol, /getViewConfig/);
console.log('Player layout checks passed: short/tall viewports, measured notices/metadata, guarded native speaker symbol');

assert.equal(iosPlayerLayout(390, 780, 237).coverSize, 260);
assert.equal(iosPlayerLayout(430, 900, 237).coverSize, 260);

// Standalone-only SDA managers may supply audio, never a different home design.
const sharedTransport = ui.slice(ui.indexOf('  const icon ='), ui.indexOf('  const divider ='));
assert.doesNotMatch(sharedTransport, /hasNativeIOSChrome|IOSIconButton/);
const sharedVolume = ui.slice(ui.indexOf('<View style={[s.volume,'), ui.indexOf('<View style={[s.volume,') + 1100);
assert.doesNotMatch(sharedVolume, /hasNativeIOSChrome|IOSVolumeSlider/);
assert.match(sharedVolume, /accessibilityLabel="音量滑块"/);
console.log('Expo Go/release parity checks passed: shared home header, transport and volume controls');

// Both native accessory environments share actions/state with the main player.
const accessoryProps = { selected: 1, onChange() {}, pages: ['player', 'library', 'scene'], accessory: 'legacy-card', nativeAccessory: environment => 'content-' + environment, accent: '#28754a', theme: 'light', backgroundColor: '#f0f3f0' };
const accessoryHost = adapter => adapter.api.IOSUIKitTabs(accessoryProps).props.children[0];
const systemHost = accessoryHost(nativeAdapter);
assert.equal(systemHost.props.ios.bottomAccessory('regular'), 'content-regular');
assert.equal(systemHost.props.ios.bottomAccessory('inline'), 'content-inline');
assert.equal(systemHost.props.ios.bottomAccessoryHidden, false);
for (const screen of systemHost.props.children[0]) {
  const page = screen.props.children[0].props.children[0];
  assert.equal(page.props.children[1], false, 'native container must replace, not stack with, legacy card');
}
const playerHost = nativeAdapter.api.IOSUIKitTabs({...accessoryProps, selected: 0}).props.children[0];
assert.equal(playerHost.props.ios.bottomAccessoryHidden, true);
const emptyHost = nativeAdapter.api.IOSUIKitTabs({...accessoryProps, nativeAccessory: undefined, accessory: false}).props.children[0];
assert.equal(emptyHost.props.ios.bottomAccessory, undefined);
assert.equal(emptyHost.props.ios.bottomAccessoryHidden, true);
for (const adapter of [loadNativeTabs('ios', true, 25), loadNativeTabs('ios', true, 26, false)]) {
  const fallbackHost = accessoryHost(adapter);
  assert.equal(fallbackHost.props.ios.bottomAccessory, undefined);
  const library = fallbackHost.props.children[0][1].props.children[0].props.children[0];
  assert.equal(library.props.children[1], 'legacy-card');
}
assert.match(ui, /nativeAccessory=\{p.selectedUri \? miniPlayer : undefined\}/);
assert.match(ui, /environment \? s.nativeMiniPlayer/);
console.log('Mini-player checks passed: official accessory, both environments, hidden/empty, old-OS/missing-manager fallback, no double card');

const miniPlayerSource = ui.slice(ui.indexOf('const miniPlayer ='), ui.indexOf('const heading ='));
assert.match(miniPlayerSource, /cover\(environment === "inline" \? 24 : 32\)/);
assert.match(miniPlayerSource, /<IOSPlaybackSymbol playing=\{playing\} color=\{c.accent\}/);
assert.doesNotMatch(miniPlayerSource, /\{icon\(/);
assert.match(ui, /miniPlayback: \{ width: 44, height: 44,[^\n]*backgroundColor: "transparent"/);
const playbackSymbol = readFileSync('apps/mobile/src/IOSPlaybackSymbol.tsx', 'utf8');
assert.match(playbackSymbol, /getViewConfig\?\.\("ExpoUI", "ImageView"\)/);
assert.match(playbackSymbol, /systemName=\{playing \? "pause.fill" : "play.fill"\}/);
assert.match(playbackSymbol, /pointerEvents="none"/);
console.log('Mini-player design checks passed: smaller covers, bare SF Symbol, homepage accent, 44-point target');

assert.match(ui, /<IOSSkipSymbol direction="previous" color=\{c.ink\}/);
assert.match(ui, /<IOSSkipSymbol direction="next" color=\{c.ink\}/);
assert.doesNotMatch(ui, /backward.end.fill|forward.end.fill/);
assert.match(playbackSymbol, /"backward.fill" : "forward.fill"/);
console.log('Transport symbol checks passed: real double-triangle SF Symbols in Go and release');

// Home transport must visibly reflect the same mode as the settings picker.
assert.match(ui, /<IOSModeSymbol mode=\{p.playbackMode\}/);
assert.match(ui, /onPress=\{\(\) => p.setPlaybackMode\(followingPlaybackMode\(p.playbackMode\)\)\}/);
const modeControl = ui.match(/<Pressable[^>]*accessibilityLabel=\{["']播放模式：["'][\s\S]*?<\/Pressable>/)?.[0];
assert.ok(modeControl);
assert.doesNotMatch(modeControl, /<Text/);
assert.match(modeControl, /accessibilityValue=\{\{ text: PLAYBACK_MODE_LABELS\[p.playbackMode\]/);
assert.match(modeControl, /width: 44, height: 44/);
assert.match(ui, /<IOSPlaybackSymbol playing=\{playing\} color=\{c.ink\}/);
assert.doesNotMatch(ui, /icon\(playing \? "pause.fill"/);
assert.match(playbackSymbol, /"repeat.1" : mode === "repeat-all" \? "repeat" : "list.bullet"/);
console.log('Home transport regression checks passed: visible mode feedback and native playback symbols');

// ALAC shares the Apple output transport, not the MPEG-H codec or room fallback.
assert.deepEqual(compatibleRooms({rooms,layout:'7.1.4',systemSpatial360RAActive:true,systemSpatial360RA:false,sourceCodec:'alac'}),[rooms[1]]);
const alacNative = readFileSync('apps/mobile/modules/sda-core/ios/SdaModule.swift','utf8');
for (const method of ['setAlacStereoUpmix','setStereoSystemSpatialAudio']) {
 const line = alacNative.split('\n').find(line => line.includes(`Function("${method}")`));
 assert.ok(line,`${method} is exported`);
 assert.match(line,method === "setAlacStereoUpmix" ? /player\.setAlacStereoUpmix/ : /prefs\.set/);
 assert.doesNotMatch(line,/stopNative|\.play\(|startNative/,'Preference changes must not restart playback');
}
for (const file of ['IOSPlayer.tsx','IOSNativeSettings.tsx']) {
 const ui = readFileSync(`apps/mobile/src/${file}`,'utf8');
 assert.match(ui,/setAlacStereoUpmix/); assert.match(ui,/setSystemSpatialStereo/);
}
const alacReader = readFileSync('apps/mobile/modules/sda-core/ios/CompressedInput.swift','utf8');
assert.match(alacReader,/kAudioFormatAppleLossless/);
assert.match(alacReader,/mChannelsPerFrame == 2/);
assert.match(alacReader,/isAlac \? \[AVFormatIDKey:kAudioFormatLinearPCM/);
console.log('ALAC routing checks passed: distinct preferences, no toggle restart, stereo-only decoding, room isolation');

// ALAC layout must describe the selected route, not default every MP4 to Atmos.
const alacPlayer = readFileSync('apps/mobile/modules/sda-core/ios/SdaPlayer.swift','utf8');
assert.match(alacPlayer,/layout = input\.isAlac \? \(alacUpmixActive \? "7\.1\.4" : "2\.0"\)/);
assert.ok(alacPlayer.indexOf('layout = input.isAlac') < alacPlayer.indexOf('} else { try startNative() }'));
assert.match(alacPlayer,/status\["outputLayout"\] = layout/);
assert.match(readFileSync('apps/mobile/App.tsx','utf8'),/layout: value\.outputLayout \?\? this\.state\.layout/);
assert.match(readFileSync('apps/mobile/src/MobileObjectScene.tsx','utf8'),/LAYOUTS\[layout\]\.map/);
assert.doesNotMatch(readFileSync('apps/mobile/src/IOSPlayer.tsx','utf8'),/"360° 球形声场" : "7\.1\.4"/);
console.log('ALAC 2.0 layout checks passed: engine, status, scene and label');

const liveUpmix = alacPlayer.slice(alacPlayer.indexOf(' func setAlacStereoUpmix('), alacPlayer.indexOf(' func startNative()'));
assert.match(liveUpmix,/alacUpmixActive = enabled/);
assert.match(liveUpmix,/systemSpatial\?\.setAlacUpmix\(enabled\)/);
assert.doesNotMatch(liveUpmix,/stopNative|startNative|setPaused|reset_source|\.play\(/);
console.log('Live ALAC mode setter preserves the active playback session');
