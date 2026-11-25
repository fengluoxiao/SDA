import assert from "node:assert/strict";
import test from "node:test";
import {readFileSync} from "node:fs";
import {build} from "esbuild";

const compiled=await build({entryPoints:["apps/mobile/src/ArtworkEdgeBlend.tsx"],bundle:true,platform:"node",format:"cjs",external:["*"],write:false});
const React={createElement:(type,props,...children)=>({type,props:{...props,children}}),Fragment:"Fragment"};
const module={exports:{}};
new Function("require","module","exports",compiled.outputFiles[0].text)(name=>name==="react"?React:name==="react-native"?{View:"View"}:{PhoneLandscapeBlur:"RightEdge",ArtworkBottomFeather:"BottomFeather"},module,module.exports);
const blend=module.exports.ArtworkEdgeBlend,poster={type:"Poster"},video={type:"MotionVideo"};
test("portrait motion stays under the same tablet right/bottom feather and preserves poster fallback",()=>{
 const common={children:poster,uri:"poster",surfaceWidth:900,surfaceHeight:1200,frame:{left:35,top:60,width:500,height:500},backgroundColor:"#303034"};
 assert.equal(blend({...common,landscape:false}),poster);
 const loading=blend({...common,landscape:false,motionEnabled:true,motionVisible:false,overlay:video});
 assert.deepEqual(loading.props.children.slice(0,2),[poster,video]);assert.equal(loading.props.children[2],false);
 const playing=blend({...common,landscape:false,motionEnabled:true,motionVisible:true,overlay:video});
 const feather=playing.props.children[2];assert.equal(feather.type,"BottomFeather");assert.equal(feather.props.parentLeft,35);assert.equal(feather.props.parentTop,60);
 const landscape=blend({...common,landscape:true,overlay:video});assert.equal(landscape.props.children[2].type,"RightEdge");
});
test("dynamic artwork is opt-in and only mounted on the large player cover",()=>{
 const prefs=readFileSync("apps/mobile/src/navigation-preferences.tsx","utf8");assert.match(prefs,/const defaults[^\n]*motionArtwork: false/);
const overlay=readFileSync("apps/mobile/src/EmbeddedMotionArtwork.tsx","utf8");assert.match(overlay,/if\(!motionArtwork \|\| !NativeArtwork/);assert.match(overlay,/unavailable/);assert.match(overlay,/MotionFrame key=\{motionKey \?\? sourceUri\} sourceUri=\{sourceUri\}/);assert.match(overlay,/active=\{visible && active && foreground\}/);assert.match(overlay,/active=\{active\} blurred=\{blurred\} pointerEvents/);assert.doesNotMatch(overlay,/!visible \|\| !foreground/);assert.doesNotMatch(overlay,/!foreground \|\| !sourceUri/);assert.match(overlay,/report\(true\)/);assert.match(overlay,/setReady\(event\.nativeEvent\.ready\)/);
 const ui=readFileSync("apps/mobile/src/IOSPlayer.tsx","utf8");assert.equal((ui.match(/<EmbeddedMotionArtwork /g)||[]).length,3);assert.match(ui,/!sharedMini\.active/);assert.match(ui,/const motionSurfaceVisible =/);assert.doesNotMatch(ui,/active=\{immersiveHome && playing[^\n]*\} \/>/);
 const rows=ui.slice(ui.indexOf("track.metadata.coverUri ?"));assert(!rows.split("</ScrollView>")[0].includes("EmbeddedMotionArtwork"));
 assert(!readFileSync("apps/mobile/src/SharedMiniPlayerTransition.tsx","utf8").includes("EmbeddedMotionArtwork"));
 for(const file of ["IOSNativeSettings.tsx","AndroidSettings.tsx"]){const text=readFileSync("apps/mobile/src/"+file,"utf8");assert(text.includes("动态封面")&&text.includes("setMotionArtwork"));}
 assert.match(ui,/const tabletMotionEnabled = tabletPortrait && supportsEmbeddedMotionArtwork && navigationPreferences.motionArtwork/);
 assert.match(ui,/const motionSourceKey=`\$\{p\.selectedUri\}:\$\{artworkUri\}`/);
 assert.match(ui,/\{!!wantedMotionKey && <Animated.View key=\{wantedMotionKey\} testID="tablet-motion-ambient-motion"/);
 assert.match(ui,/<MotionArtworkFeather[^>]*>\s*<View[^>]*motionPlaneFrame[^>]*>\s*<EmbeddedMotionArtwork motionKey=\{wantedMotionKey\} sourceUri=\{p.selectedUri\}/);
 assert.match(ui,/new Animated.Value\(0\)[\s\S]*?\}, \[wantedMotionKey\]\)/);
 assert.match(ui,/p\.queue\.slice\(p\.queueIndex \+ 1\)\.find\(track => !track\.preparation\)/);
 assert.equal((ui.match(/<QueuedArtwork[^>]*nextUri=\{nextPortraitArtwork\}/g) || []).length, 2);
 assert.doesNotMatch(ui,/tabletStillReady|motionScene|CrossfadeArtwork/);
 const iosNative=readFileSync("apps/mobile/modules/sda-core/ios/SdaMotionArtworkModule.swift","utf8");assert.doesNotMatch(iosNative,/guard active,!starting/);assert.match(iosNative,/if !self\.active \{player\.pause\(\)\}/);assert.match(iosNative,/player\.play\(\)/);
 const androidNative=readFileSync("apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/SdaMotionArtworkModule.kt","utf8");assert.doesNotMatch(androidNative,/if\(!active \|\| !foreground/);assert.match(androidNative,/if\(!active\)player\?\.pause\(\)/);
});

const featherBundle = await build({ entryPoints: ["apps/mobile/src/PhoneLandscapeBlur.tsx"], bundle: true, platform: "node", format: "cjs", external: ["react", "react-native", "@expo/ui/*", "*.png"], write: false });
const native = { View: "View", Image: "Image", Platform: { OS: "ios" }, StyleSheet: { absoluteFill: {} } };
const loaded = { exports: {} };
new Function("require", "module", "exports", featherBundle.outputFiles[0].text)(name => name === "react" ? { ...React, memo: component => component } : name === "react-native" ? native : name.endsWith(".png") ? name : { MaskedView: "MaskedView" }, loaded, loaded.exports);
const { MotionArtworkFeather, QueuedArtwork } = loaded.exports;

test("portrait motion fades the sharp artwork down into the separate full-height backdrop", () => {
 const previous = globalThis.expo;
 globalThis.expo = { modules: { ExpoUI: {} }, getViewConfig: () => ({}) };
 try {
  const tree = MotionArtworkFeather({ children: video, surfaceWidth: 430, surfaceHeight: 900, artworkTop: 0, artworkHeight: 480, fallbackColor: "#303034" });
  assert.equal(tree.type, "MaskedView");
  assert.equal(tree.props.children[0], video);
  const [solid, ramp] = tree.props.maskElement.props.children;
  assert.equal(solid.props.style.backgroundColor, "#ffffff");
  assert.deepEqual(ramp.props.style.transform, [{ scaleY: -1 }]);
  assert.equal(solid.props.style.height + ramp.props.style.height, 480);
 } finally { globalThis.expo = previous; }
});

test("the next poster and blur become visible in their original keyed views, without an outgoing layer", () => {
 for (const resizeMode of ["contain", "cover"]) {
  const blurRadius = resizeMode === "cover" ? 48 : undefined;
  const layers = (uri, nextUri) => QueuedArtwork({ uri, nextUri, resizeMode, blurRadius, style: {} }).props.children[0];
  const before = layers("first", "second");
  const after = layers("second", "third");
  assert.equal(before[1].props.style[1].opacity, 0);
  assert.deepEqual(after.map(layer => layer.props.key), ["second", "third"]);
  assert.equal(after[0].type, before[1].type);
  assert.equal(after[0].props.key, before[1].props.key);
  assert.deepEqual(after[0].props.source, before[1].props.source);
  assert.equal(after[0].props.blurRadius, before[1].props.blurRadius);
  assert.equal(after[0].props.style[1].opacity, 1);
  assert.equal(after[0].props.fadeDuration, 0);
  assert.equal(after[0].props.onLoad, undefined);
  assert.deepEqual(layers("skipped", "").map(layer => layer.props.key), ["skipped"]);
  assert.equal(layers("shared", "shared").length, 1);
  assert.equal(layers("", "next")[0].props.style[1].opacity, 0);
 }
});
