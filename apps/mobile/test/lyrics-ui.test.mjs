import assert from 'node:assert/strict';
import {test} from 'node:test';
import {readFileSync} from 'node:fs';
import {build} from 'esbuild';
const source=readFileSync('apps/mobile/src/PortraitLyricsPager.tsx','utf8');
const compiled=await build({stdin:{contents:source,loader:'tsx',resolveDir:process.cwd()},bundle:true,platform:'node',format:'cjs',external:['*'],write:false});
const modelBundle=await build({entryPoints:['apps/mobile/src/mobile-lyrics.ts'],bundle:true,platform:'node',format:'cjs',write:false});
const modelModule={exports:{}};new Function('module','exports',modelBundle.outputFiles[0].text)(modelModule,modelModule.exports);const lyricModel=modelModule.exports;
function fixture({timed=true,paused=false,busy=false,document:provided,positionMs=2000}={}){
 const calls=[],line={id:'one',text:'歌词',startMs:12345,endMs:20000,words:[]},doc=provided??{timed,lines:[line]};
 const React={createElement:(type,props,...children)=>({type,props:{...props,children}}),useRef:current=>({current}),useState:initial=>[initial,()=>{}],useCallback:fn=>fn,useMemo:fn=>fn(),useEffect(){},useLayoutEffect(){},createContext:()=>({Provider:"LyricsSwipeLockProvider"}),useContext:()=>value=>calls.push(["lock",value])};
 const native={View:'View',Text:'Text',Pressable:'Pressable',ScrollView:'ScrollView',Image:'Image',Animated:{View:'AnimatedView',Value:class{constructor(value){this.value=value;}}},ActivityIndicator:'ActivityIndicator',StyleSheet:{create:x=>x,flatten:value=>Array.isArray(value)?Object.assign({},...value):value},useWindowDimensions:()=>({width:390,height:844})};
 const module={exports:{}};new Function('require','module','exports',compiled.outputFiles[0].text)(name=>name==='react'?React:name==='react-native'?native:name==='react-native-safe-area-context'?{SafeAreaView:'SafeAreaView'}:name.endsWith('KaraokeText')?{KaraokeText:'KaraokeText'}:name.endsWith('use-lyrics-follow')?{useLyricsFollow:()=>({scrollRef:{current:null},viewportHeight:400,index:0,resetManual(){},onViewportLayout(){},onLineLayout(){},onContentSizeChange(){},scrollHandlers:{onScrollBeginDrag(){},onScrollEndDrag(){},onMomentumScrollBegin(){},onMomentumScrollEnd(){}}})}:name.endsWith('use-lyrics-swipe')?{useLyricsSwipe:()=>({panHandlers:{},translation:0,page:0,setBlocked(){}})}:name.endsWith('use-track-lyrics')?{useTrackLyrics:()=>({document:doc,loading:false,importing:false,error:'',importLyrics(){}})}:lyricModel,module,module.exports);
 const player={selectedUri:'file:///song',metadata:{},positionMs,durationMs:30000,playing:true,paused,busy,seekTo:ms=>calls.push(['seek',ms]),togglePause:()=>calls.push(['pause']),play:()=>calls.push(['play'])};
 const tree=module.exports.LyricsPanel({player,active:true,onBack(){}});
 const find=(node,predicate)=>{if(!node||typeof node!=='object')return null;if(predicate(node))return node;for(const child of node.props?.children?.flat(Infinity)??[]){const result=find(child,predicate);if(result)return result;}return null;};
 return {calls,player,find:predicate=>find(tree,predicate),api:module.exports};
}
for(const paused of [false,true])test('lyric tap seeks through the existing transport and preserves '+(paused?'paused':'playing')+' intent',()=>{
 const f=fixture({paused});const row=f.find(node=>node.props.testID==='lyric-line-0');assert.equal(row.props.disabled,false);row.props.onPress();assert.deepEqual(f.calls,[['seek',12345]]);assert.equal(f.player.paused,paused);
});
test('untimed lyrics and busy preparation cannot trigger seeking',()=>{
 for(const options of [{timed:false},{busy:true}]){const f=fixture(options);const row=f.find(node=>node.props.testID==='lyric-line-0');assert.equal(row.props.disabled,true);row.props.onPress();assert.deepEqual(f.calls,[]);}
});
test('slider boundary locks navigation until every slider touch has ended',()=>{
 const f=fixture(),view=f.api.LyricsGestureBoundary({style:{height:20},children:'slider'});
 assert.equal(view.type,'View');assert.deepEqual(view.props.style,{height:20});
 view.props.onTouchStart({nativeEvent:{touches:[{}]}});view.props.onTouchEnd({nativeEvent:{touches:[{}]}});assert.deepEqual(f.calls,[['lock',true]]);
 view.props.onTouchEnd({nativeEvent:{touches:[]}});assert.deepEqual(f.calls,[['lock',true],['lock',false]]);
 view.props.onTouchCancel({nativeEvent:{touches:[]}});assert.deepEqual(f.calls.at(-1),['lock',false]);
});
test('lyrics page contains left-aligned lyrics without a header, import action or playback footer',()=>{
 const f=fixture();assert.equal(f.find(node=>node.props.accessibilityRole==='header'),null);
 for(const label of ['返回播放页','导入 LRC 或 TTML 歌词','播放','暂停'])assert.equal(f.find(node=>node.props.accessibilityLabel===label),null);
 const text=f.find(node=>node.type==='Text');assert.equal(text.props.style[0].textAlign,'left');assert.equal(text.props.children[0],'歌词');
});
test('compact track header shows artwork and metadata over the shared page backdrop',()=>{
 const f=fixture();f.player.metadata={title:'歌曲名',artist:'歌手名',coverUri:'file:///cover.jpg'};
 const tree=f.api.LyricsTrackHeader({player:f.player});assert.equal(tree.props.style.backgroundColor,'transparent');
 assert.equal(tree.props.children[0].type,'Image');assert.equal(tree.props.children[0].props.source.uri,'file:///cover.jpg');
 const labels=tree.props.children[1].props.children;assert.equal(labels[0].props.children[0],'歌曲名');assert.equal(labels[1].props.children[0],'歌手名');
});
test('both headers travel inside the same horizontal canvas at the same height',()=>{
 const f=fixture(),headerHeight=104,provider=f.api.PortraitLyricsPager({player:f.player,enabled:true,visible:true,children:'album',playbackHeader:'playback controls',headerHeight});
 const viewport=provider.props.children[0],canvas=viewport.props.children[0],pages=canvas.props.children,album=pages[0],lyrics=pages[1];
 assert.equal(canvas.type,'AnimatedView');assert.equal(viewport.props.style.marginTop,undefined,'no negative top margin or vertical repositioning while switching pages');
 assert.equal(album.props.style.paddingTop,headerHeight);assert.equal(lyrics.props.style.paddingTop,(headerHeight+54)/2+6,'lyrics reserve the visible metadata row rather than its oversized measurement box');
 const albumHeader=album.props.children[0],lyricsHeader=lyrics.props.children[0];
 assert.equal(albumHeader.props.testID,'album-page-header');assert.equal(lyricsHeader.props.testID,'lyrics-page-header');
 assert.deepEqual(albumHeader.props.style,lyricsHeader.props.style);assert.equal(albumHeader.props.children[0],'playback controls');
 assert.equal(lyricsHeader.props.children[0].type,f.api.LyricsTrackHeader);
});
test('a restored lyric backdrop starts fully blurred on its first render',()=>{
 const f=fixture(),tree=f.api.LyricsBackdrop({visible:true,artworkUri:'file:///cover.jpg',backgroundColor:'#303034'});
 assert.equal(tree.props.style.at(-1).opacity.value,1);assert.equal(tree.props.children[0].props.children[0].props.blurRadius,64);assert.equal(tree.props.children[0].props.children[0].props.source.uri,'file:///cover.jpg');
});

test('landscape slides only the controls column and retains its independent half-width',()=>{
 const f=fixture(),column={width:'50%',paddingLeft:12,paddingRight:25,paddingTop:28};
 const provider=f.api.LandscapeLyricsPager({player:f.player,enabled:true,visible:true,lyricsSelected:false,onLyricsPageChange(){},style:column,children:'transport controls'});
 const root=provider.props.children[0];assert.equal(root.props.testID,'landscape-controls-lyrics');assert.equal(root.props.style[0],column);assert.equal(root.props.style.at(-1).paddingTop,0,'controls-only whitespace cannot shrink the lyric viewport');
 const viewport=root.props.children[0],canvas=viewport.props.children[0],pages=canvas.props.children;
 assert.equal(viewport.props.style.overflow,'hidden');assert.equal(canvas.type,'AnimatedView');
 assert.equal(pages[0].props.testID,'landscape-transport-page');assert.equal(pages[0].props.children[0],'transport controls');assert.equal(pages[0].props.style.width,390/2);assert.equal(pages[0].props.style.paddingTop,28,'transport keeps its approved vertical placement');
 assert.equal(pages[1].props.testID,'landscape-lyrics-page');assert.equal(pages[1].props.style.width,390/2);
 assert.equal(pages[1].props.children[0].type.name,'LyricsPanel');
});
test('the landscape wrapper preserves the approved controls view when portrait is active',()=>{
 const f=fixture(),style={gap:12};const view=f.api.LandscapeLyricsPager({player:f.player,enabled:false,visible:true,lyricsSelected:false,onLyricsPageChange(){},style,children:'controls'});
 assert.equal(view.type,'View');assert.equal(view.props.style,style);assert.equal(view.props.children[0],'controls');
});
test('landscape exposes 24pt more lyrics without moving the playback controls',()=>{
 const f=fixture(),provider=f.api.LandscapeLyricsPager({player:f.player,enabled:true,visible:true,lyricsSelected:true,onLyricsPageChange(){},style:{width:'50%',paddingTop:28},topClearance:24,children:'controls'});
 const viewport=provider.props.children[0].props.children[0],controls=viewport.props.children[0].props.children[0];
 assert.equal(viewport.props.style.marginTop,-24);assert.equal(controls.props.style.paddingTop,52);
 assert.equal(viewport.props.style.marginTop+controls.props.style.paddingTop,28,'extra lyric headroom is cancelled only in the transport page');
});
test('blurred artwork and tint cover both rotation bounds before native parent layout catches up',()=>{
 const f=fixture(),tree=f.api.LyricsBackdrop({visible:true,artworkUri:'file:///cover.jpg',backgroundColor:'#303034'}),frame=tree.props.children[0].props.style;
 assert.equal(tree.props.style.at(-1).overflow,'visible');assert.equal(frame.width,frame.height);
 for(const old of [{width:390,height:844},{width:844,height:390},{width:390,height:0},{width:0,height:390}]){
  const left=old.width/2+frame.marginLeft,top=old.height/2+frame.marginTop;
  assert(left<=0&&top<=0);assert(left+frame.width>=844&&top+frame.height>=844,'no half-background gap for old, new or transient native frames');
 }
 assert.equal(tree.props.children[0].props.children[1].props.style.at(-1).backgroundColor,'rgba(0,0,0,.25)');
});

test('landscape lyric viewport meets the measured Now Playing title line',()=>{
 const f=fixture(),header=104,titleTop=26,columnTop=12,clearance=header+columnTop-titleTop;
 const provider=f.api.LandscapeLyricsPager({player:f.player,enabled:true,visible:true,lyricsSelected:true,onLyricsPageChange(){},style:{width:'50%',paddingTop:28},topClearance:clearance,children:'controls'});
 const viewport=provider.props.children[0].props.children[0],controls=viewport.props.children[0].props.children[0];
 assert.equal(header+columnTop+viewport.props.style.marginTop,titleTop);
 assert.equal(viewport.props.style.marginTop+controls.props.style.paddingTop,28,'the controls retain their original top offset');
});

test('simultaneous TTML singers retain separate highlighted lyric rows without speaker labels',()=>{
 const document=lyricModel.parseLyrics({text:'<tt xmlns:ttm="http://www.w3.org/ns/ttml#metadata"><head><metadata><ttm:agent xml:id="a" type="person"><ttm:name>甲</ttm:name></ttm:agent><ttm:agent xml:id="b" type="person"><ttm:name>乙</ttm:name></ttm:agent></metadata></head><body><p ttm:agent="a" begin="0s" end="5s">同一句</p><p ttm:agent="b" begin="0s" end="5s">同一句</p></body></tt>'});
 const f=fixture({document,positionMs:2000});
 assert.equal(f.find(n=>n.props.testID==='lyric-line-0').props.accessibilityState.selected,true);assert.equal(f.find(n=>n.props.testID==='lyric-line-1').props.accessibilityState.selected,true);
 assert.equal(f.find(n=>n.props.testID==='lyric-speaker-0'),null);assert.equal(f.find(n=>n.props.testID==='lyric-speaker-1'),null);
});
test('background vocal tap seeks its own timing and cannot bubble into the lead line seek',()=>{
 const document=lyricModel.parseLyrics({text:'<tt><body><p begin="0s" end="8s">主唱<span ttm:role="x-bg" begin="2s" end="4s">伴唱</span></p></body></tt>'});
 const f=fixture({document,positionMs:2500}),backing=f.find(n=>n.props.testID==='lyric-backing-0-0');assert(backing);let stopped=false;
 backing.props.onPress({stopPropagation(){stopped=true;}});assert(stopped);assert.deepEqual(f.calls,[['seek',2000]]);
});


const flatStyle=node=>Object.assign({},...node.props.style.flat(Infinity).filter(Boolean));
const duetDocument=()=>lyricModel.parseLyrics({text:'<tt><head><metadata><agent xml:id="a" type="person"><name>甲</name></agent><agent xml:id="b" type="person"><name>乙</name></agent><agent xml:id="g" type="group"><name>合唱</name></agent></metadata></head><body><p agent="a" begin="0s" end="5s">甲<span role="x-translation">甲译</span></p><p agent="b" begin="0s" end="5s">乙<span role="x-translation">乙译</span><span role="x-bg" agent="a" begin="2s" end="4s">乙下面的伴唱</span></p><p agent="b" role="x-chorus" begin="1s" end="3s">乙下面的合唱</p></body></tt>'});
test('duet second singer and translation align right while the first singer stays left',()=>{
 const f=fixture({document:duetDocument(),positionMs:2500});
 for(const [index,side] of [[0,'left'],[1,'right']])for(const id of [`lyric-text-${index}`,`lyric-translation-${index}`])assert.equal(flatStyle(f.find(n=>n.props.testID===id)).textAlign,side);
 assert.equal(f.find(n=>n.props.testID==='lyric-line-2'),null,'chorus is inside its lead, not another full-size row');
});
test('chorus and backing appear smaller under their owner and keep that singers alignment and own seek',()=>{
 const f=fixture({document:duetDocument(),positionMs:3500});
 const lead=flatStyle(f.find(n=>n.props.testID==='lyric-text-1'));
 // Supporting rows are sorted by time: chorus at 1s, backing at 2s.
 for(const [index,time,selected] of [[0,1000,false],[1,2000,true]]){
  const text=f.find(n=>n.props.testID===`lyric-backing-text-1-${index}`),row=f.find(n=>n.props.testID===`lyric-backing-1-${index}`);
  assert.equal(flatStyle(text).textAlign,'right');assert(flatStyle(text).fontSize<lead.fontSize);assert.equal(row.props.accessibilityState.selected,selected);
  let stopped=false;row.props.onPress({stopPropagation(){stopped=true;}});assert(stopped);assert.deepEqual(f.calls.at(-1),['seek',time]);
 }
});
test('unassigned standalone chorus stays visible at a smaller font size',()=>{
 const document=lyricModel.parseLyrics({text:'<tt><body><p begin="0s" end="1s">主唱</p><p role="x-chorus" begin="2s" end="4s">独立合唱</p></body></tt>'});
 const f=fixture({document,positionMs:2500});assert(flatStyle(f.find(n=>n.props.testID==='lyric-text-1')).fontSize<flatStyle(f.find(n=>n.props.testID==='lyric-text-0')).fontSize);
 assert(f.find(n=>n.props.testID==='lyric-line-1').props.accessibilityState.selected);
});


test('vocal roles and singer metadata are never displayed as labels above the lyric text',()=>{
 const f=fixture({document:duetDocument(),positionMs:2500});
 assert.equal(f.find(n=>n.props.testID?.startsWith('lyric-speaker-')),null);
 for(const label of ['甲','乙','合唱','伴唱','声部'])assert.equal(f.find(n=>n.type==='Text'&&n.props.children.length===1&&n.props.children[0]===label&&n.props.testID?.startsWith('lyric-text-')!==true),null);
 assert(f.find(n=>n.props.testID==='lyric-backing-text-1-0'));assert(f.find(n=>n.props.testID==='lyric-backing-text-1-1'));
});


test('staggered duet highlights each side at its own entry and ends both exactly at 6s',()=>{
 const document=lyricModel.parseLyrics({text:'<tt><head><metadata><agent xml:id="a" type="person"/><agent xml:id="b" type="person"/></metadata></head><body><p agent="a" begin="3s" end="6s">甲的歌词</p><p agent="b" begin="5s" end="6s">乙的歌词</p></body></tt>'});
 for(const [time,selected] of [[2999,[false,false]],[3000,[true,false]],[4999,[true,false]],[5000,[true,true]],[5999,[true,true]],[6000,[false,false]]]){
  const f=fixture({document,positionMs:time});
  for(const [index,side] of [[0,'left'],[1,'right']]){
   const row=f.find(n=>n.props.testID===`lyric-line-${index}`),text=f.find(n=>n.props.testID===`lyric-text-${index}`);
   assert.equal(row.props.accessibilityState.selected,selected[index],`${time}ms singer ${index}`);assert.equal(flatStyle(text).textAlign,side);
   assert.equal(flatStyle(text).color,selected[index]?'#ffffff':'rgba(255,255,255,.38)');
  }
 }
 const f=fixture({document,positionMs:5500});
 f.find(n=>n.props.testID==='lyric-line-0').props.onPress();f.find(n=>n.props.testID==='lyric-line-1').props.onPress();assert.deepEqual(f.calls,[['seek',3000],['seek',5000]]);
});


test('LRC translations render smaller below the original and share its highlight and seek',()=>{
 const document=lyricModel.parseLyrics({text:'[0:3.0]風が吹く\n[00:03.000]风在吹\n[0:6.0]Next'});
 for(const time of [2999,3000,5999,6000]){
  const f=fixture({document,positionMs:time}),original=f.find(n=>n.props.testID==='lyric-text-0'),translation=f.find(n=>n.props.testID==='lyric-translation-0');
  assert.equal(translation.props.children[0],'风在吹');assert(flatStyle(translation).fontSize<flatStyle(original).fontSize);
  assert.equal(flatStyle(translation).textAlign,'left');assert.equal(flatStyle(translation).color,time>=3000&&time<6000?'rgba(255,255,255,.8)':'rgba(255,255,255,.35)');
  const row=f.find(n=>n.props.testID==='lyric-line-0');assert(row.props.children.flat(Infinity).includes(translation),'translation belongs to the same press target');row.props.onPress();assert.deepEqual(f.calls,[['seek',3000]]);
 }
});


test('native playback controls retain their bottom safe area while lyrics underlay the floating tab bar',()=>{
 const f=fixture(),root=f.api.PortraitLyricsPager({player:f.player,enabled:true,visible:true,preservePlaybackBottomInset:true,children:'controls',headerHeight:80});
 const pages=root.props.children[0].props.children[0].props.children;
 assert.equal(pages[0].type,'SafeAreaView');assert.deepEqual(pages[0].props.edges,['bottom']);assert.equal(pages[1].type,'View');assert.equal(pages[1].props.edges,undefined);
 const landscape=f.api.PortraitLyricsPager({player:f.player,enabled:false,visible:true,preservePlaybackBottomInset:true,children:'landscape'});
 assert.equal(landscape.type,'SafeAreaView');assert.deepEqual(landscape.props.edges,['bottom']);
});


test('enhanced LRC and TTML share continuous karaoke timing, including backward seeking and translation',()=>{
 const sources=[
  '[0:3.0]<0:3.0>风<0:3.5>吹<0:4.0>过\n[00:03.000]Wind passes\n[0:6.0]下一句',
  '<tt xmlns:itunes="http://music.apple.com/lyric"><body><p begin="3.0" end="6.0"><span begin="3.0" end="3.5">风</span><span begin="3.5" end="4.0">吹</span><span begin="4.0" end="6.0">过</span><span role="x-translation">Wind passes</span></p><p begin="6.0" end="8.0">下一句</p></body></tt>'
 ];
 for(const source of sources){
  const document=lyricModel.parseLyrics({text:source});
  for(const [time,count] of [[3000,1],[3499,1],[3500,2],[3999,2],[4000,3],[5999,3],[3100,1]]){
   const f=fixture({document,positionMs:time}),text=f.find(n=>n.props.testID==='lyric-text-0');
   assert.equal(text.type,'KaraokeText');
   assert.deepEqual(text.props.line.words.map(w=>w.text),['风','吹','过']);
   assert.equal(text.props.positionMs,time);assert.equal(text.props.playing,true);assert.equal(text.props.active,true);
   assert.equal(f.find(n=>n.props.testID==='lyric-translation-0').props.children[0],'Wind passes');
  }
  const ended=fixture({document,positionMs:6000});assert.equal(ended.find(n=>n.props.testID==='lyric-text-0').props.children[0],'风吹过');assert.equal(ended.find(n=>n.props.testID==='lyric-line-0').props.accessibilityState.selected,false);
 }
});
test('ordinary LRC with only line times stays line-synchronized without invented word timing',()=>{
 const document=lyricModel.parseLyrics({text:'[0:3.0]风吹过\n[0:6.0]下一句'});assert.deepEqual(document.lines[0].words,[]);
 const f=fixture({document,positionMs:3500});assert.equal(f.find(n=>n.props.testID==='lyric-text-0').props.children[0],'风吹过');
});

test('duet and backing karaoke keep separate timing and freeze independently when paused',()=>{
 const document=lyricModel.parseLyrics({text:'<tt xmlns:itunes="http://music.apple.com/lyric"><body><p agent="a" begin="3.0" end="8.0"><span begin="3.0" end="8.0">甲</span><span role="x-bg" begin="5.0" end="7.0"><span begin="5.0" end="7.0">和</span></span></p><p agent="b" begin="4.0" end="8.0"><span begin="4.0" end="8.0">乙</span></p></body></tt>'});
 for(const paused of [false,true]){
  const f=fixture({document,positionMs:6000,paused});
  for(const [id,start] of [['lyric-text-0',3000],['lyric-text-1',4000],['lyric-backing-text-0-0',5000]]){
   const text=f.find(n=>n.props.testID===id);assert.equal(text.type,'KaraokeText');assert.equal(text.props.line.words[0].startMs,start);assert.equal(text.props.positionMs,6000);assert.equal(text.props.playing,!paused);
  }
 }
});
