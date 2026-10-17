import assert from 'node:assert/strict';
import {readFileSync,existsSync} from 'node:fs';
const source=readFileSync('apps/mobile/src/IOSPlayer.tsx','utf8');
const expression=source.match(/scrollIndicatorInsets=\{(\{[^}]+\})\}/)[1];
const indicator=new Function('sideNavigationRight','SIDE_NAVIGATION_WIDTH','sideNavigationActive','p','screenInsets','nativeLibraryHeaderActive','return '+expression);
for(const safeRight of [0,44,59,62]) {
 const inset=indicator(true,48,true,{selectedUri:'mock'},{right:safeRight});
 assert.equal(inset.top,10);assert.equal(inset.bottom,62);
 assert.equal(inset.right,56-Math.min(12,(safeRight+8)/4),'accepted right gutter remains unchanged');
 const railLeft=932-(safeRight+8)/2-48;const contentRight=932-safeRight-56;
 const indicatorX=932-safeRight-inset.right;
 assert(indicatorX>contentRight && indicatorX<railLeft);
 const left=indicator(false,48,true,{selectedUri:'mock'},{right:safeRight},true);
 assert.equal(left.right,3+safeRight);assert.equal((932-left.right)-(932-safeRight-18),15);
 const parentRight=932-safeRight-6;const marginRight=-6-safeRight;
 assert.equal(parentRight-marginRight,932,'native scroll-edge effect reaches screen right');
 assert.equal(932-(18+safeRight),932-safeRight-18,'row/card right edge stays unchanged');
}
assert.match(source,/!nativeLibraryHeaderActive && homeHeader/);
assert.match(source,/automaticallyAdjustsScrollIndicatorInsets=\{Platform.OS === "ios" && hasSystemIOSTabs\}/);
assert.match(source,/contentInsetAdjustmentBehavior="automatic"/);
assert.doesNotMatch(source,/libraryHeaderOverlay|homeHeaderHeight|libraryScrollY|IOSHeaderBlur|IOSLibraryHeaderBackdrop/);
for(const file of ['apps/mobile/src/IOSHeaderBlur.tsx','apps/mobile/src/IOSLibraryHeaderBackdrop.tsx','apps/mobile/assets/header-blur-edge-mask.png'])assert.equal(existsSync(file),false,'retired imitation must not remain in the app');
assert.equal(JSON.parse(readFileSync('apps/mobile/package.json','utf8')).dependencies['expo-blur'],undefined);
console.log('Library native header: automatic insets, accepted indicator gutters, no manual header blur/opacity/mask.');

// The left-mode scroll edge must exclude the action lane without shifting rows.
const {build}=await import('esbuild');
const model=await build({entryPoints:['apps/mobile/src/ios-ui-model.ts'],bundle:true,platform:'node',format:'esm',write:false});
const {iosLeftLibraryGutter,iosRightLibraryGutter}=await import('data:text/javascript;base64,'+Buffer.from(model.outputFiles[0].text).toString('base64'));
for(const safeLeft of [0,4,8,14,44,59,62]) {
  const pageLeft=safeLeft+30;
  const oldRowLeft=pageLeft-18+56;
  const gutter=iosLeftLibraryGutter(safeLeft);
  const scrollLeft=pageLeft+gutter.marginLeft;
  assert.equal(scrollLeft+gutter.paddingLeft,oldRowLeft,'accepted card/cover positions remain unchanged');
  const actionRight=Math.max(14,safeLeft)+6+48-2;
  assert(scrollLeft>=actionRight+2,'native scroll-edge effect clears left actions for every safe inset');
}
assert.match(source,/marginLeft: iosLeftLibraryGutter\(screenInsets\?\.left \?\? 0\).marginLeft/);
console.log('Left library viewport excludes the native-header action lane while preserving row placement.');

for(const safeRight of [0,4,8,14,44,59,62]) {
 const gutter=iosRightLibraryGutter(safeRight);
 const oldContentRight=932-safeRight-56;
 const oldIndicatorRight=oldContentRight+Math.min(12,(safeRight+8)/4);
 const scrollRight=932-safeRight-18-gutter.marginRight;
 assert.equal(scrollRight-gutter.paddingRight,oldContentRight,'right card/cover positions stay identical');
 const inset=indicator(true,48,true,{selectedUri:'mock'},{right:safeRight},true);
 assert.equal(scrollRight-inset.right,oldIndicatorRight,'accepted indicator x-position stays identical');
 const actionLeft=932-(safeRight+8)/2-48+2;
 assert(scrollRight<=actionLeft-2,'native effect clears right header actions');
 for(const safeLeft of [0,59])assert.equal(safeLeft+18+(-18-safeLeft),0,'right-mode viewport reaches screen left');
}
console.log('Right library native effect clears actions and reaches the opposite edge without moving cards or scrollbar.');
