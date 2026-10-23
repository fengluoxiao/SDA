# iOS26 Apple Music Now Playing reference

User requested visual replication after rejecting generic iOS styling. Research inspected:
- Apple official iOS26 guide (version selector verified iOS26): https://support.apple.com/zh-cn/guide/iphone/iph676daac9b/26/ios/26
- Official screenshot: https://help.apple.com/assets/69F8EBBDF3B89A4F6E0C704C/69F8EBC39FA1A6D6FA0F620C/zh_CN/0b3387ce4196b5b4e4ed7f3aeb3bce24.png
- July2025 beta comparison (not final-release specification): https://appleinsider.com/inside/apple-music/vs/apple-music-ios-26-vs-ios-18-automix-liquid-glass-and-lyrics-in-your-language

Observed: full-screen sheet with drag handle, large square rounded cover, cover-colored background, title/artist left with favorite and ellipsis right, progress with elapsed and negative remaining time and centered format, bare SF transport, speaker-ended volume slider, lyrics/output/queue footer. No home title/tab bar in expanded Now Playing. Do not confuse animated lock-screen glass platter or Dribbble concepts with this screen.

Preview-only implementation IOSNowPlaying: fullScreen Modal, local cover blurred background with dark overlay (not exact Apple mesh animation), same transport callbacks, collapse returns library, queue opens existing library, more exposes real replay/mode/settings actions. Favorite/lyrics/output disabled and accessibility labeled unsupported; seeking remains read-only. No Apple cloud library, AutoMix, animated cover or proprietary shader imitation claimed. Uses system SF symbols via ExpoUI in Expo Go/release. Large text/short viewport can scroll instead of clipping; no pixel-identical promise until physical screenshots compared. Audio unchanged. TypeScript and UI regression checks pass. User must preview before compilation; no new build dispatched.


## 用户纠正：只改主页现有播放标签

已撤销新增全屏 Modal/独立 nowPlaying state，删除本轮新增 IOSNowPlaying.tsx 和 IOSPlayerGlassAction.tsx。参考布局/背景/控件直接内联到 IOSPlayer 既有第一个标签。保留主页标题、原底栏和资料库/空间导航；不再出现收起手柄或新播放路由。沿用实际 viewport/固定控件测量限制封面尺寸，保持播放页面不滚动；音量辅助操作/未知时长/更多选项/禁用未支持功能保留。TypeScript 与更新的 UI 回归通过，无音频改动、无新编译。视觉仍须用户 Expo Go 验收。
