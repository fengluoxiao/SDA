import { Image } from "react-native";
import type { QueueTrack } from "../src/RemotePlayer";

// Imported only inside the __DEV__ + Expo Go gate in App.
export function createArtworkPreview() {
  const metadata = { title: "エガクミライ", artist: "MyGO!!!!!",
    coverUri: Image.resolveAssetSource(require("./egaku-cover.jpg"))?.uri ?? "",
    artworkColor: "rgb(63,78,63)", durationMs: 240000,
    lyrics: [{ source: "preview" as const, format: "ttml" as const, name: "歌词交互预览.ttml", text: `<tt xmlns="http://www.w3.org/ns/ttml" xmlns:itunes="http://music.apple.com/lyric-ttml-internal"><body><div>
<p begin="0:0.0" end="00:05.00"><span begin="0:0.0" end="0:2.0">歌词</span><span begin="0:2.0" end="00:05.00">界面预览</span></p>
<p begin="00:05.00" end="00:10.000">轻点这一行，跳转到五秒</p>
<p begin="0:10.000" end="00:16.00">播放时跟随当前位置</p>
<p begin="00:16.00" end="00:22.000">向右滑动，返回播放页</p>
<p begin="0:22.0" end="00:30.00">支持导入外部歌词文件</p>
<p begin="00:30.00" end="00:40.000">支持 LRC 和 TTML</p>
<p begin="0:40.0" end="01:00.00">这里只展示原创交互提示</p>
<p begin="01:00.00" end="04:00.00">Expo Go 界面预览，不输出原生渲染音频</p>
</div></body></tt>` }] };
  const duetLyrics = `<tt xmlns="http://www.w3.org/ns/ttml" xmlns:ttm="http://www.w3.org/ns/ttml#metadata" xmlns:itunes="http://music.apple.com/lyric-ttml-internal"><head><metadata>
<ttm:agent xml:id="a" type="person"><ttm:name>声部甲</ttm:name></ttm:agent><ttm:agent xml:id="b" type="person"><ttm:name>声部乙</ttm:name></ttm:agent><ttm:agent xml:id="group" type="group"><ttm:name>合唱</ttm:name></ttm:agent>
<iTunesMetadata xmlns="http://music.apple.com/lyric-ttml-internal"><translations><translation xml:lang="en"><text for="duet-b">Can you hear me?</text></translation></translations></iTunesMetadata>
</metadata></head><body><div>
<p ttm:agent="a" begin="3.0" end="6.0"><span begin="3.0" end="5.0">风吹过长街，</span><span begin="5.0" end="6.0">我还在等你</span><span ttm:role="x-translation" xml:lang="en">Wind passes the long street; I am still waiting for you.</span></p>
<p itunes:key="duet-b" ttm:agent="b" begin="5.0" end="6.0">你是否听见</p>
<p ttm:agent="a" begin="8.0" end="13.0">在同一个梦里相遇</p><p ttm:agent="b" begin="8.0" end="13.0">在同一个梦里相遇</p>
<p ttm:agent="a" begin="13.0" end="20.0">我唱着没说完的心愿</p>
<p ttm:agent="group" begin="14.0" end="19.0">把心愿唱给你<span ttm:role="x-translation" xml:lang="en">I sing my wishes to you.</span></p>
<p ttm:agent="b" begin="20.0" end="29.0"><span begin="20.0" end="29.0">沿着微光走向你</span><span ttm:role="x-bg" ttm:agent="a" begin="23.0" end="27.0"><span begin="23.0" end="25.0">我会在这里</span><span begin="25.0" end="27.0">等你</span></span></p>
<p ttm:agent="b" begin="29.0" end="240.0">让旋律穿过夜色拥抱你</p>
<p ttm:agent="a b" begin="30.0" end="240.0">每个明天都有你</p>
</div></body></tt>`;
  const followLyrics = "[00:00.000]这一句持续一分钟，滚动松手后四秒回到这里\n" + Array.from({ length: 18 }, (_, i) => {
    const seconds = 60 + i * 8; return `[${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}.00]第 ${i + 2} 行，点击或切回 Tab 验证当前进度`;
  }).join("\n");
  const bilingualLyrics = "[0:3.0]風が吹く\n[00:03.000]风在吹\n[0:6.0]君の声が聞こえる\n[00:06.000]听见了你的声音\n[0:10.0]同じ空を見上げて\n[00:10.000]抬头望着同一片天空\n[0:16.0]一緒に歩こう\n[00:16.000]一起向前走吧\n[0:24.0]";
  const wordLyrics = "[0:3.0]<0:3.0>风<0:3.5>吹<0:4.0>过<0:4.5>长<0:5.0>街\n[00:03.000]The wind passes along the street.\n[0:6.0]<0:6.0>我<0:6.5>还<0:7.0>在<0:7.5>等<0:8.0>你\n[00:06.000]I am still waiting for you.\n[0:10.0]";
  const queue: QueueTrack[] = Array.from({ length: 12 }, (_, index) => ({
    contentHash: `expo-artwork-preview-${index}`, uri: `preview://artwork/${index}`,
    name: index === 0 ? "05. エガクミライ.m4a" : index === 1 ? "对唱与合唱测试.m4a" : index === 2 ? "四秒自动回正测试.m4a" : index === 3 ? "LRC双语翻译测试.m4a" : index === 4 ? "LRC逐字与翻译测试.m4a" : `界面预览 ${index + 1}.m4a`,
    metadata: index === 0 ? metadata : index === 1 ? { ...metadata, title: "对唱与合唱测试", artist: "声部甲 / 声部乙", lyrics: [{ source: "preview", format: "ttml", text: duetLyrics }] } : index === 2 ? { ...metadata, title: "四秒自动回正测试", lyrics: [{ source: "preview", format: "lrc", text: followLyrics }] } : index === 3 ? { ...metadata, title: "LRC双语翻译测试", lyrics: [{ source: "preview", format: "lrc", text: bilingualLyrics }] } : index === 4 ? { ...metadata, title: "LRC逐字与翻译测试", lyrics: [{ source: "preview", format: "lrc", text: wordLyrics }] } : { ...metadata, title: `界面预览 ${index + 1}` },
  }));
  return { queue, queueIndex: 0, selectedUri: queue[0]!.uri, fileName: queue[0]!.name, metadata,
    positionMs: 0, durationMs: 240000, decodedMs: 240000, sourceCodec: "eac3",
    renderingStatus: "Dolby Atmos", paused: true, playing: false, error: null };
}
