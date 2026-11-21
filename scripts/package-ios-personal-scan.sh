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

本包使用 FaceFrame（Apache-2.0）固定版本 `122fdc04ae2070991c82bf31d2c3c07b7f158d0b` 的深度配准和左右侧覆盖代码。仅一个前置 TrueDepth 会话，不使用后置相机、录像、云端、GNM 头模或推测的耳廓。

手机保持在正前方不动。先正视，再把鼻尖转向自己的右肩，展示左耳；然后经过正面，把鼻尖转向自己的左肩，展示右耳。正脸平移不算转头。FaceFrame 确认新配准的侧面深度帧后，使用 ARKit 标记的双眼位置核对耳侧，并要求离线模型在同期画面检测到耳廓。不需要扫描后脑。

FaceFrame 在头部坐标系中融合深度。StandardCyborg / PoissonRecon 根据未滤波的绝对深度点及法线提取候选表面。遇到表面缺边时，在内存预算允许的前提下，使用同一份测量点以低一档网格分辨率自动重试一次。不保存相机图像；临时输入输出在本次处理结束、失败或取消时清理，异常退出遗留文件在下次启动清理。两次使用相同的位置、法线、拓扑和自交标准，耳道入口仍需人工确认。

失败面板可分别导出诊断日志和几何文件。日志不含相机图像、坐标或设备标识。若有可读但未通过几何检查的候选网格，可导出标明未验证的 PLY；没有可读网格时提供实测点云（带法线、无三角面）。在成功的模型预览页可导出屏幕上正在检查的实测网格；坐标单位为米。几何文件含个人头耳形状，只在用户点击导出并选择分享目标时交付。

范围：这是用于真实设备验证的测量包。尚未证明耳部细节的声学精度，不会生成或启用个人 HRTF。关闭结果页后不保留扫描，应用中断遗留的共享临时文件下次启动清理。已有 .phrtf 档案仍可导入。

包内带有重建框架、Metal 资源与许可声明。personal-scan-package-audit.json 记录实际 IPA 的资源审计，不代表已经通过真人采集或听感验收。
EOF
