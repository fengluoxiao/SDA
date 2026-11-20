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

本包使用 FaceFrame（Apache-2.0）固定版本 `122fdc04ae2070991c82bf31d2c3c07b7f158d0b` 的深度配准、左右侧覆盖和表面提取代码。仅一个前置 TrueDepth 会话，不使用后置相机、录像、云端、GNM 头模或推测的耳廓。

手机保持在面前。先正视手机，再向本人右边转头，让左耳朝向相机；然后经过正面向左转头，让右耳朝向相机。正脸即使露出耳朵也不完成耳侧步骤。只有 FaceFrame 在新配准的深度帧中记录到该侧，同时离线耳部模型在对应画面检测到耳廓，才记录该耳。无需等上一只耳朵消失、点击确认、扫下巴或后脑。

FaceFrame 在头部坐标系中融合深度，提取开放表面；模型体积、表面数量和单独保留的深度测量点均有上限。只用未滤波的绝对深度作为独立测量支持，只展示通过位置及法线支持检查的三角面。耳道入口仍需人工确认。相机帧和本次网格不写入磁盘。

范围：这是用于真实设备验证的测量包。尚未证明耳部细节的声学精度，也未完成真实头部网格的频率／分辨率收敛及资源验证；不会根据本次扫描生成或启用个人 HRTF。扫描完成、失败或取消后删除临时扫描；应用被终止时遗留的临时扫描在下次启动清理。已有 .phrtf 档案仍可导入。

包内带有重建框架、Metal 资源与许可声明。personal-scan-package-audit.json 记录实际 IPA 的资源审计，不代表已经通过真人采集或听感验收。
EOF
