const assert = require('node:assert/strict');
const fs = require('node:fs');
const { transformSync } = require('esbuild');
const mod = { exports: {} };
new Function('module', 'exports', transformSync(fs.readFileSync('apps/mobile/src/android-chrome-model.ts', 'utf8'), { loader: 'ts', format: 'cjs' }).code)(mod, mod.exports);
const { androidChromeState, isAndroidTabIndex, ANDROID_CHROME_TABS } = mod.exports;
assert.deepEqual(ANDROID_CHROME_TABS, ['播放', '资料库', '空间']);
for (const selected of [0, 1, 2]) {
  for (const selectedUri of ['', 'file:///track.m4a']) {
    for (const playing of [false, true]) {
      for (const paused of [false, true]) {
        const state = androidChromeState({ selectedUri, playing, paused, busy: false }, selected);
        assert.equal(state.hasTrack, !!selectedUri);
        assert.equal(state.showMini, !!selectedUri && selected !== 0);
        assert.equal(state.height, state.showMini ? 128 : 74);
        assert.equal(state.playing, playing && !paused);
      }
    }
  }
}
for (const index of [-1, 3, 1.5, NaN, Infinity]) assert.equal(isAndroidTabIndex(index), false);
for (const index of [0, 1, 2]) assert.equal(isAndroidTabIndex(index), true);
const adapter = fs.readFileSync('apps/mobile/src/AndroidChrome.tsx', 'utf8');
assert.match(adapter, /onExpand=\{\(\) => onSelect\(0\)\}/);
assert.match(adapter, /player\.playing \? player\.togglePause\(\) : player\.play\(\)/);
assert.match(adapter, /Platform\.OS === "android"/);
const modules = JSON.parse(fs.readFileSync('apps/mobile/modules/sda-core/expo-module.config.json', 'utf8'));
assert.ok(modules.android.modules.includes('app.sda.mobile.sda.SdaAndroidChromeModule'));
assert.ok(modules.android.modules.includes('app.sda.mobile.sda.SdaAndroidSymbolModule'));
const router = fs.readFileSync('apps/mobile/src/RemotePlayer.tsx', 'utf8');
assert.match(router, /Platform\.OS === "ios" \|\| hasAndroidChrome \? <IOSPlayer/);
const shared = fs.readFileSync('apps/mobile/src/IOSPlayer.tsx', 'utf8');
const tabs = fs.readFileSync('apps/mobile/src/PlatformPlayerTabs.tsx', 'utf8');
assert.match(tabs, /dark=\{props.selected === 0 \|\| props.theme === "dark"\}/);
assert.match(shared, /translucent=\{Platform.OS === "android"\}/);
const activity = fs.readFileSync('apps/mobile/android/app/src/main/java/app/sda/mobile/MainActivity.kt', 'utf8');
assert.match(activity, /WindowCompat.setDecorFitsSystemWindows\(window, false\)/);
assert.match(activity, /window.isStatusBarContrastEnforced = false/);
const chrome = fs.readFileSync('apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/SdaAndroidChromeModule.kt', 'utf8');
assert.doesNotMatch(chrome, /onDrawSurface\s*=|drawRect\(surfaceColor\)|drawRect\(Color\(0x88101318\)\)/);
assert.match(chrome, /blur\(8.dp.toPx\(\)\)/);
assert.match(chrome, /override fun dispatchDraw\(canvas: Canvas\)[\s\S]*?if \(!capturing\) super.dispatchDraw\(canvas\)/);
assert.match(chrome, /drawBackgroundBranch\(root, canvas\)/);
assert.match(chrome, /beginRecording\(width, height\)/);
assert.match(chrome, /drawRenderNode\(node\)/);
assert.match(chrome, /LiquidBottomTabs\(selectedTabIndex = selection/);
assert.match(chrome, /if \(settingsButton\)/);
assert.match(chrome, /view is SdaAndroidChromeView/);
assert.match(chrome, /override fun onWindowFocusChanged/);
assert.match(chrome, /key\(interactionGeneration\) \{ Chrome\(\) \}/);
assert.match(chrome, /LiquidButton\(onClick = \{ onExpand/);
assert.match(chrome, /onClickLabel = if \(playing\) "暂停" else "播放"\) \{ onPlayback/);
const liquidButton = fs.readFileSync('apps/mobile/modules/sda-core/android/src/main/java/com/kyant/backdrop/catalog/components/LiquidButton.kt', 'utf8');
assert.match(liquidButton, /interactiveHighlight.gestureModifier/);
assert.match(liquidButton, /maxDragScale/);
assert.match(chrome, /height = 48.dp, contentPadding = 10.dp,[\s\S]*?modifier = Modifier.fillMaxWidth\(\)/);
assert.doesNotMatch(chrome, /backdrop = miniBackdrop, framed = false/);
assert.doesNotMatch(chrome, /playbackBackdrop|LiquidButton\(onClick = \{ onPlayback/);
assert.match(liquidButton, /if \(framed\).*Highlight.Default else null/);
const liquid = fs.readFileSync('apps/mobile/modules/sda-core/android/src/main/java/com/kyant/backdrop/catalog/components/LiquidBottomTabs.kt', 'utf8');
// Both glass sampling origins and the reserved RN footer must match the
// compact native stack; otherwise controls clip or refract the wrong pixels.
const footer = chrome.slice(chrome.indexOf("val miniBackdrop ="));
const top = Number(footer.match(/padding\(top = (\d+)\.dp/)[1]);
const bottom = Number(footer.match(/padding\(top = \d+\.dp, bottom = (\d+)\.dp/)[1]);
const gap = Number(footer.match(/verticalArrangement = Arrangement.spacedBy\((\d+)\.dp\)/)[1]);
const mini = Number(footer.match(/height = (\d+)\.dp, contentPadding/)[1]);
const tab = Number(chrome.match(/accentColor = accent, height = (\d+)\.dp/)[1]);
const withTrack = { selectedUri: 'test', playing: true, paused: false, busy: false };
assert.equal(androidChromeState(withTrack, 1).height, top + mini + gap + tab + bottom);
assert.equal(androidChromeState(withTrack, 0).height, top + tab + bottom);
const origins = chrome.match(/if \(showMini\) (\d+)\.dp else (\d+)\.dp/);
assert.equal(Number(origins[1]), top + mini + gap);
assert.equal(Number(origins[2]), top);
assert.ok(mini >= 48 && tab - 8 >= 48, 'keep 48dp touch targets');
assert.equal((liquid.match(/\.height\(height - 8\.dp\)/g) || []).length, 2);
assert.match(liquid, /\.height\(height\)/);
assert.match(liquid, /rememberCombinedBackdrop\(backdrop, tabsBackdrop\)/);
assert.match(liquid, /chromaticAberration = true/);
assert.match(liquid, /dampedDragAnimation.scaleX/);
assert.match(liquid, /interactiveHighlight.gestureModifier/);
assert.match(liquid, /InnerShadow\(/);
assert.doesNotMatch(chrome, /Bitmap.createBitmap|SystemClock|postInvalidateDelayed|lastCapture/);
assert.match(chrome, /drawWithContent \{ backgroundRevision; drawContent\(\) \}/);
assert.match(tabs, /position: "absolute", bottom: 0/);
const bridge = fs.readFileSync('apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/SdaModule.kt', 'utf8');
assert.match(bridge, /activeSourceCodec = if \(isAlac\) "alac" else if \(isMpegh\) "mpegh" else if \(isMp3\) "mp3"/);
assert.match(bridge, /equals\("ac4", ignoreCase = true\)\) "ac4"/);
assert.match(bridge, /\.put\("sourceCodec", activeSourceCodec\)/);
assert.match(bridge, /\.put\("outputLayout", if \(activeSourceCodec ===? "alac"/);
assert.match(shared, /PlatformPlayerTabs as IOSSystemTabs/);
assert.match(shared, /BackHandler\.addEventListener\("hardwareBackPress"/);
assert.match(shared, /artwork-color-fade\.png/);
assert.match(shared, /Platform\.OS === "ios" && <>\{heading\("声音增强"\)/);
console.log('Android chrome state matrix, tab validation and adapter wiring passed.');


// Exercise the actual tab wrapper: the library's native scroll viewport must
// end above both forms of the floating footer, not merely pad its last row.
const tabsModule={exports:{}};
const react={
  createElement:(type,props,...children)=>({type,props:{...props,children}}),
  Children:{toArray:value=>value},
  isValidElement:value=>!!value?.props,
  cloneElement:(value,props)=>({...value,props:{...value.props,...props}}),
};
const imports={
  react,
  'react-native':{Platform:{OS:'android'},View:'View'},
  './app-theme':{useAppTheme:()=>({id:'sakiko'})},
  './android-material-theme':{androidMaterialTheme:()=>({bg:'#fff'})},
  './IOSSystemTabs':{hasSystemIOSTabs:false,IOSSystemTabs:'IOSSystemTabs'},
  './AndroidChrome':{hasAndroidChrome:true,AndroidChrome:'AndroidChrome'},
  './android-chrome-model':mod.exports,
};
new Function('require','module','exports',transformSync(tabs,{loader:'tsx',format:'cjs'}).code)(name=>{assert(name in imports,name);return imports[name];},tabsModule,tabsModule.exports);
for(const selectedUri of ['', 'file:///song']) {
  const player={selectedUri,playing:true,paused:false,busy:false};
  const pages=[{type:'Player',props:{}},{type:'ScrollView',props:{contentContainerStyle:{paddingTop:8}}},{type:'Scene',props:{}}];
  const tree=tabsModule.exports.PlatformPlayerTabs({player,selected:1,theme:'light',children:pages});
  const wrappers=tree.props.children[0].props.children[0];
  const library=wrappers[1];
  const footerHeight=androidChromeState(player,1).height;
  assert.equal(library.props.style.paddingBottom,footerHeight);
  const scroll=library.props.children[0];
  assert.equal(scroll.props.contentContainerStyle.at(-1).paddingBottom,20);
  // Reserving the footer in the viewport retains the previous scroll range,
  // while keeping its indicator out of the footer overlay.
  for(const screenHeight of [300,650]) {
    const contentHeight=1000;
    const before=contentHeight+footerHeight+20-screenHeight;
    const after=contentHeight+20-(screenHeight-library.props.style.paddingBottom);
    assert.equal(after,before);
  }
}
console.log('Android library: tab and mini-player viewport reservations preserve the last-row scroll range.');
