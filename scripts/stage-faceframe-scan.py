"""Stage a pinned subset of the FaceFrame iOS scanner. Build-time network only."""
import argparse
import hashlib
import json
from pathlib import Path
import time
import urllib.request


REVISION = '122fdc04ae2070991c82bf31d2c3c07b7f158d0b'
REPOSITORY = 'https://github.com/YannisDC/FaceFrame'
BLOBS = {
    'FaceFrame/Core/DepthImage.swift': '96930cc1a073d7ee459d4d4fd11511fdac1854b5',
    'FaceFrame/Core/DepthMapping.swift': 'e0c7aac4ad3f54234ff063b867906000120d7e67',
    'FaceFrame/Core/FusionEngine.swift': '3a80854806f8a13f404e96474535ae7c55a14a7c',
    'FaceFrame/Core/FusionFrame.swift': '7405231f956c0b4ead571e4a9b9a0e7e236b498f',
    'FaceFrame/Core/PoseCorrection.swift': '7a1bd37f9a0d1226555bfbb2d622fa0b1209d731',
    'FaceFrame/Core/RigidMath.swift': 'aeb6c0673fc92374282a722ffc3a8f8783b8419a',
    'FaceFrame/Core/SDFAligner.swift': '21a5df54890bfcd3fc6cae576100824cd9e1e460',
    'FaceFrame/Core/TSDFVolume.swift': 'ef4b4d6a095fec5490372ed4dd46fc3cd69562b1',
    'FaceFrame/Scan/DepthConversion.swift': '72ea92e4499c69c5d5d8832b664442fc7b292ce1',
    'FaceFrame/Scan/PhotoObservations.swift': 'affec2978a329b76d734a4cc5dc81b2b8c73df58',
    'LICENSE': '0843f3b52f1bbbe9ee596e5f40a91b3ead38dab7',
}
MODIFICATIONS = {
    'FusionFrame.swift': 'Omit the optional disk recorder and replay format; keep the upstream in-memory frame type.',
    'FusionEngine.swift': 'Expose accepted-frame coverage counts and measured depth for independent support verification. Apply the caller\'s block budget. Upstream view selection, pose recovery, and registration are unchanged.',
    'TSDFVolume.swift': 'Enforce a maximum number of allocated blocks before insertion; skip updates for unallocated blocks at the limit.',
    'PhotoObservations.swift': 'Use only the upstream upright quarter-turn calculation for the ear detector, without loading the GNM photo fitting models.',
}


def replace_once(text, old, new):
    if text.count(old) != 1:
        raise ValueError('FaceFrame integration anchor changed: ' + old[:90])
    return text.replace(old, new)


def adapt(name, data):
    if name not in MODIFICATIONS:
        return data
    source = data.decode('utf-8')
    if name == 'FusionFrame.swift':
        source, separator, _ = source.partition('// MARK: - Binary capture format')
        if not separator:
            raise ValueError('Unrecognized FaceFrame capture layout')
    elif name == 'FusionEngine.swift':
        source = replace_once(source, '    private var counts =', '    var maximumBlocks = Int.max\n    var onMeasuredFrame: ((DepthImage, [UInt8], simd_float4x4) -> Void)?\n    private(set) var counts =')
        source = replace_once(source, '        updateCoverage(view)', '        updateCoverage(view)\n        onMeasuredFrame?(img, quality, pose)')
        source = replace_once(source, '        counts = (0, 0, 0, 0, 0)', '        volume?.maximumBlocks = maximumBlocks\n        counts = (0, 0, 0, 0, 0)')
        source = replace_once(source, '        return status(.arkit, pose: arPose)\n    }\n\n    /// Constant-velocity', '        volume?.maximumBlocks = maximumBlocks\n        return status(.arkit, pose: arPose)\n    }\n\n    /// Constant-velocity')
    elif name == 'PhotoObservations.swift':
        start = source.index('        let camFromHead = frame.arPose.inverse')
        end = source.index('        let K = frame.intrinsics', start)
        turn = source[start:end].replace('frame.arPose.inverse','headFromCamera.inverse')
        source = 'import simd\n\nenum FaceFrameUpright {\n    static func quarterTurns(_ headFromCamera: simd_float4x4) -> Int {\n' + turn + '        return k\n    }\n}\n'
    else:
        source = replace_once(source, '    var allocatedBlocks: Int', '    var maximumBlocks = Int.max\n    private(set) var allocationLimitReached = false\n    var allocatedBlocks: Int')
        source = replace_once(source, '        blockLookup[slot] = Int32(blockCoords.count)', '        guard blockCoords.count < maximumBlocks else { allocationLimitReached = true; return }\n        blockLookup[slot] = Int32(blockCoords.count)')
        source = replace_once(source, '                    let id = blockLookup[blockSlot(v &>> TSDFVolume.blockShift)]\n                    if blockStamp.count', '                    let id = blockLookup[blockSlot(v &>> TSDFVolume.blockShift)]\n                    guard id >= 0 else { break }\n                    if blockStamp.count')
    return ('// Modified by SDA; see Licenses/FaceFrame/provenance.json.\n' + source).encode()


def download(filename):
    request = urllib.request.Request(f'https://raw.githubusercontent.com/YannisDC/FaceFrame/{REVISION}/{filename}',headers={'User-Agent':'SDA-FaceFrame-builder'})
    for attempt in range(3):
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                return response.read(256 * 1024)
        except OSError:
            if attempt == 2:
                raise
            time.sleep(1 + attempt)


def stage(ios, cache):
    destination = ios / 'PersonalHrtfFusion' / 'FaceFrame'
    notices = ios / 'Licenses' / 'FaceFrame'
    destination.mkdir(parents=True, exist_ok=True)
    notices.mkdir(parents=True, exist_ok=True)
    hashes = {}
    for filename, blob in BLOBS.items():
        cached = cache / blob
        data = cached.read_bytes() if cached.is_file() else download(filename)
        if hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest() != blob:
            raise ValueError('Unverified FaceFrame source: ' + filename)
        cache.mkdir(parents=True, exist_ok=True)
        cached.write_bytes(data)
        name = Path(filename).name
        installed = adapt(name, data)
        ((notices if name == 'LICENSE' else destination) / name).write_bytes(installed)
        hashes[filename] = {'upstreamBlob':blob,'upstreamSHA256':hashlib.sha256(data).hexdigest(),'installedSHA256':hashlib.sha256(installed).hexdigest()}
    (notices/'provenance.json').write_text(json.dumps({'repository':REPOSITORY,'commit':REVISION,'license':'Apache-2.0','modifications':MODIFICATIONS,'files':hashes},indent=2)+'\n',encoding='utf-8')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('ios',type=Path)
    parser.add_argument('cache',type=Path)
    arguments = parser.parse_args()
    stage(arguments.ios, arguments.cache)
