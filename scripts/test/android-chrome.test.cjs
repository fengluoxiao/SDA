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
assert.match(tabs, /dark=\{props.theme === "dark"\}/);
assert.match(shared, /translucent=\{Platform.OS === "android"\}/);
const activity = fs.readFileSync('apps/mobile/android/app/src/main/java/app/sda/mobile/MainActivity.kt', 'utf8');
assert.match(activity, /WindowCompat.setDecorFitsSystemWindows\(window, false\)/);
assert.match(activity, /window.isStatusBarContrastEnforced = false/);
const chrome = fs.readFileSync('apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/SdaAndroidChromeModule.kt', 'utf8');
assert.doesNotMatch(chrome, /drawRect\(surfaceColor\)|drawRect\(Color\(0x88101318\)\)/);
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
const top = Number(footer.match(/padding\(top = if \(miniOnly\) 0\.dp else (\d+)\.dp/)[1]);
const bottom = Number(footer.match(/bottom = if \(miniOnly\) 0\.dp else (\d+)\.dp/)[1]);
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
  useState:value=>[value,()=>{}],
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
  './AndroidChrome':{hasAndroidChrome:true,AndroidChrome:'AndroidChrome',AndroidPageHeader:'AndroidPageHeader'},
  './android-chrome-model':mod.exports,
};
new Function('require','module','exports',transformSync(tabs,{loader:'tsx',format:'cjs'}).code)(name=>{assert(name in imports,name);return imports[name];},tabsModule,tabsModule.exports);
for(const selectedUri of ['', 'file:///song']) for(const sideBar of [false,true]) for(const sideBarPosition of ['left','right']) {
  const player={selectedUri,playing:true,paused:false,busy:false};
  const pages=[{type:'Player',props:{}},{type:'ScrollView',props:{contentContainerStyle:{paddingTop:8}}},{type:'Scene',props:{}}];
  const tree=tabsModule.exports.PlatformPlayerTabs({player,selected:1,theme:'light',children:pages,sideBar,sideBarPosition});
  const library=tree.props.children[0].props.children[0][1];
  const footerHeight=sideBar?(selectedUri?64:0):androidChromeState(player,1).height;
  assert.equal(library.props.style.paddingBottom,0,'list continues underneath the glass instead of exposing an opaque footer');
  const scroll=library.props.children[0].props.children[0];
  assert.equal(scroll.props.contentContainerStyle[1].paddingBottom,footerHeight+24,'last row can scroll above glass');
  assert.equal(scroll.props.contentContainerStyle[1].paddingTop,64,'first row clears native header');
  assert.equal(scroll.props.scrollIndicatorInsets.top,56);
  if(sideBar)assert(scroll.props.style[1][sideBarPosition==='right'?'marginRight':'marginLeft']>0,'Android viewport itself clears the rail');
  const footer=tree.props.children[1];
  if(sideBar&&!selectedUri)assert.equal(footer,false);
  else assert.equal(footer.props.children[0].props.presentation,sideBar?'mini':'bottom');
}
console.log('Android native layout: persistent pages, floating glass, scroll reachability, left/right rail gutters and mini-only landscape footer.');

// Verify actual native adapters route only valid events, retain explicit theme
// colors and keep disabled glass switches from issuing playback-setting writes.
const chromeModule={exports:{}};
const oldExpo=globalThis.expo;
globalThis.expo={modules:{SdaAndroidChrome:{}}};
new Function('require','module','exports',transformSync(adapter,{loader:'tsx',format:'cjs'}).code)(name=>{
 const values={react,'react-native':{Platform:{OS:'android'}},expo:{requireNativeView:()=> 'NativeChrome'},'./app-theme':{useAppTheme:()=>({id:'sakiko'})},'./android-material-theme':{androidMaterialTheme:()=>({primary:'#7799CC'})},'./android-chrome-model':mod.exports};
 assert(name in values,name);return values[name];
},chromeModule,chromeModule.exports);
globalThis.expo=oldExpo;
for(const dark of [false,true])for(const selected of [0,1,2]) {
 const calls=[];
 const rail=chromeModule.exports.AndroidSideTabs({selected,dark,accent:'#EE5577',onChange:index=>calls.push(index)});
 assert.deepEqual(rail.props.style,{width:48,height:140});
 assert.equal(rail.props.presentation,'rail');assert.equal(rail.props.dark,dark);assert.equal(rail.props.accentColorValue,'#EE5577');
 for(const index of [0,1,2,-1,3,1.5,NaN])rail.props.onSelect({nativeEvent:{index}});
 assert.deepEqual(calls,[0,1,2]);
}
for(const disabled of [false,true]) {
 const writes=[];
 const toggle=chromeModule.exports.AndroidGlassToggle({value:false,onChange:value=>writes.push(value),label:'空间平衡',disabled,dark:false,accent:'#7799CC'});
 toggle.props.onToggle({nativeEvent:{value:true}});toggle.props.onToggle({nativeEvent:{value:'true'}});
 assert.deepEqual(writes,disabled?[]:[true]);
 assert.equal(toggle.props.busy,disabled);
}
const vertical=fs.readFileSync('apps/mobile/modules/sda-core/android/src/main/java/com/kyant/backdrop/catalog/components/LiquidSideTabs.kt','utf8');
assert.match(vertical,/dragAmount.y \/ tabHeight/);assert.doesNotMatch(vertical,/rotate|rotationZ|dragAmount.x/);
assert.match(vertical,/rememberCombinedBackdrop\(backdrop, tabsBackdrop\)/);
assert.match(vertical,/ColorFilter.tint\(accentColor\)/);
assert.match(vertical,/onDragCancelled/);
assert.match(vertical,/Box\(Modifier.fillMaxSize\(\).then\(dampedDragAnimation.modifier\)/,'the whole capsule owns the gesture, including the selected overlay');
console.log('Android glass adapters: upright vertical drag/tint pass, active tab events, theme and disabled-switch routing passed.');

const manifest=fs.readFileSync('apps/mobile/android/app/src/main/AndroidManifest.xml','utf8');
assert.match(manifest,/android:screenOrientation="unspecified"/,'release activity must allow the shared landscape UI');
assert.match(manifest,/android:configChanges="[^"]*orientation[^"]*screenSize/,'rotation keeps the playback activity mounted');
