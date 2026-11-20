#!/bin/bash
set -euo pipefail
# Run before pod install. This is a device measurement build; reconstruction and
# Metal resources execute on the phone without a desktop/server helper.
export SDA_FUSION_DEVICE_ONLY=1
out="$PWD/ios-build/personal-scan"
bash scripts/build-ios-personal-scan-fusion.sh "$out"
module="$PWD/apps/mobile/modules/sda-core/ios"
test ! -e "$module/StandardCyborgFusion.xcframework"
cp -R "$out/StandardCyborgFusion.xcframework" "$module/StandardCyborgFusion.xcframework"
python3 - "$out" "$module/Licenses/StandardCyborg" <<'PY'
from pathlib import Path
import sys,shutil,json,hashlib
out,dest=map(Path,sys.argv[1:]);sources=list(out.glob('fusion-source.*'))
assert len(sources)==1
source=sources[0];dest.mkdir(parents=True,exist_ok=False)
notices=[]
for p in sorted(source.rglob('*')):
    if not p.is_file() or '.git' in p.parts or '.build' in p.parts:continue
    if not any(p.name.lower().startswith(x)for x in ['license','copying','notice','copyright','authors']):continue
    assert p.stat().st_size <= 2*1024*1024
    relative=p.relative_to(source);target=dest/relative
    target.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,target)
    notices.append({'path':relative.as_posix(),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()})
assert notices,'No reconstruction dependency notices found'
provenance=json.loads((out/'fusion-provenance.json').read_text())
provenance.update(notices=notices,measurementOnly=True,personalGenerationReady=False)
(dest/'provenance.json').write_text(json.dumps(provenance,indent=2))
Path('ios-artifacts/personal-scan-framework.json').write_text(json.dumps(provenance,indent=2))
PY
cat > ios-artifacts/PERSONAL-HRTF-TEST-README-zh.md <<'EOF'
# SDA iPhone 前置 TrueDepth 测量测试版

本包为未签名 arm64 IPA，最低 iOS 16.4。安装需要用户侧载签名；不是 App Store / TestFlight 包。

入口：设置 → 声音增强 → 个人 HRTF · 头耳扫描 → 本机适配与档案 → 前置 TrueDepth · 测量验证。

支持：前置 TrueDepth 原始深度、本机 Metal 配准与三维重建、几何筛查、测量模型旋转和放大、左右耳声源面与鼻尖／头顶人工确认。采用“正脸 → 本人左耳 → 本人右耳”的三步引导：每步显示当前部位与进度，完成后有语音、震动提示，并自动进入下一步。保持头部和表情不动，缓慢移动手机（可由他人协助），采集另一耳前沿脸侧回到正面；无需扫描后脑。没有后置相机回退，不上传数据，不依赖电脑执行采集和重建。

预算：根据当前可用内存计算采集点数（最多 150 万点）。预算用满后暂停相机并保留本次测量，三个步骤未完成不能跳过，可取消删除后重新采集；600 条位姿记录不再终止扫描。点数预算是工作空间估计，真实完整扫描的内存峰值仍需验证。

采集判定：先识别单人正脸和有效深度，才允许初始化重建，并过滤采集范围外的深度。已配准画面的本机语义检测按当前步骤计数，扫错耳侧不会推进；相机还必须移到对应的耳侧（相对正面约 34° 以上），从正面看到两个耳朵不能代替左右耳侧采集。正脸至少四次有效观测；每耳至少五次空间稳定的有效观测、跨越至少 1.2 秒且相机位置变化至少 8 毫米，才推进。不要只停留在一张静止耳部画面；在耳旁小幅移动手机，保留脸侧重叠。短暂遮挡可清除当前尚未完成的观测，已完成步骤保留。三个步骤完成且至少有 10000 个实测点和有效位姿后，停止相机并允许继续检查。区域识别不代表耳廓声学精度合格，也不能自动确认耳道入口。

重建诊断：保留小幅度测量法线的导出精度，网格使用二进制 Float32 输出，透传底层求解及文件写入失败原因。稠密候选的读取、模型预览和几何检查采用一致的有界容量，并在读取前核对当前内存预算；测量预览剔除退化面，仅保留有深度和法线支持的三角面，允许未扫描区域的开放边界；声学闭合网格验收仍独立保留。

范围：这是用于真实设备验证的测量包。尚未证明耳部细节的声学精度，也未完成真实头部网格的频率／分辨率收敛及资源验证；不会根据本次扫描生成或启用个人 HRTF。扫描完成、失败或取消后删除临时扫描；应用被终止时遗留的临时扫描在下次启动清理。已有 .phrtf 档案仍可导入。

包内带有重建框架、Metal 资源与许可声明。personal-scan-package-audit.json 记录实际 IPA 的资源审计，不代表已经通过真人采集或听感验收。
EOF
