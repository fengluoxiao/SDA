# Android KU100 高解析接入

Android 启动时将 `hrtf-dense/hrtf-set.json` 传入 JNI，使用与 Windows 共用的 `NativeHrtfSet::load_calibrated` 和 `replace_hrtf` 加载 61 方向 KU100。标准 `hrtf` 同时提供扬声器锚点。

Android APK 构建现在直接从 `apps/desktop/native-renderer/hrtf-assets` 打包 `hrtf-dense` 和 `hrtf`，不再使用 Android 源目录里的独立副本。构建检查高解析清单包含 61 方向且经过校准。已有的方向优化随资产原样进入 APK，不重新执行资产优化脚本，也不额外添加均衡或近场处理。

播放状态只有在原生引擎报告 61 方向时显示“高解析”。启动时仍逐文件校验 APK 与应用缓存，修复过期缓存后再加载。

此前 Android 副本已经与当前 Windows 资产逐字节一致。因此本次统一资产来源并不代表发现或修复了听感差异，也不能据此宣称已达到用户记忆中的 Windows 效果。房间仿真与独立近场开关是另外的功能，不能代替高解析 HRTF。
