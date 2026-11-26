import assert from "node:assert/strict";
import test from "node:test";
import {readFileSync} from "node:fs";
import {build} from "esbuild";
import "./motion-artwork-selection.test.mjs";
import "./portrait-motion-artwork-layout.test.mjs";

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
const overlay=readFileSync("apps/mobile/src/EmbeddedMotionArtwork.tsx","utf8");assert.match(overlay,/if\(!motionArtwork \|\| !NativeArtwork/);assert.match(overlay,/unavailable/);assert.match(overlay,/MotionFrame key=\{uri\} sourceUri=\{uri\}/);assert.match(overlay,/uri===sourceUri && visible && active && foreground/);assert.match(overlay,/active=\{active\} blurred=\{blurred\} bottomExtension=\{bottomExtension\} pointerEvents/);assert.doesNotMatch(overlay,/!visible \|\| !foreground/);assert.doesNotMatch(overlay,/!foreground \|\| !sourceUri/);assert.match(overlay,/report\(event\.nativeEvent\.ready\)/);assert.match(overlay,/setReady\(event\.nativeEvent\.ready\)/);
 const ui=readFileSync("apps/mobile/src/IOSPlayer.tsx","utf8");assert.equal((ui.match(/<EmbeddedMotionArtwork /g)||[]).length,2);assert.match(ui,/!sharedMini\.active/);assert.match(ui,/const motionSurfaceVisible =/);assert.doesNotMatch(ui,/active=\{immersiveHome && playing[^\n]*\} \/>/);
 const rows=ui.slice(ui.indexOf("track.metadata.coverUri ?"));assert(!rows.split("</ScrollView>")[0].includes("EmbeddedMotionArtwork"));
 assert(!readFileSync("apps/mobile/src/SharedMiniPlayerTransition.tsx","utf8").includes("EmbeddedMotionArtwork"));
 for(const file of ["IOSNativeSettings.tsx","AndroidSettings.tsx"]){const text=readFileSync("apps/mobile/src/"+file,"utf8");assert(text.includes("动态封面")&&text.includes("setMotionArtwork"));}
 assert.match(ui,/const tabletMotionEnabled = tabletPortrait && motionEnabled/);
 assert.match(ui,/const motionSourceKey=p.selectedUri/);
 assert.match(ui,/liveBackdropEnabled=motionEnabled && \(!landscapePlayer \|\| phoneLandscapeBlur \|\| tabletLandscape\)/);
 assert.match(ui,/\{liveBackdropEnabled && <View testID="motion-artwork-live-backdrop"/);
 assert.match(ui,/<MotionArtworkFeather enabled=\{motionEnabled\} mode=\{motionFeatherMode\}/);
 assert.match(ui,/const showStaticArtwork=motionSelection.current==="static"/);
 assert.doesNotMatch(ui,/motionAppearance|Animated\.timing\(motion/);
 assert.doesNotMatch(overlay,/Animated|revealDuration/);
 assert.equal((ui.match(/nextSourceUri=\{nextMotionSourceUri\}/g)||[]).length,2);
 assert.match(ui,/p\.queue\.slice\(p\.queueIndex \+ 1\)\.find\(track => !track\.preparation\)/);
 assert.equal((ui.match(/<QueuedArtwork[^>]*nextUri=\{nextPortraitArtwork\}/g) || []).length, 2);
 assert.doesNotMatch(ui,/tabletStillReady|motionScene|CrossfadeArtwork/);
 const iosNative=readFileSync("apps/mobile/modules/sda-core/ios/SdaMotionArtworkModule.swift","utf8");assert.doesNotMatch(iosNative,/guard active,!starting/);assert.match(iosNative,/if !self\.active \|\| !self\.presented \{self\.pause\(\)\}/);assert.match(iosNative,/player\.automaticallyWaitsToMinimizeStalling=false/);assert.match(iosNative,/player\.playImmediately\(atRate:1\)/);assert.match(iosNative,/AsyncFunction\("hasMotionArtwork"\)/);assert.match(iosNative,/if presented\{startIfNeeded\(\)\}else\{rewind\(\)\}/);assert.match(iosNative,/player.seek\(to:\.zero/);
 const androidNative=readFileSync("apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/SdaMotionArtworkModule.kt","utf8");assert.doesNotMatch(androidNative,/if\(!active \|\| !foreground/);assert.match(androidNative,/if\(!active \|\| !presented\)pause\(\)/);assert.match(androidNative,/AsyncFunction\("hasMotionArtwork"\)/);assert.match(androidNative,/if\(value\)startIfNeeded\(\)else rewind\(\)/);
});

test("native covers alternate direction without creating reversed-video files or retaining a full decoded clip",()=>{
 const ios=readFileSync("apps/mobile/modules/sda-core/ios/SdaMotionArtworkModule.swift","utf8");
 assert.doesNotMatch(ios,/AVPlayerLooper/);
 assert.match(ios,/direction = -1/);assert.match(ios,/playImmediately\(atRate:direction\)/);
 assert.doesNotMatch(ios,/generateCGImagesAsynchronously|item.canPlayReverse/);
 assert.match(ios,/MotionArtworkReverseFrames.acquire\(uri\)/);
 assert.match(ios,/bottomExtension \? frame.bottomImage:frame.blurredImage/);
 assert.match(ios,/toneLayer=CAGradientLayer\(\)/);
 const iosFrames=readFileSync("apps/mobile/modules/sda-core/ios/MotionArtworkReverseFrames.swift","utf8");
 assert.match(iosFrames,/AVAssetReaderTrackOutput/);assert.match(iosFrames,/copyNextSampleBuffer/);
 assert.match(iosFrames,/blurFrame.cropping\(to:/);assert.match(iosFrames,/deferred:false/);
 assert.doesNotMatch(iosFrames,/MotionArtworkBackdrop.blurred\(small,bottomExtension:true/);
 assert.match(iosFrames,/ProcessInfo.processInfo.physicalMemory\/128/);assert.match(iosFrames,/abs\(\$0.key-wantedIndex\)<=1/);
 assert.match(ios,/reverseClock.pause\(at:CACurrentMediaTime\(\)\)/);
 const android=readFileSync("apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/SdaMotionArtworkModule.kt","utf8");
 assert.match(android,/next.isLooping=false/);assert.match(android,/setOnCompletionListener.*beginReverse/);
 assert.match(android,/setOnPreparedListener[^\n]*prepareReverseTail\(\)/);
 assert.match(android,/currentReversePosition\(\)/);assert.match(android,/waitingForwardFrame && forwardSeekComplete/);
 const frames=readFileSync("apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/MotionArtworkReverseFrames.kt","utf8");
 assert.match(frames,/getFramesAtIndex\(start,count\)/);assert.match(frames,/maxMemory\(\)\/12/);
 assert.match(frames,/kotlin.math.abs\(it-wanted\)>1/);
 assert.match(frames,/executor.shutdown\(\)/);
 assert.doesNotMatch(ios+android+frames+iosFrames,/AVAssetWriter|MediaMuxer|FileOutputStream|mutableListOf<Bitmap|\[CGImage\]/);
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
  const vertical=tree.props.children[0];
  assert.equal(vertical.props.children[0], video);
  assert.equal(tree.props.maskElement.props.style.backgroundColor,"#ffffff");
  const [solid, ramp] = vertical.props.maskElement.props.children;
  assert.equal(solid.props.style.backgroundColor, "#ffffff");
  assert.deepEqual(ramp.props.style.transform, [{ scaleY: -1 }]);
  assert.equal(solid.props.style.height + ramp.props.style.height, 480);
 } finally { globalThis.expo = previous; }
});

test("rotation keeps one video under directional masks and exposes the same live blur at each cover edge",()=>{
 const previous=globalThis.expo;globalThis.expo={modules:{ExpoUI:{}},getViewConfig:()=>({})};
 try{
  const common={children:video,surfaceWidth:1000,surfaceHeight:700,artworkLeft:24,artworkTop:10,artworkWidth:500,artworkHeight:500,fallbackColor:"#303034"};
  const portrait=MotionArtworkFeather({...common,mode:"portrait"});
  const phone=MotionArtworkFeather({...common,mode:"phone-landscape"});
  const tablet=MotionArtworkFeather({...common,mode:"tablet-landscape"});
  for(const tree of [phone,tablet]){
   assert.equal(tree.type,portrait.type);
   assert.equal(tree.props.children[0].type,portrait.props.children[0].type);
   assert.equal(tree.props.children[0].props.children[0],video);
  }
  const phoneRamp=phone.props.maskElement.props.children[1];
  assert.equal(phoneRamp.props.style.left,374);assert.equal(phoneRamp.props.style.width,150);
  assert.deepEqual(phoneRamp.props.style.transform,[{scaleX:-1}]);
  assert.equal(phone.props.children[0].props.maskElement.props.style.backgroundColor,"#ffffff");
  const tabletRamp=tablet.props.maskElement.props.children[1];
  assert.equal(tabletRamp.props.style.left,476);assert.equal(tabletRamp.props.style.width,48);
  assert.equal(tablet.props.children[0].props.maskElement.props.children[1].props.style.height,175);
  const still=MotionArtworkFeather({...common,mode:"none"});
  assert.equal(still.props.maskElement.props.style.backgroundColor,"#ffffff");
  assert.equal(still.props.children[0].props.maskElement.props.style.backgroundColor,"#ffffff");
  const disabled=MotionArtworkFeather({...common,enabled:false});assert.equal(disabled.props.children[0],video);
 }finally{globalThis.expo=previous;}
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

const motionBundle=await build({entryPoints:["apps/mobile/src/EmbeddedMotionArtwork.tsx"],bundle:true,platform:"node",format:"cjs",external:["*"],write:false});
function motionHarness(){
 const states=new Map(),effects=[];
 let cursor=0,slots=[];
 const hooks={...React,
  useState(initial){const slot=cursor++,state=slots;if(!(slot in state))state[slot]=initial;return[state[slot],value=>{state[slot]=value;}];},
  useRef(initial){const slot=cursor++;return slots[slot]??={current:initial};},
  useCallback(callback){return callback;},
  useEffect(run,deps){const slot=cursor++;if(!slots[slot]||deps.some((value,index)=>value!==slots[slot].deps[index]))effects.push(()=>{slots[slot]?.cleanup?.();slots[slot]={deps,cleanup:run()};});},
 };
 hooks.useLayoutEffect=hooks.useEffect;
 const animated={View:"Animated.View",Value:class{constructor(value){this.value=value;}setValue(value){this.value=value;}},timing:(value,config)=>({start(){value.value=config.toValue;},stop(){}})};
 const native={View:"View",Animated:animated,AppState:{currentState:"active",addEventListener:()=>({remove(){}})},StyleSheet:{absoluteFill:{}}};
 const previous=globalThis.expo;
 globalThis.expo={modules:{SdaMotionArtwork:{}}};
 const exports={exports:{}};
 try{
  new Function("require","module","exports",motionBundle.outputFiles[0].text)(name=>name==="react"?hooks:name==="react-native"?native:name==="expo"?{requireNativeView:()=>"NativeMotion"}:{useNavigationPreferences:()=>({motionArtwork:true})},exports,exports.exports);
 }finally{globalThis.expo=previous;}
 return{component:exports.exports.EmbeddedMotionArtwork,render(component,props){cursor=0;if(!states.has(component))states.set(component,[]);slots=states.get(component);const tree=component(props);effects.splice(0).forEach(run=>run());return tree;}};
}

test("the next video prepares one paused frame and promotes the same native view on selection",()=>{
 const queue=motionHarness(),reports=[];
 const props={sourceUri:"file:///first.m4a",nextSourceUri:"file:///next.m4a",motionKey:"file:///first.m4a",visible:true,active:true,revealDuration:0,onVisible:(...args)=>reports.push(args)};
 const before=queue.render(queue.component,props).props.children[0];
 assert.equal(before.length,2);
 assert.equal(before[1].props.active,false);
 assert.equal(before[1].props.presented,false);
 const frame=queue;
 let tree=frame.render(before[1].type,before[1].props);
 const nativeView=tree.props.children[0];
 nativeView.props.onStatus({nativeEvent:{uri:"file:///wrong.m4a",ready:true}});
 assert.equal(reports.length,0);
 nativeView.props.onStatus({nativeEvent:{uri:"file:///next.m4a",ready:true}});
 assert.deepEqual(reports,[[true,"file:///next.m4a"]]);
 tree=frame.render(before[1].type,before[1].props);
 assert.equal(tree.props.style[1].opacity,0);
 assert.equal(tree.props.children[0].props.presented,false);
 const promoted=queue.render(queue.component,{...props,sourceUri:"file:///next.m4a",motionKey:"file:///next.m4a",nextSourceUri:"file:///third.m4a"}).props.children[0][0];
 assert.equal(promoted.type,before[1].type);
 assert.equal(promoted.props.key,before[1].props.key);
 assert.equal(promoted.props.active,true);
 tree=frame.render(promoted.type,promoted.props);
 assert.equal(tree.props.style[1].opacity,1);
 assert.equal(tree.props.children[0].props.presented,true);
 const paused=queue.render(queue.component,{...props,active:false}).props.children[0];
 assert(paused.every(layer=>layer.props.active===false));
 assert.deepEqual(queue.render(queue.component,{...props,sourceUri:"file:///same.m4a",nextSourceUri:"file:///same.m4a"}).props.children[0].length,1);
});

test("an absent or failed next video remains invisible and cannot hide a static-only song",()=>{
 const queue=motionHarness(),reports=[];
 const props={sourceUri:"file:///first.m4a",nextSourceUri:"file:///still.m4a",visible:true,active:true,onVisible:(...args)=>reports.push(args)};
 const pending=queue.render(queue.component,props).props.children[0][1];
 const frame=queue;
 const tree=frame.render(pending.type,pending.props);
 tree.props.children[0].props.onStatus({nativeEvent:{uri:"file:///still.m4a",ready:false,unavailable:true}});
 assert.equal(frame.render(pending.type,pending.props),null);
 assert.equal(reports.length,0);
 const third=queue.render(queue.component,{...props,sourceUri:"file:///third.m4a",nextSourceUri:undefined}).props.children[0];
 assert.deepEqual(third.map(layer=>layer.props.sourceUri),["file:///third.m4a"]);
});
