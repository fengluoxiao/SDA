# How Love 目标段元数据边界核对

前一轮完成短时残响回归和限幅归因，属于进展；本轮继续检查解码元数据，不重做相同频谱/重复候选，不修改生产默认。

## 实际核对

入口 `scripts/diag-how-love-metadata-boundary.py` 已运行通过。输出 `C:/Users/legendshop/Downloads/SDA-clarity-validation/metadata-route-audit.json`。

三首现有完整回放事件：How目标55–63s3750条、Very0–65s30465条、Lose重置fixture0–138s64710条；这些窗口没有非零size、显式distance或非零gainDb。窗口为已解码事件统计，不能证明原码流所有可能的扩展字段已实现。

代码核对：
- `packages/core/src/lib.rs` ObjectEvent.diffuse明确为MPEG-H diffuse energy；`eac3_pipeline.rs`赋0不等于丢掉OAMD extent。
- EAC3解析器 `harletty-bridge/eac3/src/eac3dec/metadata.rs` 的跨帧状态在 `write_to` 中将已解析gain/size/distance/position写回；不能仅凭bridge的`unwrap_or`猜测该窗口一直重置尺寸。
- `NativeObjectEvent`承载size/diffuse/distance/ramp；不会将未传输的物理距离从xyz半径自行推断。

## 原OAMD证据

核对现有 raw-bit oracle target audit 共251条与55–63s重叠的观测。注意旧导出字段`parse_error`在成功时保存字符串`accepted`；不能把非null误计为251个错误。parse/vendor结果均为251条accepted。

每条可定位bitfield：warp_mode=0、global_trim_mode=0、disable_trim_per_object_present=0。

本地参考解析器 `truehd-0.6.3/src/structs/oamd.rs` 在global_trim_mode==2才读取逐配置的自定义trim值。不能把报告的trim_config_count=9当成九个已测得的非零调节，也不能从mode0自行推断默认trim数值为0。既有vendor trim timeline unavailable/fidelity eligible=false仍保留：此次是边界核对，不升级解码器完整认证、也不宣称Apple等效。

结论：目前没有该窗口遗漏非零size/distance/gain或显式自定义trim的可验证证据；没有据此加boost、距离模拟或渲染模式猜测。How Love顶部清晰度仍未完成验收；残响-12候选已有局部平衡风险且尚无听感确认，不升级正式默认。
