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
const overlay=readFileSync("apps/mobile/src/EmbeddedMotionArtwork.tsx","utf8");assert.match(overlay,/if\(!motionArtwork \|\| !NativeArtwork/);assert.match(overlay,/unavailable/);assert.match(overlay,/MotionFrame key=\{sourceUri\} sourceUri=\{sourceUri\}/);assert.match(overlay,/active=\{visible && active && foreground\}/);assert.match(overlay,/active=\{active\} pointerEvents/);assert.doesNotMatch(overlay,/!visible \|\| !foreground/);assert.doesNotMatch(overlay,/!foreground \|\| !sourceUri/);
 const ui=readFileSync("apps/mobile/src/IOSPlayer.tsx","utf8");assert.equal((ui.match(/<EmbeddedMotionArtwork /g)||[]).length,2);assert.match(ui,/!sharedMini\.active/);assert.match(ui,/const motionSurfaceVisible =/);assert.doesNotMatch(ui,/active=\{immersiveHome && playing[^\n]*\} \/>/);
 const rows=ui.slice(ui.indexOf("track.metadata.coverUri ?"));assert(!rows.split("</ScrollView>")[0].includes("EmbeddedMotionArtwork"));
 assert(!readFileSync("apps/mobile/src/SharedMiniPlayerTransition.tsx","utf8").includes("EmbeddedMotionArtwork"));
 for(const file of ["IOSNativeSettings.tsx","AndroidSettings.tsx"]){const text=readFileSync("apps/mobile/src/"+file,"utf8");assert(text.includes("动态封面")&&text.includes("setMotionArtwork"));}
 const player=readFileSync("apps/mobile/src/IOSPlayer.tsx","utf8");assert.match(player,/np\.motionCover/);assert.match(player,/tabletPlayer && !tabletMotionBackdrop/);
 const blur=readFileSync("apps/mobile/src/PhoneLandscapeBlur.tsx","utf8");assert.match(blur,/export function ArtworkBottomFeather/);assert.match(blur,/edgeDirection="bottom"/);
});
