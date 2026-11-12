# Folia 本地可视化内核

固定 Folia 提交 `a44758bbc02929225ca7304b5a4c4580d7b4e87b` 的 Latent 背景链路，以及该提交锁定的 Paper Shaders 0.0.80。不是重新编写的仿制着色器。

- `upstream/` 保存未修改的 Folia 源码，扩展名 `.source` 防止移动端直接编译浏览器组件。
- `latent.generated.ts` 由 `node scripts/sync-folia-runtime.mjs` 从上游 AST 提取默认参数及颜色、声音响应函数；函数体保持原样。
- `paper-shaders/` 保存未修改的原始运行时代码；Three/Expo 兼容转换只在 SDA 适配层进行。
- `upstream.json` 记录来源、版本、npm 完整性和原文件 SHA-256，验证脚本检查所有原文。
- `LICENSE` 为 Folia 的 GPL-3.0；Paper Shaders 的 Apache-2.0 LICENSE 与 NOTICE 保存在对应目录。分发时应随附这些文件及对应源代码。

SDA 适配层负责共享 GL/VR 合成、尺寸、暂停与后台生命周期、主题数据输入。没有运行上游播放器 UI，没有向浏览器或 AI 发送音频。

AI 主题提示也从 Folia 原文提取（`theme-prompt.generated.ts`），仅附加说明当前输入为用户授权的歌曲元信息；不会自动上传歌词或音频。

音频驱动输入来自 SDA 原生消费者的被动 RMS/频段标量。播放器系统音频旁路没有该数据时使用原版基础运动，不伪造节拍。
