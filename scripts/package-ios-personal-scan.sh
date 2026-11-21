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
provenance.update(notices=notices,measurementOnly=False,experimentalGeneration=True,personalGenerationReady=False)
(dest/'provenance.json').write_text(json.dumps(provenance,indent=2))
Path('ios-artifacts/personal-scan-framework.json').write_text(json.dumps(provenance,indent=2))
PY
cat > ios-artifacts/PERSONAL-HRTF-TEST-README-zh.md <<'EOF'
# SDA 前置 TrueDepth 个人声学实验版

本包为未签名 arm64 IPA，最低 iOS 16.4。安装需要用户侧载签名；不是 App Store / TestFlight 包。

入口：设置 → 声音增强 → 个人 HRTF · 头耳扫描 → 本机适配与档案。选择「前置 TrueDepth · 测量验证」只进行测量；选择「扫描并生成实验档案」才进行数值求解。

本包使用 FaceFrame（Apache-2.0）固定版本 `122fdc04ae2070991c82bf31d2c3c07b7f158d0b` 的深度配准和左右侧覆盖代码。仅一个前置 TrueDepth 会话，不使用后置相机、录像、云端、GNM 头模或推测的耳廓。

手机保持在正前方不动。先正视，再把鼻尖转向自己的右肩，展示左耳；然后经过正面，把鼻尖转向自己的左肩，展示右耳。正脸平移不算转头。FaceFrame 确认新配准的侧面深度帧后，使用 ARKit 标记的双眼位置核对耳侧，并要求离线模型在同期画面检测到耳廓。不需要扫描后脑。

FaceFrame 在头部坐标系中融合深度。StandardCyborg / PoissonRecon 根据未滤波的绝对深度点及法线提取候选表面。遇到表面缺边时，在内存预算允许的前提下，使用同一份测量点以低一档网格分辨率自动重试一次。不保存相机图像；临时输入输出在本次处理结束、失败或取消时清理，异常退出遗留文件在下次启动清理。两次使用相同的位置、法线、拓扑和自交标准，耳道入口仍需人工确认。

几何检查或声学输入准备失败时可导出诊断日志，不含相机图像、坐标或设备标识。网格与独立实测点云（带法线）可分开选择：几何检查失败的候选网格标为未验证，声学输入失败时导出的是原始实测网格，不含后方计算边界或修补面。几何文件含个人头耳形状，只在点击导出并选择分享目标时交付。

实验生成使用实测双耳、正脸及明确标记的后方刚性计算边界。局部网格缺面只有在内部有位置和法线都吻合的独立深度时才可重新三角化；不推测缺测耳形，也不改已确认的耳道声源面。不用后置摄像头，无需扫描后脑。前台计算可能很久；只有完成双耳全频点、档案校验且本人点选启用后才替换档案。计算结束删除工作文件，异常退出下次启动清理。尚未验证真人声学精度或实机耗时。

包内带有重建框架、Metal 资源与许可声明。personal-scan-package-audit.json 记录实际 IPA 的资源审计，不代表已经通过真人采集或听感验收。
EOF
