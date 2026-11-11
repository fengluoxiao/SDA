import assert from "node:assert/strict";
import { test } from "node:test";
import { createRequire } from "node:module";
import { buildSync } from "esbuild";
const require = createRequire(import.meta.url);
function moduleFor(path, mocks = {}) {
  const result = buildSync({ entryPoints: [path], bundle: true, platform: "node", format: "cjs", write: false, external: ["react", "react-native"] });
  const module = { exports: {} };
  new Function("require", "module", "exports", result.outputFiles[0].text)(name => mocks[name] ?? require(name), module, module.exports);
  return module.exports;
}
const model = moduleFor("apps/mobile/src/space-visual-model.ts");
test("inactive objects never acquire a visible glow and reduced motion has no decorative oscillation", () => {
  for (const phase of [-10, 0, .5, 10, NaN]) assert.equal(model.spaceGlowOpacity(0, phase, false), 0);
  assert.equal(model.spaceGlowOpacity(1, 0, true), model.spaceGlowOpacity(1, 9, true));
  assert.equal(model.spaceGlowOpacity(NaN, 0, false), 0);
  assert(model.spaceGlowOpacity(.12, 3, false) < model.spaceGlowOpacity(1, 3, false));
});
test("colors remain deterministic for every supported object id", () => {
  for (const id of [NaN, -1, ...Array.from({length:128}, (_, i) => i)]) {
    assert.match(model.spaceObjectColor(id), /^#[0-9a-f]{6}$/i);
    assert.equal(model.spaceObjectColor(id), model.spaceObjectColor(id));
  }
});
let measuredSize = { width: 430, height: 932 };
const react = { useState: () => [measuredSize, () => {}], createElement: (type, props, ...children) => ({ type, props: { ...props, children } }) };
const native = { View: "View", Text: "Text", Image: "Image", Pressable: "Pressable", StyleSheet: { absoluteFill: {}, create: value => value } };
const { SpaceVisualPage } = moduleFor("apps/mobile/src/SpaceVisualPage.tsx", { react, "react-native": native });
const nodes = tree => !tree || typeof tree !== "object" ? [] : Array.isArray(tree) ? tree.flatMap(nodes) : [tree, ...nodes(tree.props?.children)];
const canvas = { type: "existing-scene", props: { marker: "same-native-canvas" } };
const props = { enabled: false, title: "当前歌曲", artist: "歌手", accent: "#7799CC", ink: "#111111", muted: "#888888", layout: "7.1.4", objectCount: 16, positionMs: 30000, durationMs: 90000, header: "header", footer: "existing-controls", viewportStyle: { flex: 1, maxHeight: 410 }, children: canvas };
test("switching appearance retains one existing Canvas in the same viewport tree position", () => {
  const classic = SpaceVisualPage(props), immersive = SpaceVisualPage({...props, enabled:true});
  const viewport = tree => nodes(tree).find(n => n.props?.testID === "space-scene-viewport");
  assert.equal(viewport(classic).props.children[0], canvas);assert.equal(viewport(immersive).props.children[0], canvas);
  assert.equal(classic.props.children[1].props.testID, immersive.props.children[1].props.testID);
  for (const tree of [classic, immersive]) assert.equal(nodes(tree).filter(n => n.type === "existing-scene").length, 1);
  assert.equal(nodes(classic).some(n=>n.props?.testID==='space-track-identity'), false);
  assert(nodes(immersive).some(n=>n.props?.testID==='space-track-identity'));
});
test("portrait, compact landscape and empty metadata have finite layout and real object counts", () => {
  for (const compact of [false, true]) for (const count of [0, 16, 128]) {
    const tree = SpaceVisualPage({...props, enabled:true, compact, objectCount:count, positionMs:NaN, durationMs:0});
    assert.equal(nodes(tree).filter(n=>n.type==='existing-scene').length, 1);
    const texts=nodes(tree).filter(n=>n.type==='Text').flatMap(n=>n.props.children);
    assert(texts.includes(String(count).padStart(2,'0')));
    assert(!JSON.stringify(tree).includes('NaN%'));
  }
});

test("tall viewports fit horizontally without discarding the user's pinch-zoom ratio",()=>{
 const portrait=model.spaceCameraDistance(7,430,932),landscape=model.spaceCameraDistance(7,932,430);
 assert(portrait>landscape);assert.equal(model.spaceCameraDistance(3.5,430,932),portrait/2);
 for(const [w,h]of [[0,0],[NaN,400],[400,Infinity]])assert.equal(model.spaceCameraDistance(7,w,h),7);
 assert(Number.isFinite(model.spaceCameraDistance(NaN,430,932)));
});

test("widescreen puts information beside the stage while small/portrait viewports retain a single column",()=>{
 assert(model.spaceVisualColumns(900,300));assert(model.spaceVisualColumns(1200,650));
 for(const [w,h]of [[430,932],[500,300],[900,180],[NaN,400]])assert.equal(model.spaceVisualColumns(w,h),false);
 measuredSize={width:900,height:300};let received;
 const tree=SpaceVisualPage({...props,enabled:true,compact:true,footer:columns=>{received=columns;return 'controls';}});
 assert.equal(received,true);assert.equal(tree.props.children[1].props.children[0],canvas);
 measuredSize={width:430,height:932};
});

test("the visual floor cannot obscure any bed speaker, including the lower 360RA hemisphere",()=>{
 const {LAYOUTS}=moduleFor('packages/renderer/src/layouts.ts');
 const {speakerScenePosition}=moduleFor('packages/renderer/src/scene-coordinates.ts');
 for(const layout of ['2.0','7.1.4','360RA-13'])for(const speaker of LAYOUTS[layout])assert(speakerScenePosition(speaker)[1]>=model.spaceStageFloor(layout));
});


test("a short tap enters fullscreen but drags, pinches, holds and cancellation never do", () => {
 const tap=new model.SpaceSceneTap(),point=(pageX,pageY)=>({pageX,pageY});
 tap.begin([point(10,20)],100);tap.update([point(12,21)]);assert(tap.end(240));assert.equal(tap.end(241),false);
 tap.begin([point(10,20)],300);tap.update([point(30,20)]);tap.update([point(10,20)]);assert.equal(tap.end(400),false);
 tap.begin([point(10,20)],500);tap.update([point(10,20),point(30,20)]);tap.update([point(10,20)]);assert.equal(tap.end(600),false);
 tap.begin([point(10,20),point(30,20)],700);tap.update([point(10,20)]);assert.equal(tap.end(800),false);
 tap.begin([point(10,20)],900);assert.equal(tap.end(1300),false);
 tap.begin([point(10,20)],1400);tap.cancel();assert.equal(tap.end(1500),false);
});

test("fullscreen and exit keep the same scene and expose an accessible control without a duplicate timeline",()=>{
 measuredSize={width:1200,height:700};
 const values=[];const viewport=tree=>nodes(tree).find(n=>n.props?.testID==='space-scene-viewport');
 for(const enabled of [false,true])for(const fullscreen of [false,true]){
  const tree=SpaceVisualPage({...props,enabled,fullscreen,onFullscreenChange:value=>values.push(value)});
  assert.equal(viewport(tree).props.children[0],canvas);
  const button=nodes(tree).find(n=>n.props?.testID==='space-fullscreen-button');
  assert.equal(button.props.accessibilityLabel,fullscreen?'退出空间全屏':'空间全屏');button.props.onPress();
  const styles=node=>node.props.style.flat(Infinity).filter(Boolean);
  assert.equal(styles(tree.props.children[0]).some(s=>s.display==='none'),fullscreen);
  assert.equal(styles(tree.props.children[2]).some(s=>s.display==='none'),fullscreen);
  // Only the compact identity badge may show the channel layout; no stage watermark.
  assert.equal(nodes(viewport(tree)).filter(n=>n.type==='Text').some(n=>n.props.children.includes('7.1.4')),false);
  assert.equal(nodes(tree).some(n=>n.props?.accessibilityRole==='progressbar'),false);
 }
 assert.deepEqual(values,[true,false,true,false]);measuredSize={width:430,height:932};
});


// Execute the actual host components with native surfaces stubbed. Exercise both
// mobile chrome paths so fullscreen cannot leave an invisible mini-player lane.
function hostModule(file,mocks){
 const ts=require("typescript"),fs=require("node:fs"),module={exports:{}};
 const js=ts.transpileModule(fs.readFileSync(file,"utf8"),{compilerOptions:{jsx:ts.JsxEmit.React,target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
 new Function("require","module","exports","__DEV__",js)(name=>{if(name in mocks)return mocks[name];throw Error("Missing host mock: "+name);},module,module.exports,false);
 return module.exports;
}
const hostReact={...react,useState:initial=>[initial,()=>{}],useRef:value=>({current:value}),Children:{toArray:children=>children},isValidElement:()=>false};
const insets={top:24,bottom:16,left:0,right:0};
const pages=[0,1,2].map(index=>({type:"test-page-"+index,props:{}}));
const hostProps={selected:2,onChange(){},children:pages,pages,accessory:{type:"test-mini",props:{}},nativeAccessory:()=>"native-mini",fallback:"fallback",accent:"#aaaaaa",theme:"dark",backgroundColor:"#111111"};
test("iOS hides native tabs and mini-player in fullscreen and restores them on exit with the same page slot",()=>{
 const{IOSUIKitTabs}=hostModule("apps/mobile/src/IOSUIKitTabs.tsx",{
  react:hostReact,"react-native":{Platform:{OS:"ios",Version:26},UIManager:{hasViewManagerConfig:()=>true}},
  "./ios-platform-version":{supportsIOSGlass:()=>true},"./native-tab-selection":{},"./ios-tab-appearance":{},"./IOSNativePage":{hasNativePageHeader:false},
  "react-native-safe-area-context":{useSafeAreaInsets:()=>insets,SafeAreaProvider:"SafeAreaProvider",SafeAreaView:"SafeAreaView"},
  "./ios-ui-model":{IOS_TABS:["播放","资料库","空间"]},"react-native-screens":{Tabs:{Host:"TabsHost",Screen:"TabsScreen"}},
 });
 for(const spaceFullscreen of [false,true,false]){
  const tree=IOSUIKitTabs({...hostProps,spaceFullscreen});
  const host=nodes(tree).find(n=>n.type==="TabsHost");assert.equal(host.props.tabBarHidden,spaceFullscreen);assert.equal(host.props.ios.bottomAccessoryHidden,spaceFullscreen);
  assert.equal(nodes(tree).filter(n=>n.type==="test-page-2").length,1);
 }
});
test("Android removes fullscreen chrome and its reserved lane, then restores the normal mini-player",()=>{
 const{PlatformPlayerTabs}=hostModule("apps/mobile/src/PlatformPlayerTabs.tsx",{
  react:hostReact,"react-native":{Platform:{OS:"android"},View:"View"},
  "react-native-safe-area-context":{useSafeAreaInsets:()=>insets},"./android-material-theme":{androidMaterialTheme:()=>({bg:"#111111"})},"./app-theme":{useAppTheme:()=>({id:"default"})},
  "./IOSSystemTabs":{hasSystemIOSTabs:false},"./AndroidChrome":{hasAndroidChrome:true,AndroidChrome:"AndroidChrome",AndroidPageHeader:"AndroidPageHeader"},
  "./android-chrome-model":{androidHeaderLayout:()=>({top:24,contentTop:64,height:40}),androidChromeState:()=>({showMini:true,height:116})},
 });
 for(const sideBar of [false,true])for(const spaceFullscreen of [false,true,false]){
  const tree=PlatformPlayerTabs({...hostProps,player:{},spaceFullscreen,sideBar,sideBarPosition:"left"});
  assert.equal(nodes(tree).filter(n=>n.type==="AndroidChrome").length,spaceFullscreen?0:1);
  const parent=nodes(tree).find(n=>n.type==="View"&&n.props.children?.[0]?.props?.children?.includes(pages[2]));
  assert(parent);assert.equal(parent.props.style.paddingBottom,spaceFullscreen?16:sideBar?80:132);
 }
});
