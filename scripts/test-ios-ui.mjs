import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { build } from 'esbuild';
const result = await build({ entryPoints: ['apps/mobile/src/ios-ui-model.ts'], bundle: true, platform: 'node', format: 'esm', write: false });
const { IOS_TABS, playbackStatus, compatibleRooms, isPresetSelected, trackTitle, iosPlayerLayout, iosLandscapeControlLayout, landscapeArtworkLayout } = await import('data:text/javascript;base64,' + Buffer.from(result.outputFiles[0].text).toString('base64'));
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
const nativePageSource=readFileSync('apps/mobile/src/IOSNativePage.tsx','utf8');
assert.match(nativePageSource, /largeTitle: false/);
assert.doesNotMatch(settings, /<ToolbarItem/);
assert.match(settings, /headerConfig=\{\{ hidden: true, title \}\}/);
assert.match(ui, /<View style=\{\[s.header, compactLandscape && np\.compactHeader, iosLandscape && np\.iosLandscapeHeader/);
assert.doesNotMatch(ui, /hasNativeIOSNavigation/);
assert.match(settings, /systemImage="gearshape"/);
assert.match(settings, /<Form modifiers=\{\[[\s\S]*?tint\(appTheme.accent\)/);
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
assert.match(settings, /frame\(\{ maxWidth: width, alignment: "leading" \}\)/);
assert.match(settings, /PlatformColor\("systemGroupedBackground"\)/);
assert.match(settings, /frame\(\{ (?:width: 44, )?minHeight: 44 \}\)/);
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
assert.match(ui, /edges=\{nativeLibraryHeaderActive \? \[\] : hasSystemIOSTabs && Platform\.OS === "ios"/);
assert.match(ui, /hasSystemIOSTabs && \{ paddingBottom: 0 \}/);
assert.match(ui, /backgroundColor="transparent" pageBackgroundColors=\{\["transparent", c.bg, c.bg\]\} accent=\{c.accent\}/);
assert.match(tabs, /background\(backgroundColor, \{ ignoresSafeAreaEdges: "all" \}\)/);
assert.match(tabs, /paddingHorizontal: 18, backgroundColor/);

// Never place an engine-error footer between the native tabs and home indicator.
assert.doesNotMatch(ui, /<\/IOSSystemTabs>\s*\{!!p.error/);
assert.ok(ui.indexOf('!!p.error && !engineUnavailable') < ui.indexOf('<IOSSystemTabs selected='));
assert.match(ui, /engineUnavailable && (?:!?nativeLibraryHeaderActive && )?<Text/);
assert.match(tabs, /<VStack spacing=\{0\}>/);

// UIKit path exposes actual native bar appearances, not just page backgrounds.
const uiKitTabs = readFileSync('apps/mobile/src/IOSUIKitTabs.tsx', 'utf8');
assert.match(tabs, /if \(hasUIKitIOSTabs\) return <IOSUIKitTabs/);
assert.match(uiKitTabs, /UIManager.hasViewManagerConfig\(name\)/);
assert.match(uiKitTabs, /tabBarBackgroundColor: "transparent"/);
assert.match(uiKitTabs, /tabBarBlurEffect: "systemDefault"/);
assert.match(uiKitTabs, /standardAppearance: appearance, scrollEdgeAppearance: appearance/);
assert.match(uiKitTabs, /nativeContainerStyle=\{\{ backgroundColor \}\}/);
assert.match(uiKitTabs, /navStateRequest=\{\{ selectedScreenKey: String\(selected\), baseProvenance: provenance \}\}/);
assert.match(uiKitTabs, /edges=\{index === 1 \?/);
assert.doesNotMatch(uiKitTabs, /\b(?:stop|pause|resume|play|setSystemSpatial360RA)\(/);

// Exercise the UI adapter with mocked native components (not a visual/native
// runtime test): appearances, controlled selection, capability-safe imports.
const nativeTabsBundle = await build({ entryPoints: ['apps/mobile/src/IOSUIKitTabs.tsx'], bundle: true, platform: 'node', format: 'cjs', write: false, external: ['react', 'react-native', 'react-native-screens', 'react-native-safe-area-context','./IOSNativePage'] });
function loadNativeTabs(platform, available, version = 26, accessoryAvailable = true, headerHeight = 0) {
  const module = { exports: {} };
  let nativeImports = 0;
  let stateSlot = 0;
  const mockReact = {
    isValidElement: value => !!value && typeof value === "object" && "props" in value,
    cloneElement: (value, props) => ({ ...value, props: { ...value.props, ...props } }),
    createElement: (type, props, ...children) => ({ type, props: { ...props, children } }),
    useState: () => [stateSlot++ === 1 ? headerHeight : 0, () => {}],
  };
  const mockRequire = name => {
    if (name === 'react') return mockReact;
    if (name === 'react-native') return { Platform: { OS: platform, Version: version }, UIManager: { hasViewManagerConfig: name => {
      const known = ["RNSTabsHostIOS", "RNSTabsScreenIOS", "RNSTabsBottomAccessory", "RNSTabsBottomAccessoryContent"];
      if (!known.includes(name)) throw new Error("Exception in HostFunction: unknown native component " + name);
      return available && (!name.startsWith("RNSTabsBottomAccessory") || accessoryAvailable);
    } }, View: 'View' };
    if (name === './IOSNativePage') return {IOSNativePage:'NativePage',hasNativePageHeader:true};
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
function pageViewport(screen) {
  const page=screen.props.children[0];
  const provider=page.type==='NativePage'?page.props.children[0]:page;
  return provider.props.children[0];
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
  const safeViewport = pageViewport(screen);
  assert.deepEqual(safeViewport.props.edges, screen.props.screenKey === "1" ? ["left","right"] : ["bottom"]);
  assert.equal(safeViewport.props.style.backgroundColor, "transparent");
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
assert.match(settings, /hasUIKitSettingsNavigation = hasNativePageHeader/);
assert.match(nativePageSource, /"RNSScreen", "RNSScreenStack", "RNSScreenStackHeaderConfig"/);
assert.match(settings, /hasUIKitSettingsNavigation \? require\("react-native-screens"\) : null/);
assert.doesNotMatch(settings, /hasSwiftUISettingsNavigation|IOSSwiftUISettingsPage/);
assert.match(settings, /<IOSNativePage screenId="sda-settings-content" title="设置"/);
assert.match(settings, /onDismissed=\{\(\) => onSettingsChange\(false\)\}/);
assert.match(settings, /screenId="sda-home" activityState=\{2\} freezeOnBlur=\{false\}/);
console.log('Settings native-header checks passed: guarded UIKit stack, system title/back, mounted home');

assert.doesNotMatch(settings, /activityState=\{settings \?/);

const uiKitStack = settings.slice(settings.indexOf('return <ScreenStack style='), settings.indexOf('return <>{children}<Modal'));
assert.equal((uiKitStack.match(/style=\{StyleSheet.absoluteFill\}/g) || []).length, 2);
assert.match(uiKitStack, /contentStyle=\{\{ flex: 1/);
assert.match(nativePageSource, /largeTitle: false/);
assert.doesNotMatch(uiKitStack, /backTitle: "返回"/);
console.log('Settings fullscreen checks passed: mounted home, inline settings title');

assert.match(nativePageSource, /translucent: true/);
assert.match(nativePageSource, /backgroundColor: "transparent"/);
assert.doesNotMatch(uiKitStack, /scrollEdgeEffects=|blurEffect:/);
assert.match(uiKitStack, /<Host style=\{\{ flex: 1 \}\} colorScheme=\{theme\}>/);

assert.match(uiKitStack, /<NativeSettingsForm/);
assert.doesNotMatch(settings, /navigationTitle\(|<NavigationStack|<NavigationDestination/);
assert.doesNotMatch(settings, /scrollEdgeEffectStyle\(|blurEffect:/);
console.log('Settings header checks passed: single inline title, no large Form title or forced material');

// Execute both capability paths rather than only matching source text. The
// title must remain inline regardless of SwiftUI NavigationStack availability.
const settingsBundle = await build({ entryPoints: ['apps/mobile/src/IOSNativeSettings.tsx'], bundle: true, platform: 'node', format: 'cjs', write: false, external: ['react', 'react-native', 'react-native-screens', './IOSSystemTabs', './IOSNativePage', './app-theme', './navigation-preferences', '@expo/ui/swift-ui/modifiers'] });
function settingsAdapter(swiftNavigationAvailable, dimensions = {width:390,height:844}) {
  const module = { exports: {} };
  const previousExpo = globalThis.expo;
  globalThis.expo = { getViewConfig: (_module, name) => !['NavigationStackView', 'SlotView'].includes(name) || swiftNavigationAvailable };
  const react = { createElement: (type, props, ...children) => ({ type, props: { ...props, children } }), useRef: current => ({current}) };
  const mockRequire = name => {
    if (name === 'react') return react;
    if (name === 'react-native') return { Animated: { Value: class {constructor(value){this.value=value;}}, timing:(value,config)=>({start(){assert.equal(config.useNativeDriver,true);value.value=config.toValue;}}) }, Platform: { OS: 'ios', Version: 26 }, UIManager: { hasViewManagerConfig: () => true }, StyleSheet: { absoluteFill: {} }, PlatformColor: name => name, useWindowDimensions: () => dimensions, View: 'View', Modal: 'Modal' };
    if (name === './navigation-preferences') return { useNavigationPreferences: () => ({ landscapeSideTabs: false, setLandscapeSideTabs() {} }) };
    if (name === './app-theme') return { useAppTheme: () => ({id:'sakiko',accent:'#7799CC',setTheme() {}}), CHARACTER_THEMES: [], validTheme: () => true };
    if (name === './IOSNativePage') return {IOSNativePage:'NativePage',hasNativePageHeader:true};
    if (name === './IOSSystemTabs') return { swiftUI: Object.fromEntries(['Host','NavigationStack','NavigationDestination','Text','VStack','ZStack','HStack','Button','RNHostView'].map(name => [name,name])) };
    if (name === '@expo/ui/swift-ui/modifiers') return new Proxy({}, {get: (_,name)=>(...args)=>({name,args})});
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
  assert.equal(settingsScreen.props.headerConfig.hidden,true);
  const nativePage=settingsScreen.props.children[0];
  assert.equal(nativePage.type,'NativePage');assert.equal(nativePage.props.title,'设置');
  assert.equal(nativePage.props.children[0].type,'Host');
  settingsScreen.props.onDismissed();
  assert.deepEqual(closed,[false]);
}
console.log('Settings adapter checks passed: inline native title, no nested large-title stack, native dismissal, unchanged home');

// Native ScrollView decides overflow including adjusted insets; never disable
// gestures using a full-screen measurement that ignores the floating accessory.
const homePages = ui.slice(ui.indexOf('<IOSSystemTabs '), ui.indexOf('</IOSSystemTabs>'));
assert.equal((homePages.match(/<ScrollView /g) || []).length, 1);
assert.match(homePages, /scrollEnabled bounces=\{false\} alwaysBounceVertical=\{false\}/);
assert.match(homePages, /contentInsetAdjustmentBehavior="automatic"/);
assert.doesNotMatch(ui, /libraryCanScroll|setLibrarySize/);
assert.doesNotMatch(ui, /lockScroll|scrollLocks|playerScroll|sceneScroll/);
console.log('Home scrolling checks passed: fixed player/scene, inset-aware native library scrolling without measurement lock or bounce');

// Responsive layout uses actual viewport and measured controls, distributing
// spare height rather than piling it up below the volume row.
for (const viewport of [410, 520, 650, 780]) {
  for (const blocks of [237, 285, 315]) {
    const layout = iosPlayerLayout(390, viewport, blocks);
    assert.ok(layout.coverSize >= 48 && layout.coverSize <= 340);
    assert.ok(layout.coverSize + blocks + 44 + layout.gap * 5 <= viewport + 1);
  }
}
assert.ok(iosPlayerLayout(390, 700, 237).coverSize > iosPlayerLayout(390, 410, 237).coverSize);
assert.ok(iosPlayerLayout(390, 500, 285).coverSize < iosPlayerLayout(390, 500, 237).coverSize);
assert.match(ui, /justifyContent: "flex-start"/);
assert.match(ui, /marginHorizontal: -18, overflow: "hidden"/);
assert.match(ui, /measurePlayerBlock\("(?:status|info|controls|volume)"/);
assert.match(ui, /name="speaker.wave.2.fill"/);
assert.doesNotMatch(ui, /label\("音量", true/);
const volumeSymbol = readFileSync('apps/mobile/src/IOSVolumeSymbol.tsx', 'utf8');
assert.match(volumeSymbol, /speaker.wave.2.fill/);
assert.match(volumeSymbol, /speaker.slash.fill/);
assert.match(volumeSymbol, /getViewConfig/);
console.log('Player layout checks passed: short/tall viewports, measured notices/metadata, guarded native speaker symbol');

assert.equal(iosPlayerLayout(390, 780, 237).coverSize, 326);
assert.equal(iosPlayerLayout(430, 900, 237).coverSize, 340);

// Standalone-only SDA managers may supply audio, never a different home design.
const sharedTransport = ui.slice(ui.indexOf('  const icon ='), ui.indexOf('  const divider ='));
assert.doesNotMatch(sharedTransport, /hasNativeIOSChrome|IOSIconButton/);
const sharedVolume = ui.slice(ui.indexOf('<View style={[np.volume, np.controlsDown,'), ui.indexOf('<View style={[np.volume, np.controlsDown,') + 1800);
assert.doesNotMatch(sharedVolume, /hasNativeIOSChrome|IOSVolumeSlider/);
assert.match(sharedVolume, /accessibilityRole="adjustable"/);
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
  const page = pageViewport(screen);
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
  const library = pageViewport(fallbackHost.props.children[0][1]);
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

assert.match(ui, /<IOSSkipSymbol direction="previous" color=\{pc.ink\}/);
assert.match(ui, /<IOSSkipSymbol direction="next" color=\{pc.ink\}/);
assert.doesNotMatch(ui, /backward.end.fill|forward.end.fill/);
assert.match(playbackSymbol, /"backward.fill" : "forward.fill"/);
console.log('Transport symbol checks passed: real double-triangle SF Symbols in Go and release');

// Mode remains a real action in the existing player's more menu.
assert.match(ui, /播放模式：/);
assert.match(ui, /p.setPlaybackMode\(followingPlaybackMode\(p.playbackMode\)\)/);
assert.match(ui, /<IOSPlaybackSymbol playing=\{playing\} color=\{pc.ink\} size=\{38\}/);
console.log('Home transport checks passed: original callbacks, more-menu mode, library queue navigation');

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

// Restyle the existing first tab, never introduce a separate playback route.
assert.doesNotMatch(ui, /IOSNowPlaying|setNowPlaying|presentationStyle="fullScreen"/);
assert.match(homePages, /s.page, np.root/);
assert.match(homePages, /onPress=\{p.previous\}/);
assert.match(homePages, /onPress=\{p.next\}/);
assert.match(homePages, /当前不支持拖动定位/);
assert.match(homePages, /onAccessibilityAction=/);
console.log('Existing playback tab checks passed: page background, reference layout, original navigation, no new screen/modal');

assert.ok(iosPlayerLayout(390, 780, 237).coverSize > 260);


assert.doesNotMatch(homePages, /np.footer|歌词（尚未支持）|选择输出设备（尚未支持）|待播清单/);

assert.doesNotMatch(ui, /lowerPlaybackGroup|marginVertical: playerGap/);
assert.match(ui, /androidTabletPortrait = Platform\.OS === "android" && width >= 600 && height > width/);
assert.match(ui, /androidTabletPortrait && np\.tabletControls/);
assert.match(ui, /tabletControls: \{ marginTop: "auto" \}/);
for (const height of [520, 650, 780]) {
 const layout = iosPlayerLayout(390, height, 236);
 assert.ok(layout.gap >= 0 && layout.gap <= 48);
 const unused = height - (layout.coverSize + 236 + 44 + layout.gap * 5);
 assert.ok(unused >= -1 && unused < 24, `unexpected empty bottom: ${unused}`);
}

assert.match(ui, /marginTop: transportMargin/);
assert.match(ui, /layout.height - transportMargin/);

assert.equal((homePages.match(/np.infoProgressOffset/g) || []).length, 1);
assert.match(ui, /infoProgressOffset: \{ transform: \[\{ translateY: 12 \}\]/);



assert.doesNotMatch(homePages, /blurRadius|immersive-default|immersive &&/);

assert.match(ui, /const homeBackground = immersiveHome \? "transparent" : c.bg/);
assert.doesNotMatch(homePages, /np.coverImage|np.emptyCover/);


assert.match(ui, /backgroundColor=\{Platform\.OS === "android" \? "transparent" : homeBackground\}/);
assert.match(ui, /immersiveHome && <View pointerEvents="none"/);


assert.match(ui, /backgroundColor: fadeColor/);
assert.match(ui, /artworkColor\(artworkUri\)/);

assert.match(ui, /width: artworkSize, height: artworkSize/);
assert.equal((homePages.match(/np.controlsDown/g) || []).length, 2);

assert.match(ui, /const artworkSize = landscapePlayer \? artworkLayout.size : width \+ 48/);
assert.match(ui, /landscapePlayer = width > height && \(Platform\.OS === "ios" \|\| \(Platform\.OS === "android" && height >= 600\)\)/);
const useLandscape = new Function('Platform', 'width', 'height', 'return ' + ui.match(/const landscapePlayer = ([^;]+);/)[1]);
for (const [os, width, height, expected] of [
 ['ios',1194,834,true], ['ios',1366,1024,true], ['android',1097,617,true],
 ['ios',844,390,true], ['android',915,412,false], ['ios',834,1194,false], ['web',1194,834,false],
]) assert.equal(useLandscape({OS:os}, width, height), expected, `${os} ${width}x${height}`);
const expoConfig = JSON.parse(readFileSync('apps/mobile/app.json','utf8')).expo;
const { setOrientation } = await import('@expo/config-plugins/build/ios/Orientation.js');
assert.equal(expoConfig.orientation, 'default');
// Expo's explicit Info.plist property guard preserves these per-device masks.
const generatedOrientation = { ...setOrientation(expoConfig, {}), ...expoConfig.ios.infoPlist };
assert.deepEqual(generatedOrientation.UISupportedInterfaceOrientations, ['UIInterfaceOrientationPortrait', 'UIInterfaceOrientationLandscapeLeft', 'UIInterfaceOrientationLandscapeRight']);
assert.deepEqual(generatedOrientation['UISupportedInterfaceOrientations~ipad'], [
 'UIInterfaceOrientationPortrait', 'UIInterfaceOrientationPortraitUpsideDown',
 'UIInterfaceOrientationLandscapeLeft', 'UIInterfaceOrientationLandscapeRight',
]);

assert.match(ui, /landscapeContent: \{ flexDirection: "row"/);
assert.match(ui, /landscapePlayer && np\.landscapeControls/);
const androidActivity = readFileSync('apps/mobile/android/app/src/main/java/app/sda/mobile/MainActivity.kt', 'utf8');
assert.match(androidActivity, /smallestScreenWidthDp >= 600[\s\S]*?SCREEN_ORIENTATION_FULL_USER else ActivityInfo\.SCREEN_ORIENTATION_PORTRAIT/);


const metadataSheet = readFileSync("apps/mobile/src/IOSMetadataSheet.tsx", "utf8");
assert.match(metadataSheet, /presentationDetents\(\["medium", "large"\]\)/);
assert.match(metadataSheet, /presentationDragIndicator\("visible"\)/);
assert.match(metadataSheet, /onIsPresentedChange=\{onChange\}/);
assert.match(ui, /onPress=\{\(\) => setMetadataOpen\(true\)\}/);

assert.doesNotMatch(homePages, /收藏（尚未支持）|更多歌曲选项/);


assert.match(homePages, /onPress=\{\(\) => p.setPlaybackMode\(mode\)\}/);
assert.match(homePages, /selected = p.playbackMode === mode/);

assert.doesNotMatch(ui, /images.unsplash.com|previewArtwork/);

assert.doesNotMatch(ui, /bandTop|length: 64|length: 8/);

assert.match(ui, /const immersiveHome = page === 0;/);
assert.doesNotMatch(ui, /const immersiveHome = page === 0 && !settings/);

assert.doesNotMatch(ui, /artworkTransition|transitionArtwork/);
assert.match(ui, /tintColor: fadeColor/);

assert.match(ui, /p.busy \|\| p.preparingAudio \? "正在准备音频…"/);
assert.match(ui, /label\(homeTitle, false, \[s.pageTitle, \{ color: immersiveHome \? pc.ink : c.ink \}\]\)/);
assert.doesNotMatch(homePages, /正在准备音频/);
assert.match(ui, /PlatformColor\("label"\), fontSize: 13, fontWeight: "600"/);

assert.doesNotMatch(ui, /rgba\(255,255,255,.55\)|rgba\(0,0,0,.45\)/);

assert.match(readFileSync("crates/sda-native/src/ios.rs", "utf8"), /status\["sourceCodec"\] = json!\(e.codec_name\(\)\)/);
assert.doesNotMatch(readFileSync("apps/mobile/modules/sda-core/ios/SdaPlayer.swift", "utf8"), /layout == "360RA-13" \? "mpegh" : ""/);

assert.match(ui, /animated=\{false\} barStyle=\{settings \?/);
assert.match(metadataSheet, /buttonStyle\(Platform.OS === "ios" && Number\(Platform.Version\) >= 26 \? "glass"/);
assert.match(readFileSync('apps/mobile/src/IOSUIKitTabs.tsx','utf8'), /pageBackgroundColors\?\.\[index\] \?\? backgroundColor/);

assert.doesNotMatch(readFileSync("apps/mobile/src/IOSNativeSettings.tsx", "utf8"), /statusBarStyle=|statusBarAnimation=/);

assert.match(homePages, /size=\{22\} theme=\{selected \? "dark" : isLight \? "light" : "dark"\}/);
assert.doesNotMatch(ui, /color.startsWith/);

assert.match(uiKitTabs, /experimental_userInterfaceStyle: theme,/);
assert.doesNotMatch(uiKitTabs, /index === 0 \? "dark"/);

assert.match(homePages, /color=\{selected \? "#ffffff" : c.ink\}/);


assert.match(metadataSheet, /pointerEvents="none"/);
assert.match(metadataSheet, /color: accent, fontSize: 17/);

assert.match(metadataSheet, /<NativeText modifiers=\{\[frame\(\{ width: 44, height: 28 \}\), opacity\(0\)\]\}/);
assert.doesNotMatch(metadataSheet, /label=" "/);

assert.doesNotMatch(ui, /barStyle=\{settings \|\| metadataOpen/);

assert.match(uiKitTabs, /paddingBottom: floatingLibraryAccessory \? 76 : 24/);

assert.match(ui, /const previewMetadata = false;/);
assert.doesNotMatch(ui, /mockQueue|mock:\/\//);

assert.match(ui, /AppThemeProvider/);
assert.match(settings, /角色代表色/);

// Empty-song metadata, wrapped titles and compact landscape screens must not
// trade control height for whitespace or push the volume row below the page.
for (const viewport of [180, 220, 280, 350, 520]) {
  for (const intrinsic of [150, 178, 210, 250]) {
    const spacing = iosLandscapeControlLayout(viewport, intrinsic);
    assert(Object.values(spacing).every(value => Number.isFinite(value) && value >= 0));
    const used = Object.values(spacing).reduce((sum, value) => sum + value, intrinsic);
    if (viewport >= intrinsic) assert(used <= viewport + 1e-8);
  }
}
console.log('Landscape control spacing preserves measured blocks and bounded gaps within the viewport.');

// Native pages stay in the same Tabs.Host when side navigation is toggled;
// only bar visibility changes; the rail overlays unchanged content width. Keep one usable mini-player.
for (const sideBar of [undefined, 'side-rail']) {
  const root=nativeAdapter.api.IOSUIKitTabs({...accessoryProps,sideBar,sideBarWidth:48});
  const host=root.props.children[0];
  assert.equal(root.props.style.paddingLeft,undefined,"overlay rail must not resize page content");
  assert.equal(host.props.tabBarHidden,!!sideBar);
  assert.equal(host.props.navStateRequest.selectedScreenKey,'1');
  const library=pageViewport(host.props.children[0][1]);
  if(sideBar) {
    assert.equal(host.props.ios.bottomAccessory,undefined);
    assert.equal(host.props.ios.bottomAccessoryHidden,true);
    assert.equal(library.props.children[0].props.children[1].props.children[0],'legacy-card');
    const dock=root.props.children[1];
    assert.deepEqual(dock.props.edges,["bottom"]);
    assert.equal(dock.props.style.justifyContent,"flex-end");
    assert.equal(dock.props.style.alignItems,"flex-end");
    assert.equal(dock.props.style.paddingBottom,8);
    assert.equal(dock.props.style.paddingRight,0);
    assert.equal(library.props.children[0].props.children[1].props.style.marginRight,38);
  } else assert.equal(host.props.ios.bottomAccessory('regular'),'content-regular');
}
console.log('Optional side navigation: retained native pages, hidden bottom bar and single mini-player passed.');

for(const [width,height,usable] of [[844,390,726],[1194,834,1194],[1097,617,1049],[932,430,814]]) {
  const frame=landscapeArtworkLayout(width,height,usable);
  assert.equal(frame.columnWidth*2,usable);
  assert.equal(frame.size,height);
  assert.equal(frame.left,0);
}
console.log('Landscape columns: equal measured halves and edge-aligned full-height square artwork passed.');

// External docking must hide the bottom bar without drawing a duplicate rail.
const externalDockTree = nativeAdapter.api.IOSUIKitTabs({...accessoryProps, sideBar:true, sideBarExternal:true});
assert.equal(externalDockTree.props.children[0].props.tabBarHidden,true);
assert.equal(externalDockTree.props.children[1],false);
console.log('External landscape dock: one rail, native bottom bar hidden.');

const leftDockTree=nativeAdapter.api.IOSUIKitTabs({...accessoryProps,sideBar:true,sideBarExternal:true,sideBarPosition:'left'});
const leftDockLibrary=pageViewport(leftDockTree.props.children[0].props.children[0][1]);
assert.equal(leftDockLibrary.props.children[0].props.children[1].props.style.marginLeft,38);
assert.equal(leftDockLibrary.props.children[0].props.children[1].props.style.marginRight,undefined);
assert.equal(leftDockLibrary.props.children[0].props.children[1].props.style.right,12,'left library mini-player aligns with the dedicated scrollbar gutter');
console.log('Left dock: mini-player clearance follows the rail without a right-side reservation.');

const leftPageInsets=leftDockLibrary.props.style;
assert.equal(leftPageInsets.paddingLeft,30);assert.equal(leftPageInsets.paddingRight,6);
assert.equal(leftPageInsets.paddingLeft+leftPageInsets.paddingRight,36,'move the page right without shrinking it');
console.log('Left rail stays fixed while page content moves 12 pt right using spare right padding.');

// Expo Go can throw from capability detection itself, rather than returning false.
// The strict UIManager mock above reproduces this for unknown native managers.
assert.doesNotMatch(uiKitTabs,/RNSScrollViewMarker|react-native-screens\/experimental/);
const scrollFixture={type:'ScrollView',props:{contentContainerStyle:{paddingTop:8}}};
const compatibleTree=nativeAdapter.api.IOSUIKitTabs({...accessoryProps,pages:['player',scrollFixture,'scene']});
const compatibleScroll=pageViewport(compatibleTree.props.children[0].props.children[0][1]).props.children[0];
assert.equal(compatibleScroll.type,'ScrollView');
assert.deepEqual(compatibleScroll.props.contentContainerStyle,[{paddingTop:8},{paddingBottom:24}]);
console.log('Expo Go startup: no experimental component probe/import; normal library scrolling and tail padding retained.');

for (const dimensions of [{width:390,height:844},{width:932,height:430},{width:844,height:390}]) {
  const closed=[];const adapter=settingsAdapter(true,dimensions);
  const navigation=adapter.IOSSettingsNavigation({children:'approved-home',settings:true,onSettingsChange:v=>closed.push(v),title:'正在播放',player:{},theme:'light',accent:'#7799CC'});
  const screen=navigation.props.children[1];
  assert.equal(screen.props.stackAnimation,undefined);
  assert.deepEqual(screen.props.headerConfig,{hidden:true},'transition stack never changes its own header visibility');
  const page=screen.props.children[0];assert.equal(page.type,'NativePage');assert.equal(page.props.title,'设置');
  assert.equal(page.props.children[0].type,'Host');
  page.props.headerLeft.props.onPress();assert.deepEqual(closed,[false]);
}
for(const [adapter,props,expected] of [
  [nativeAdapter,accessoryProps,["left","right"]],
  [nativeAdapter,{...accessoryProps,accessory:false,nativeAccessory:undefined},["left","right"]],
  [nativeAdapter,{...accessoryProps,sideBar:true},['left','right','bottom']],
  [loadNativeTabs('ios',true,26,false),accessoryProps,['left','right','bottom']],
]) {
  const root=adapter.api.IOSUIKitTabs(props);
  const viewport=pageViewport(root.props.children[0].props.children[0][1]);
  assert.deepEqual(viewport.props.edges,expected);
}
console.log('Settings: native page-owned bar in both orientations; library underlays native bars while side/fallback layouts keep bottom protection.');

const floatingTree=nativeAdapter.api.IOSUIKitTabs({...accessoryProps,pages:['player',scrollFixture,'scene'],sideBar:true});
const floatingViewport=pageViewport(floatingTree.props.children[0].props.children[0][1]);
const floatingContent=floatingViewport.props.children[0];
assert.equal(floatingViewport.props.children[1],false,'no second in-flow accessory');
assert.equal(floatingContent.props.children[1].props.style.position,'absolute');
assert.deepEqual(floatingContent.props.children[0].props.contentContainerStyle,[{paddingTop:8},{paddingBottom:76}]);
console.log('Landscape library accessory overlays rows with scrollable tail clearance, no opaque footer area.');

const measuredAdapter=loadNativeTabs('ios',true,26,true,78);
const measuredScroll={type:'ScrollView',props:{contentContainerStyle:{paddingTop:8},automaticallyAdjustsScrollIndicatorInsets:true,scrollIndicatorInsets:{top:10,right:62,bottom:62,left:0}}};
const measuredTree=measuredAdapter.api.IOSUIKitTabs({...accessoryProps,pages:['player',measuredScroll,'scene'],sideBar:true,sideBarPosition:'left'});
const measuredViewport=pageViewport(measuredTree.props.children[0].props.children[0][1]);
const measuredResult=measuredViewport.props.children[0].props.children[0];
assert.equal(measuredResult.props.automaticallyAdjustsScrollIndicatorInsets,false);
assert.deepEqual(measuredResult.props.scrollIndicatorInsets,{top:88,right:62,bottom:62,left:0});
const rightMeasured=loadNativeTabs('ios',true,26,true,78).api.IOSUIKitTabs({...accessoryProps,pages:['player',measuredScroll,'scene'],sideBar:true,sideBarPosition:'right'});
const rightMeasuredScroll=pageViewport(rightMeasured.props.children[0].props.children[0][1]).props.children[0].props.children[0];
assert.equal(rightMeasuredScroll.props.automaticallyAdjustsScrollIndicatorInsets,true);
assert.equal(rightMeasuredScroll.props.scrollIndicatorInsets,measuredScroll.props.scrollIndicatorInsets);
console.log('Extended left viewport uses measured native header safety once; accepted right indicator handling is untouched.');
