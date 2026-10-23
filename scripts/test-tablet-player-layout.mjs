import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {build} from 'esbuild';
const result=await build({entryPoints:['apps/mobile/src/tablet-player-layout.ts'],bundle:true,platform:'node',format:'esm',write:false});
const {isTabletPlayer,tabletPlayerLayout,tabletNavigationLane,tabletNavigationPageInsets}=await import('data:text/javascript;base64,'+Buffer.from(result.outputFiles[0].text).toString('base64'));
assert(isTabletPlayer('ios',true,834,1194));assert(isTabletPlayer('ios',true,1194,834));
assert(!isTabletPlayer('ios',false,932,430));assert(!isTabletPlayer('android',false,932,430));
assert(isTabletPlayer('android',false,1280,800));assert(isTabletPlayer('android',false,800,1280));
assert(!isTabletPlayer('web',false,1280,800));
for(const [width,height] of [[1366,900],[1194,730],[1024,650],[960,560],[744,500],[834,1070],[768,900],[600,800],[390,700]])for(const block of [212,270,340])for(const side of [false,true]) {
 const landscape=width>height;
 const layout=tabletPlayerLayout(width,height,block,landscape,side);
 assert(Object.values(layout).every(value=>Number.isFinite(value)&&value>=0));
 assert(layout.coverSize<=520);assert(layout.controlsWidth<=(landscape?440:520));
 const inner=layout.contentWidth-layout.horizontalPadding*2;
 assert(layout.coverSize<=layout.columnWidth);assert(layout.controlsWidth<=layout.columnWidth);
 const controls=block+layout.metadataGap+layout.transportGap+layout.volumeGap;
 if(landscape) {
  assert(layout.columnWidth*2+layout.columnGap<=inner+.001);
  assert(layout.coverSize+layout.verticalPadding*2<=height);
 } else {
  assert(layout.coverSize+layout.columnGap+controls+layout.verticalPadding*2<=height+.001);
 }
}
const source=readFileSync('apps/mobile/src/IOSPlayer.tsx','utf8');
assert.match(source,/tabletLayout && \{ marginTop: tabletLayout.metadataGap, transform: \[/);
assert.match(source,/tabletLayout && \{ marginTop: tabletLayout.volumeGap, transform: \[/);
assert.match(source,/placement="bottom" tablet=\{tabletPlayer\}/);
console.log('Shared tablet layout: iPad/Android gates, both orientations, square complete covers, bounded controls, measured text and navigation space passed.');

for(const inset of [0,12,24,44]) {
 const pageEdge=tabletNavigationLane(inset);
 assert(pageEdge-(inset+20+48)>=28,'page and scrollbar must clear the capsule');
 assert(pageEdge-(inset+20+44)>=32,'page and header must clear the action buttons');
}
assert.match(source,/sideBarInsetReserved=\{tabletSideNavigation\}/);
assert.match(source,/s.libraryViewport, sideNavigationActive && !tabletSideNavigation/);
assert.match(source,/right: tabletSideNavigation \? 3/);

// Pack the visible cover and controls as one centered group. Reserving an
// invisible full column around the smaller cover biases the outer whitespace.
assert.match(source,/width: landscapePlayer \? tabletLayout.coverSize : "100%"/);
assert.doesNotMatch(source,/width: landscapePlayer \? tabletLayout.columnWidth/);
for(const width of [960,1194,1366,1600])for(const side of ['left','right']) {
 const lane=tabletNavigationLane(0),viewport=width-lane*2;
 const frame=tabletPlayerLayout(viewport,834,250,true,false);
 const group=frame.coverSize+frame.columnGap+frame.controlsWidth;
 const start=(viewport-group)/2;
 const pageLeft=lane;
 const coverLeft=pageLeft+start;
 const controlsRight=coverLeft+group;
 assert(Math.abs((coverLeft-pageLeft)-(pageLeft+viewport-controlsRight))<1e-8,'visible artwork and controls have equal outer space within the page');
}

for(const [left,right] of [[0,0],[24,0],[0,24],[12,28]])for(const width of [1097,1194,1366,1600]) {
 const margins=tabletNavigationPageInsets(left,right);
 assert.equal(margins.marginLeft,margins.marginRight);
 const viewport=width-margins.marginLeft-margins.marginRight;
 assert.equal(margins.marginLeft+viewport/2,width/2,'content center stays on the screen center with the rail on either side');
 assert(margins.marginLeft>=left+96);assert(margins.marginRight>=right+96);
}
assert.match(source,/tabletSideNavigation && tabletNavigationPageInsets\(outerInsets\?\.left \?\? 0, outerInsets\?\.right \?\? 0\)/);

for(const os of ['ios','android'])for(const enabled of [false,true]) {
 assert.equal(isTabletPlayer(os,false,390,844,enabled),enabled,'phone portrait follows the saved style switch');
 assert.equal(isTabletPlayer(os,false,844,390,enabled),false,'phone landscape retains the approved layout');
 assert.equal(isTabletPlayer(os,os==='ios',834,1194,enabled),true,'physical tablets retain their tablet layout');
}
assert.equal(isTabletPlayer('web',false,390,844,true),false);
assert.match(source,/width, height, navigationPreferences.tabletPortraitLayout\)/);

const blurExpression=source.match(/const phoneLandscapeBlur = ([^;]+);/)[1];
const blurEnabled=new Function('tabletDevice','landscapePlayer','navigationPreferences','return '+blurExpression);
for(const tablet of [false,true])for(const landscape of [false,true])for(const enabled of [false,true])assert.equal(blurEnabled(tablet,landscape,{tabletPortraitLayout:enabled}),!tablet&&landscape&&enabled);
assert(source.indexOf('<PhoneLandscapeBlur uri=')<source.indexOf('key={artworkUri}'),'blur is behind the original sharp cover');
assert.match(source,/!phoneLandscapeBlur && <>/,'only the optional phone landscape style suppresses both existing gradient masks');
assert.match(source,/key=\{artworkUri\} source=\{\{ uri: artworkUri \}\} resizeMode="contain".*top: artworkTop, left: artworkLeft, width: artworkWidth, height: artworkSize/,'foreground artwork geometry and contain mode are unchanged');
console.log('Phone style switch: portrait tablet layout, landscape blur-only backdrop, original sharp cover geometry and off-state gradient retained.');

// Tablet artwork/background styles reuse the approved gradient-player typography.
assert.match(source,/style=\{\[np\.title, compactLandscape && np\.compactTitle/);
assert.match(source,/style=\{\[np\.artist, tabletPlayer && \{ marginTop: 7 \}, compactLandscape && np\.compactArtist/);
assert.doesNotMatch(source,/tabletPlayer && \{ fontSize: (?:28|20), lineHeight:/);
