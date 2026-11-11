import assert from 'node:assert/strict';
import {test} from 'node:test';
import {readFileSync} from 'node:fs';
import {transformSync} from 'esbuild';
const load=async name=>import('data:text/javascript;base64,'+Buffer.from(transformSync(readFileSync(new URL('../src/'+name,import.meta.url),'utf8'),{loader:'ts',format:'esm',target:'es2022'}).code).toString('base64'));
const {parseLyrics,lyricTime,activeLyricIndex,activeLyricIndices,lyricSpeakerLabel,lyricFollowIndex,lyricPresentationDocument,lyricSpeakerAlignments}=await load('mobile-lyrics.ts');
const {decodeLyricBytes,readEmbeddedLyrics}=await load('embedded-lyrics.ts');
const parse=text=>parseLyrics({text});
const utf8=text=>Buffer.from(text,'utf8');
const u32=n=>{const b=Buffer.alloc(4);b.writeUInt32BE(n);return b;};
const ss=n=>Buffer.from([n>>>21&127,n>>>14&127,n>>>7&127,n&127]);
const frame=(name,payload)=>Buffer.concat([utf8(name),u32(payload.length),Buffer.alloc(2),payload]);
const id3=payload=>Buffer.concat([utf8('ID3'),Buffer.from([3,0,0]),ss(payload.length),payload]);
const atom=(name,...parts)=>{const b=Buffer.concat(parts);return Buffer.concat([u32(b.length+8),Buffer.from(name,'latin1'),b]);};
const read=async data=>readEmbeddedLyrics({size:data.length,read:async(p,n)=>data.subarray(p,p+n)});
test('LRC accepts zero padding, precise decimals, repeated timestamps and hour clocks',()=>{
 const d=parse('[0:1.1]一\n[00:02.01][0:3.001]二\n[01:00:00.000]三');
 assert.deepEqual(d.lines.map(l=>l.startMs),[1100,2010,3001,3600000]);
 assert.equal(d.lines[0].text,'一');assert.equal(lyricTime('00:60.0'),null);
 assert.equal(activeLyricIndex(d,1099),-1);assert.equal(activeLyricIndex(d,2010),1);
});
test('LRC offset, enhanced words and instrumental boundaries preserve the timeline',()=>{
 const d=parse('[offset:100]\n[00:01.00]<00:01.00>你<00:01.50>好\n[00:02.0]\n[00:03]世界');
 assert.equal(d.lines[0].startMs,900);assert.deepEqual(d.lines[0].words.map(w=>[w.startMs,w.endMs]),[[900,1400],[1400,1900]]);
 assert.equal(d.lines[1].text,'');assert.equal(activeLyricIndex(d,2000),1);
});
test('untimed lyrics do not acquire invented seek targets',()=>{
 const d=parse('[ar:作者]\n中文\n日本語');assert.equal(d.timed,false);assert.deepEqual(d.lines.map(l=>l.text),['中文','日本語']);assert.equal(activeLyricIndex(d,100),-1);
});
test('standard TTML inherits relative times, ends, entities, translations and ruby',()=>{
 const d=parse('<tt xmlns="http://www.w3.org/ns/ttml"><body begin="5s"><div><p begin="2s" dur="3s"><span begin="0s" dur="1s">你&amp;我</span><span begin="1s" dur="2s">&#x65e5;本</span><span ttm:role="translation">翻译</span><span tts:ruby="text">annotation</span></p></div></body></tt>');
 assert.equal(d.lines[0].startMs,7000);assert.equal(d.lines[0].endMs,10000);assert.equal(d.lines[0].text,'你&我日本');assert.equal(d.lines[0].translation,'翻译');assert.deepEqual(d.lines[0].words.map(w=>w.startMs),[7000,8000]);
});
test('Apple/AMLL TTML karaoke clocks are absolute, not doubled by the line start',()=>{
 const d=parse('<tt xmlns="http://www.w3.org/ns/ttml" xmlns:itunes="http://music.apple.com/lyric"><body><div><p begin="00:10.000" end="00:12.000"><span begin="00:10.000" end="00:11.000">一</span><span begin="00:11.000" end="00:12.000">二</span></p></div></body></tt>');
 assert.deepEqual(d.lines[0].words.map(w=>[w.startMs,w.endMs]),[[10000,11000],[11000,12000]]);
});
test('TTML sequential paragraphs, frames and tick units',()=>{
 const d=parse('<tt ttp:frameRate="25" ttp:tickRate="100"><body><div timeContainer="seq"><p dur="1s">一</p><p dur="2s">二</p></div></body></tt>');
 assert.deepEqual(d.lines.map(l=>[l.startMs,l.endMs]),[[0,1000],[1000,3000]]);assert.equal(lyricTime('00:00:01:12',25),1480);assert.equal(lyricTime('50t',25,100),500);
});
test('malformed and unsafe XML fails within bounded parsing',()=>{
 assert.throws(()=>parse('<!DOCTYPE tt [<!ENTITY x SYSTEM "file:///secret">]><tt><p>&x;</p></tt>'));
 assert.throws(()=>parse('<tt><body><p begin="invalid">a</p></body></tt>'));
 assert.throws(()=>parse('<tt><body><p>a</body></tt>'));
 assert.throws(()=>parse('a'.repeat(2*1024*1024+1)));
});
test('embedded Unicode decoding supports UTF-8 and both UTF-16 BOM orders',()=>{
 const text='中文 日本語 🎵';assert.equal(decodeLyricBytes(utf8(text)),text);
 const le=Buffer.from(text,'utf16le');assert.equal(decodeLyricBytes(Buffer.concat([Buffer.from([255,254]),le])),text);
 const be=Buffer.from(le);be.swap16();assert.equal(decodeLyricBytes(Buffer.concat([Buffer.from([254,255]),be])),text);
});
test('ID3 USLT and TXXX preserve Unicode; unrelated text frames are ignored',async()=>{
 const sources=await read(id3(Buffer.concat([frame('TXXX',Buffer.concat([Buffer.from([3]),utf8('COMMENT\0unrelated')])),frame('USLT',Buffer.concat([Buffer.from([3]),utf8('eng\0[00:01]中文')])),frame('TXXX',Buffer.concat([Buffer.from([3]),utf8('TTML\0<tt><p begin="1s">日本語</p></tt>')]))])));
 assert.equal(sources.length,2);assert.equal(sources[0].text,'[00:01]中文');assert(sources[1].text.includes('日本語'));
});
test('ID3 SYLT millisecond timestamps become timed lyrics',async()=>{
 const payload=Buffer.concat([Buffer.from([3]),utf8('eng'),Buffer.from([2,1,0]),utf8('一\0'),u32(1234),utf8('二\0'),u32(2345)]);
 const sources=await read(id3(frame('SYLT',payload)));assert.deepEqual(parse(sources[0].text).lines.map(l=>l.startMs),[1234,2345]);
});
test('MP4 reads lyric tags after a large mdat without reading audio data',async()=>{
 const ftyp=atom('ftyp',utf8('M4A '),Buffer.alloc(4));const gap=500*1024*1024;
 const moov=atom('moov',atom('udta',atom('meta',Buffer.alloc(4),atom('ilst',atom('©lyr',atom('data',u32(1),u32(0),utf8('[0:01]中文')))))));
 const mdatAt=ftyp.length,moovAt=mdatAt+gap,size=moovAt+moov.length;let bytes=0;
 const sources=await readEmbeddedLyrics({size,read:async(p,n)=>{bytes+=n;if(p<ftyp.length)return ftyp.subarray(p,p+n);if(p===mdatAt&&n===8)return Buffer.concat([u32(gap),utf8('mdat')]);assert(p>=moovAt,'reader must skip all audio bytes');return moov.subarray(p-moovAt,p-moovAt+n);}});
 assert.equal(sources[0].text,'[0:01]中文');assert(bytes<1024);
});
test('ID3 skips unrelated large artwork instead of reading whole tags',async()=>{
 const big=10*1024*1024,lyric=frame('USLT',Buffer.concat([Buffer.from([3]),utf8('eng\0[0:1]OK')]));
 const head=Buffer.concat([utf8('ID3'),Buffer.from([3,0,0]),ss(10+big+lyric.length),utf8('APIC'),u32(big),Buffer.alloc(2)]);let bytes=0;
 const sources=await readEmbeddedLyrics({size:20+big+lyric.length,read:async(p,n)=>{bytes+=n;if(p<head.length)return head.subarray(p,p+n);assert(p>=20+big);return lyric.subarray(p-20-big,p-20-big+n);}});
 assert.equal(sources[0].text,'[0:1]OK');assert(bytes<256);
});
test('malformed MP4 sizes cannot loop or read outside the file',async()=>{
 const bad=Buffer.concat([u32(0xffffffff),utf8('ftyp'),Buffer.alloc(12)]);assert.deepEqual(await read(bad),[]);
});
test('globally unsynchronized ID3v2.3 uses logical offsets after an earlier stuffed frame',async()=>{
 const raw=Buffer.concat([frame('APIC',Buffer.from([255,224,0])),frame('USLT',Buffer.concat([Buffer.from([3]),utf8('eng\0[0:1]正常')]))]);
 const stuffed=Buffer.from([...raw].flatMap((v,i)=>v===255&&(raw[i+1]===0||raw[i+1]>=224)?[v,0]:[v]));
 const data=Buffer.concat([utf8('ID3'),Buffer.from([3,0,128]),ss(stuffed.length),stuffed]);const sources=await read(data);assert.equal(sources[0].text,'[0:1]正常');
});
test('TTML sequential word spans follow the previous word end',()=>{
 const d=parse('<tt><body><p begin="1s" end="4s" timeContainer="seq"><span dur="1s">一</span><span dur="2s">二</span></p></body></tt>');
 assert.deepEqual(d.lines[0].words.map(w=>[w.startMs,w.endMs]),[[1000,2000],[2000,4000]]);
});

const voicesHead='<head><metadata><ttm:agent xml:id="a" type="person"><ttm:name>甲</ttm:name></ttm:agent><ttm:agent xml:id="b" type="person"><ttm:name>乙</ttm:name></ttm:agent><ttm:agent xml:id="g" type="group"><ttm:name>合唱</ttm:name></ttm:agent></metadata></head>';
const voiceTTML=body=>'<tt xmlns="http://www.w3.org/ns/ttml" xmlns:ttm="http://www.w3.org/ns/ttml#metadata" xmlns:itunes="http://music.apple.com/lyric-ttml-internal">'+voicesHead+'<body>'+body+'</body></tt>';
test('duets retain singers and overlapping lines, including identical unison text',()=>{
 const d=parse(voiceTTML('<div><p ttm:agent="a" begin="0.0" end="10.0">同一句</p><p ttm:agent="b" begin="0.0" end="3.0">同一句</p></div>'));
 assert.equal(d.lines.length,2);assert.deepEqual(d.lines.map(l=>l.agentIds),[['a'],['b']]);assert.deepEqual(activeLyricIndices(d,2000),[0,1]);
 assert.equal(activeLyricIndex(d,5000),0,'earlier voice remains active after the shorter overlapping voice ends');assert.equal(lyricSpeakerLabel(d,d.lines[0]),'甲');assert.equal(lyricSpeakerLabel(d,d.lines[1]),'乙');
});
test('group agents and multiple agent references identify chorus without hardcoding speaker ids',()=>{
 const d=parse(voiceTTML('<div><p ttm:agent="g" begin="10.0" end="12.0">合唱一</p><p ttm:agent="a b" begin="12.0" end="14.0">合唱二</p></div>'));
 assert.deepEqual(d.lines.map(l=>l.vocal),['chorus','chorus']);assert.equal(lyricSpeakerLabel(d,d.lines[0]),'合唱');assert.equal(lyricSpeakerLabel(d,d.lines[1]),'合唱 · 甲 / 乙');
});
test('body/div speaker inheritance and inline voice spans preserve separate timelines',()=>{
 const d=parse(voiceTTML('<div ttm:agent="a"><p begin="1.0" end="3.0">继承甲</p><p begin="4.0" end="10.0"><span ttm:agent="a" begin="4.0" end="9.0">甲声部</span><span ttm:agent="b" begin="5.0" end="7.0">乙声部</span></p></div>'));
 assert.equal(d.lines[0].agentIds[0],'a');assert.deepEqual(d.lines.slice(1).map(l=>[l.agentIds[0],l.startMs,l.endMs]),[['a',4000,9000],['b',5000,7000]]);assert.deepEqual(activeLyricIndices(d,6000),[1,2]);
});
test('background vocals keep independent text, translation and word timing instead of merging into lead',()=>{
 const d=parse(voiceTTML('<div><p ttm:agent="a" begin="20.0" end="25.0"><span begin="20.0" end="22.0">主唱</span><span ttm:role="x-bg" ttm:agent="b" begin="22.5" end="23.8"><span begin="22.5" end="23.8">伴唱</span><span ttm:role="x-translation">伴唱翻译</span></span></p></div>'));
 const lead=d.lines[0],bg=lead.background[0];assert.equal(lead.text,'主唱');assert.equal(lead.words.length,1);assert.equal(bg.text,'伴唱');assert.deepEqual([bg.startMs,bg.endMs],[22500,23800]);assert.equal(bg.translation,'伴唱翻译');assert.deepEqual(bg.agentIds,['b']);assert.equal(bg.words[0].startMs,22500);assert.equal(lyricSpeakerLabel(d,bg),'伴唱 · 乙');
});
test('bare seconds use actual decimal precision and Apple word timing stays absolute',()=>{
 assert.equal(lyricTime('12.3'),12300);
 const d=parse(voiceTTML('<div><p ttm:agent="a" begin="10.0" end="12.0"><span begin="10.0" end="11.0">一</span><span begin="11.0" end="12.0">二</span></p></div>'));
 assert.deepEqual(d.lines[0].words.map(w=>w.startMs),[10000,11000]);
});
test('a later background vocal remains active after the lead words finish',()=>{
 const d=parse(voiceTTML('<div><p ttm:agent="a" begin="0.0"><span begin="0.0" end="2.0">主唱先结束</span><span ttm:role="x-bg" begin="3.0" end="5.0">伴唱继续</span></p></div>'));
 assert.equal(d.lines[0].endMs,2000);assert.equal(activeLyricIndex(d,4000),0);assert.equal(d.lines[0].background[0].endMs,5000);
});
test('instrumental gaps retain a nearby follow anchor without highlighting a silent lyric',()=>{
 const d=parse('<tt><body><p begin="10s" end="12s">一</p><p begin="20s" end="22s">二</p></body></tt>');
 assert.equal(activeLyricIndex(d,15000),-1);assert.equal(lyricFollowIndex(d,15000),0);assert.equal(lyricFollowIndex(d,0),0);assert.equal(lyricFollowIndex(d,30000),1);
});


test('duet sides use declared singer order and ignore backing-only metadata',()=>{
 const d=parse('<tt><head><metadata><agent xml:id="extra" type="person"/><agent xml:id="first" type="person"/><agent xml:id="second" type="person"/></metadata></head><body><p agent="second" begin="0s" end="2s">乙先唱</p><p agent="first" begin="2s" end="4s">甲后唱</p><p agent="extra" role="x-bg" begin="0s" end="2s">伴唱</p></body></tt>');
 assert.deepEqual([...lyricSpeakerAlignments(d)],[['first','left'],['second','right']]);
 const undeclared=parse('<tt><body><p agent="solo1" begin="0s" end="2s">甲</p><p agent="solo2" begin="2s" end="4s">乙</p></body></tt>');
 assert.deepEqual([...lyricSpeakerAlignments(undeclared)],[['solo1','left'],['solo2','right']]);
});
test('chorus paragraphs group under the unique overlapping lead without mutating source timelines',()=>{
 const d=parse(voiceTTML('<p agent="a" begin="0s" end="3s">主唱</p><p agent="g" begin="2s" end="5s">合唱</p><p agent="b" begin="6s" end="8s">乙</p>'));
 const before=JSON.stringify(d),view=lyricPresentationDocument(d);
 assert.equal(JSON.stringify(d),before);assert.equal(d.lines.length,3);assert.equal(view.lines.length,2);
 assert.equal(view.lines[0].background[0],d.lines[1]);assert.equal(view.lines[0].background[0].vocal,'chorus');
 assert.equal(activeLyricIndex(view,4000),0,'backing remains active after lead words end');assert.equal(lyricFollowIndex(view,6500),1,'follow indices match displayed rows');
});
test('overlapping duet chorus uses speaker references only when the owner is unambiguous',()=>{
 const d=parse(voiceTTML('<p agent="a" begin="0s" end="5s">甲</p><p agent="b" begin="0s" end="5s">乙</p><p agent="b" role="x-chorus" begin="1s" end="3s">乙的合唱</p><p agent="g" begin="1s" end="3s">未指定归属</p>'));
 const view=lyricPresentationDocument(d);assert.equal(view.lines.length,3);assert.equal(view.lines[1].background[0].text,'乙的合唱');
 assert.equal(view.lines[2].text,'未指定归属');assert.equal(view.lines[0].background,undefined);
});
test('standalone chorus in a gap remains visible and is never attached to a previous verse',()=>{
 const d=parse(voiceTTML('<p agent="a" begin="0s" end="1s">主唱</p><p agent="g" begin="2s" end="3s">独立合唱</p>'));
 assert.equal(lyricPresentationDocument(d),d);assert.equal(activeLyricIndex(d,2500),1);
});
test('inline explicit chorus keeps independent timing, text and translation under its parent',()=>{
 const d=parse(voiceTTML('<p agent="b" begin="0s" end="5s">乙主唱<span agent="g" role="x-chorus" begin="1s" end="3s">合唱<span role="x-translation">翻译</span></span></p>'));
 assert.equal(d.lines.length,1);assert.equal(d.lines[0].text,'乙主唱');const backing=d.lines[0].background[0];
 assert.equal(backing.vocal,'chorus');assert.equal(backing.text,'合唱');assert.equal(backing.translation,'翻译');assert.deepEqual([backing.startMs,backing.endMs],[1000,3000]);
});


test('staggered duet entries at 3s and 5s both stay active until the shared 6s end',()=>{
 for(const body of [
  '<p agent="a" begin="3s" end="6s">甲</p><p agent="b" begin="5s" end="6s">乙</p>',
  '<p begin="0s" end="6s"><span agent="a" begin="3s" end="6s">甲</span><span agent="b" begin="5s" end="6s">乙</span></p>',
 ]){
  const d=lyricPresentationDocument(parse(voiceTTML(body)));
  assert.deepEqual(d.lines.map(l=>[l.text,l.startMs,l.endMs]),[['甲',3000,6000],['乙',5000,6000]],'entering voice must not truncate the earlier voice');
  for(const [time,expected] of [[2999,[]],[3000,[0]],[4999,[0]],[5000,[0,1]],[5999,[0,1]],[6000,[]]])assert.deepEqual(activeLyricIndices(d,time),expected);
  assert.equal(lyricFollowIndex(d,5500),0,'overlap keeps the lead anchor rather than scrolling away from the earlier singer');
 }
});
test('preview contains staggered 3s/5s entries ending at 6s and a simultaneous duet example',()=>{
 const fixture=readFileSync(new URL('../dev-preview/fixture.ts',import.meta.url),'utf8');
 const sample=fixture.match(/const duetLyrics = `([\s\S]*?)`;/)[1],d=lyricPresentationDocument(parse(sample));
 assert.deepEqual(d.lines.slice(0,2).map(l=>[l.agentIds[0],l.startMs,l.endMs]),[['a',3000,6000],['b',5000,6000]]);
 assert.deepEqual(activeLyricIndices(d,5500),[0,1]);assert.deepEqual(activeLyricIndices(d,6000),[]);assert.deepEqual(activeLyricIndices(d,9000),[2,3]);
});


test('bilingual LRC pairs equal zero-padded timestamps and keeps offset and original word timing',()=>{
 const d=parse('[offset:100]\n[0:3.0]<0:3.0>風が<0:4.0>吹く\n[00:03.000]风在吹\n[00:03.00]风在吹\n[0:6.0]聞こえる\n[00:06.000]听得见\n[0:8.0]');
 assert.equal(d.lines.length,3);assert.equal(d.lines[0].text,'風が吹く');assert.equal(d.lines[0].translation,'风在吹');
 assert.deepEqual([d.lines[0].startMs,d.lines[0].endMs],[2900,5900]);assert.deepEqual(d.lines[0].words.map(w=>[w.startMs,w.endMs]),[[2900,3900],[3900,5900]]);
 assert.equal(d.lines[1].translation,'听得见');assert.equal(d.lines[2].text,'');assert.deepEqual(activeLyricIndices(d,3500),[0]);
});
test('bilingual LRC pairs repeated refrains across separate original and translated blocks',()=>{
 const d=parse('[0:1][0:5]Original\n[0:3]Middle\n[00:01.00][00:05.000]译文\n[00:03.00]中间\n[00:07.00]');
 assert.deepEqual(d.lines.map(l=>[l.startMs,l.text,l.translation]),[[1000,'Original','译文'],[3000,'Middle','中间'],[5000,'Original','译文'],[7000,'',undefined]]);
 const same=parse('[0:1]\n[00:01.00]Original\n[0:1]译文\n[0:1]\n[0:2]Next');assert.equal(same.lines[0].text,'Original');assert.equal(same.lines[0].translation,'译文');assert.equal(same.lines[0].endMs,2000);
});
test('TTML Apple head translations resolve keys without changing sung text or word timing',()=>{
 const d=parse('<tt xmlns:itunes="http://music.apple.com/lyric-ttml-internal"><head><metadata><iTunesMetadata><translations><translation xml:lang="zh-Hans"><text for="L1">风在吹</text><text for="L2">听得见</text><text for="unused">无关译文</text></translation></translations><transliterations><transliteration><text for="L1">kaze ga fuku</text></transliteration></transliterations></iTunesMetadata></metadata></head><body><p itunes:key="L1" begin="3.0" end="6.0"><span begin="3.0" end="5.0">風が</span><span begin="5.0" end="6.0">吹く</span><span role="x-translation">风在吹</span></p><p itunes:key="L2" begin="6.0" end="8.0">聞こえる</p></body></tt>');
 assert.equal(d.lines.length,2);assert.equal(d.lines[0].text,'風が吹く');assert.equal(d.lines[0].translation,'风在吹');assert.equal(d.lines[0].romanization,'kaze ga fuku');assert.equal(d.lines[1].translation,'听得见');
 assert.deepEqual(d.lines[0].words.map(w=>[w.startMs,w.endMs]),[[3000,5000],[5000,6000]]);
});
test('TTML head and inline supporting translations stay under their own singer',()=>{
 const d=parse('<tt><head><metadata><translations><translation><text for="backing">合唱翻译</text></translation></translations></metadata></head><body><p agent="b" begin="0s" end="5s">Original<span role="x-translation">原文翻译</span><span key="backing" role="x-bg" begin="2s" end="4s">Backing</span></p></body></tt>');
 assert.equal(d.lines[0].translation,'原文翻译');assert.equal(d.lines[0].background[0].translation,'合唱翻译');assert.equal(d.lines[0].background[0].text,'Backing');
});

test('TTML original-only transliteration does not create a duplicate small-text layer',()=>{
 const d=parse('<tt xmlns:itunes="http://music.apple.com/lyric"><head><metadata><transliterations><transliteration xml:lang="en"><text for="L1">One original lyric line</text></transliteration></transliterations></metadata></head><body><p itunes:key="L1" begin="1.0" end="4.0"><span begin="1.0" end="2.0">One original </span><span begin="2.0" end="4.0">lyric line</span></p></body></tt>');
 const line=d.lines[0];assert.equal(line.text,'One original lyric line');assert.equal(line.translation,undefined);
 assert.deepEqual(line.words.map(w=>[w.text,w.startMs,w.endMs]),[['One original ',1000,2000],['lyric line',2000,4000]]);
});
test('TTML inline and header annotations remove self-copies and deduplicate real translations conservatively',()=>{
 const d=parse('<tt><head><metadata><translations><translation><text for="L1">Don’t   wait</text><text for="L1">不要等待</text></translation></translations></metadata></head><body><p key="L1" begin="1s" end="3s">Don&apos;t wait<span role="x-translation">Don’t wait</span><span role="x-translation">不要等待</span></p></body></tt>');
 assert.equal(d.lines[0].text,"Don't wait");assert.equal(d.lines[0].translation,'不要等待');
 const distinct=parse('<tt><body><p begin="0s" end="2s">Stay!<span role="x-translation">Stay?</span></p><p begin="2s" end="3s">US<span role="x-translation">us</span></p></body></tt>');
 assert.equal(distinct.lines[0].translation,'Stay?');assert.equal(distinct.lines[1].translation,'us');
});
test('TTML wrapped or canonically equivalent original annotations disappear without hiding a real English translation',()=>{
 const d=parse('<tt><body><p begin="0s" end="2s">Café line<span role="x-roman">Cafe&#x301;<br/>line</span></p><p begin="2s" end="4s">風が吹く<span role="x-translation">The wind blows</span><span role="x-roman">kaze ga fuku</span></p></body></tt>');
 assert.equal(d.lines[0].translation,undefined);assert.equal(d.lines[1].translation,'The wind blows');assert.equal(d.lines[1].romanization,'kaze ga fuku');
});
test('same sung text in backing vocals remains independently timed while only its redundant annotation is hidden',()=>{
 const d=parse('<tt><body><p agent="a" begin="0s" end="5s">Echo<span role="x-translation">Echo</span><span role="x-bg" agent="b" begin="2s" end="4s">Echo<span role="x-translation">Echo</span></span></p></body></tt>');
 assert.equal(d.lines[0].text,'Echo');assert.equal(d.lines[0].translation,undefined);assert.equal(d.lines[0].background.length,1);
 const backing=d.lines[0].background[0];assert.equal(backing.text,'Echo');assert.equal(backing.translation,undefined);assert.deepEqual([backing.startMs,backing.endMs],[2000,4000]);assert.deepEqual(backing.agentIds,['b']);
});
test('split TTML duet lines do not inherit a duplicate of the combined original',()=>{
 const d=parse(voiceTTML('<p begin="0s" end="4s"><span agent="a" begin="0s" end="3s">One </span><span agent="b" begin="1s" end="4s">Two</span><span role="x-translation">One Two</span><span role="x-translation">一和二</span></p>'));
 assert.deepEqual(d.lines.map(l=>[l.text,l.translation,l.startMs,l.endMs]),[['One','一和二',0,3000],['Two','一和二',1000,4000]]);
 assert.deepEqual(d.lines.map(l=>l.agentIds),[['a'],['b']]);
});

test('TTML with only a Jyutping transliteration table never invents translation rows',()=>{
 const d=parse('<tt xmlns:itunes="http://music.apple.com/lyric-ttml-internal"><head><metadata><transliterations><transliteration xml:lang="zh-Latn-jyutping"><text for="L1">One original line</text><text for="L2">nei hou</text></transliteration></transliterations></metadata></head><body><p itunes:key="L1" begin="1.0" end="3.0"><span begin="1.0" end="2.0">One original </span><span begin="2.0" end="3.0">line</span></p><p itunes:key="L2" begin="3.0" end="5.0">你好</p></body></tt>');
 assert(d.lines.every(line=>line.translation===undefined));assert.equal(d.lines[0].romanization,undefined);assert.equal(d.lines[1].romanization,'nei hou');
 assert.deepEqual(d.lines[0].words.map(w=>[w.startMs,w.endMs]),[[1000,2000],[2000,3000]]);
});
test('plain TTML and romanization roles are never classified as translated vocals',()=>{
 const plain=parse('<tt><body><p begin="1s" end="3s"><span begin="1s" end="2s">One </span><span begin="2s" end="3s">line</span></p></body></tt>');assert.equal(plain.lines[0].translation,undefined);
 const inline=parse('<tt><body><p begin="1s" end="3s">你好<span role="x-roman">nei hou</span></p></body></tt>');assert.equal(inline.lines[0].text,'你好');assert.equal(inline.lines[0].translation,undefined);assert.equal(inline.lines[0].romanization,'nei hou');
});

test('TTML annotation parsing remains usable on hosts without String normalization support',()=>{
 const normalize=String.prototype.normalize;
 try {
  String.prototype.normalize=undefined;
  const d=parse('<tt><body><p begin="1s" end="3s">Original<span role="x-translation">Original</span><span role="x-roman">Original</span></p></body></tt>');
  assert.equal(d.lines[0].text,'Original');assert.equal(d.lines[0].translation,undefined);assert.equal(d.lines[0].romanization,undefined);
 } finally { String.prototype.normalize=normalize; }
});
