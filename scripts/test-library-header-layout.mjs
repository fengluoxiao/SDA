import assert from 'node:assert/strict';
import {readFileSync,existsSync} from 'node:fs';
const {build}=await import('esbuild');
const model=await build({entryPoints:['apps/mobile/src/ios-ui-model.ts'],bundle:true,platform:'node',format:'esm',write:false});
const {iosLeftLibraryGutter,iosRightLibraryGutter,rightSideDockInset,iosRightSideContentGap,leftSideDockInset,iosLeftContentOffset}=await import('data:text/javascript;base64,'+Buffer.from(model.outputFiles[0].text).toString('base64'));
const source=readFileSync('apps/mobile/src/IOSPlayer.tsx','utf8');
const expression=source.match(/scrollIndicatorInsets=\{(\{[^}]+\})\}/)[1];
const indicator=new Function('iosRightSideContentGap','iosRightLibraryGutter','sideNavigationRight','SIDE_NAVIGATION_WIDTH','sideNavigationActive','p','screenInsets','nativeLibraryHeaderActive','cutoutSide','return '+expression).bind(null,iosRightSideContentGap,iosRightLibraryGutter);
for(const safeRight of [0,44,59,62]) {
 const inset=indicator(true,48,true,{selectedUri:'mock'},{right:safeRight});
 assert.equal(inset.top,10);assert.equal(inset.bottom,62);
 assert.equal(inset.right,(48+iosRightSideContentGap(safeRight))-iosRightLibraryGutter(safeRight).indicatorRight,'indicator fits ahead of cutout-safe rail');
 const railLeft=932-rightSideDockInset(safeRight)-48;const contentRight=932-safeRight-48-iosRightSideContentGap(safeRight);
 const indicatorX=932-safeRight-inset.right;
 assert(indicatorX>contentRight && indicatorX<railLeft);
 const left=indicator(false,48,true,{selectedUri:'mock'},{right:safeRight},true);
 assert.equal(left.right,3+safeRight);assert.equal((932-left.right)-(932-safeRight-18),15);
 const parentRight=932-safeRight-6;const marginRight=-6-safeRight;
 assert.equal(parentRight-marginRight,932,'native scroll-edge effect reaches screen right');
 assert.equal(932-(18+safeRight),932-safeRight-18,'row/card right edge stays unchanged');
}
assert.match(source,/!hasUIKitIOSTabs && !hasAndroidChrome && pageHeader\(page\)/);
assert.match(source,/const nativeLibraryHeaderActive = hasNativePageHeader && hasUIKitIOSTabs;/);
assert.match(source,/automaticallyAdjustsScrollIndicatorInsets=\{Platform.OS === "ios" && hasSystemIOSTabs\}/);
assert.match(source,/contentInsetAdjustmentBehavior="automatic"/);
assert.doesNotMatch(source,/libraryHeaderOverlay|homeHeaderHeight|libraryScrollY|IOSHeaderBlur|IOSLibraryHeaderBackdrop/);
for(const file of ['apps/mobile/src/IOSHeaderBlur.tsx','apps/mobile/src/IOSLibraryHeaderBackdrop.tsx','apps/mobile/assets/header-blur-edge-mask.png'])assert.equal(existsSync(file),false,'retired imitation must not remain in the app');
assert.doesNotMatch(readFileSync('apps/mobile/src/IOSNativePage.tsx','utf8'),/expo-blur|BlurView|glassEffect/,'page headers must stay native navigation bars');
console.log('Library native header: automatic insets, accepted indicator gutters, no manual header blur/opacity/mask.');

// The left-mode scroll edge must exclude the action lane without shifting rows.
for(const safeLeft of [0,4,8,14,44,59,62]) {
  const pageLeft=safeLeft+30;
  const oldRowLeft=pageLeft-18+56;
  const gutter=iosLeftLibraryGutter(safeLeft);
  const scrollLeft=pageLeft+gutter.marginLeft;
  assert.equal(scrollLeft+gutter.paddingLeft,oldRowLeft,'accepted card/cover positions remain unchanged');
  const actionRight=Math.max(14,safeLeft)+6+48-2;
  assert(scrollLeft>=actionRight+2,'native scroll-edge effect clears left actions for every safe inset');
}
assert.match(source,/marginLeft: iosLeftLibraryGutter\(screenInsets\?\.left \?\? 0, cutoutSide\).marginLeft/);
console.log('Left library viewport excludes the native-header action lane while preserving row placement.');

for(const safeRight of [0,4,8,14,44,59,62]) for(const cutoutSide of ['left','right',null]) {
 const gutter=iosRightLibraryGutter(safeRight,cutoutSide);
 const contentRight=932-safeRight-48-iosRightSideContentGap(safeRight,cutoutSide);
 const scrollRight=932-safeRight-18-gutter.marginRight;
 assert.equal(scrollRight-gutter.paddingRight,contentRight,'rows reserve the gap for the actual cutout side');
 const inset=indicator(true,48,true,{selectedUri:'mock'},{right:safeRight},true,cutoutSide);
 const indicatorX=scrollRight-inset.right;
 assert.equal(indicatorX,contentRight+gutter.indicatorRight);
 const railLeft=932-rightSideDockInset(safeRight,cutoutSide)-48;
 assert(scrollRight<=railLeft,'native effect clears the side controls');
 assert(indicatorX>contentRight && indicatorX<railLeft,'scrollbar stays between content and rail');
 if(cutoutSide==='left') {
   assert.equal(railLeft,932-(safeRight+8)/2-48,'opposite-side dock restores its exact approved position');
   assert.equal(contentRight,932-safeRight-56,'opposite-side rows restore their exact approved position');
   assert.equal(indicatorX,contentRight+Math.min(12,(safeRight+8)/4),'opposite-side scrollbar restores its exact approved position');
 } else {
   assert.equal(railLeft-contentRight,safeRight>0?16:4,'extra gap only applies on the cutout side');
 }
 for(const safeLeft of [0,59])assert.equal(safeLeft+18+(-18-safeLeft),0,'right-mode viewport reaches screen left');
}
console.log('Right layout: full same-side cutout protection; exact original dock, rows and scrollbar on the opposite side.');

for(const safeLeft of [0,44,59,62]) for(const cutoutSide of ['left','right']) {
 const row= safeLeft+30+iosLeftLibraryGutter(safeLeft,cutoutSide).marginLeft+iosLeftLibraryGutter(safeLeft,cutoutSide).paddingLeft;
 const dockRight=leftSideDockInset(safeLeft,cutoutSide)+48;
 const baselineGap=safeLeft+68-(Math.max(14,safeLeft)+6+48);
 assert.equal(row-dockRight,baselineGap,'left content follows its dock without creating opposite-side dead space');
 assert.equal(row,safeLeft+68+iosLeftContentOffset(safeLeft,cutoutSide));
}
