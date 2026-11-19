# Reproducible lifecycle adaptation of the pinned native iOS reconstructor.
from pathlib import Path
import sys,hashlib,json
root=Path(sys.argv[1])
paths={
 'header':('StandardCyborgFusion/Sources/include/StandardCyborgFusion/SCReconstructionManager.h','a673e231eaa60ddd8568e2435e65d46c00400af6f322d418fd28d80b7251a7d2'),
 'source':('StandardCyborgFusion/Sources/StandardCyborgFusion/Public/SCReconstructionManager.mm','7f9cc2b02d8f061c4cb30316335b71b2edd422696b6a9b9bb753386a70242f60')}
paths.update({
 'fusionHeader':('StandardCyborgFusion/Sources/StandardCyborgFusion/Algorithm/SurfelFusion.hpp','6d29ab53436844da86d57035b47c3f6c183569ad7491a110194e5bc81a34779c'),
 'fusionSource':('StandardCyborgFusion/Sources/StandardCyborgFusion/Algorithm/SurfelFusion.cpp','e43dd7b8ab2ada15059edbafb9ea1cf09e546c6fbceffa52e158c0311501a28e')})
texts={}
for key,(path,digest) in paths.items():
 data=(root/path).read_bytes()
 if hashlib.sha256(data).hexdigest()!=digest:raise ValueError('Pinned reconstructor source changed: '+path)
 texts[key]=data.decode()
def replace(key,old,new):
 if texts[key].count(old)!=1:raise ValueError('Reconstruction patch anchor is not unique: '+old[:70])
 texts[key]=texts[key].replace(old,new)
replace('header','/** The gravity vector of the current scan. */', '''/** SDA: model-thread snapshot; does not borrow the live surfel vector. */
@property (atomic, readonly) NSUInteger sdaSurfelCount;
/** SDA: configure before the first camera frame, bounded by the local mesh reader. */
@property (atomic) NSUInteger sdaMaximumSurfelCount;
/** SDA: capture is paused at the bound; the measured model remains available. */
@property (atomic, readonly) BOOL sdaReachedSurfelLimit;
/** SDA: completion runs on main after the model queue has released its state. */
- (void)sdaResetWithCompletion:(nullable dispatch_block_t)completion NS_SWIFT_NAME(sdaReset(completion:));

/** The gravity vector of the current scan. */''')
replace('source','#import <iostream>','#import <iostream>\n#include <atomic>\n#include <mutex>\n#include <algorithm>\n#include "SdaTrackingRecovery.hpp"\n#include "../Algorithm/SdaHrtfMesh.h"')
replace('source','    GravityEstimator _gravityEstimator;','''    GravityEstimator _gravityEstimator;
    std::mutex _sdaGravityMutex;
    std::atomic<size_t> _sdaSurfelCount;
    std::atomic<size_t> _sdaMaximumSurfelCount;
    SdaTrackingRecovery _sdaTrackingRecovery;''')
replace('source','        _wroteIntrinsicsToFile = NO;','        _wroteIntrinsicsToFile = NO;\n        _sdaSurfelCount.store(0, std::memory_order_relaxed);\n        _sdaMaximumSurfelCount.store(500000, std::memory_order_relaxed);')
replace('source','- (simd_float3)gravity\n{','''- (NSUInteger)sdaMaximumSurfelCount
{
    return _sdaMaximumSurfelCount.load(std::memory_order_acquire);
}
- (void)setSdaMaximumSurfelCount:(NSUInteger)count
{
    _sdaMaximumSurfelCount.store(std::min<size_t>(SDA_HRTF_MAXIMUM_SUPPORT_POINTS, std::max<size_t>(10000, count)), std::memory_order_release);
}
- (BOOL)sdaReachedSurfelLimit
{
    return _sdaSurfelCount.load(std::memory_order_acquire) >= _sdaMaximumSurfelCount.load(std::memory_order_acquire);
}
- (NSUInteger)sdaSurfelCount
{
    return (NSUInteger)_sdaSurfelCount.load(std::memory_order_acquire);
}

- (simd_float3)gravity
{
    std::lock_guard<std::mutex> guard(_sdaGravityMutex);''')
replace('source','- (void)accumulateDeviceMotion:(CMDeviceMotion *)deviceMotion\n{','''- (void)accumulateDeviceMotion:(CMDeviceMotion *)deviceMotion
{
    std::lock_guard<std::mutex> guard(_sdaGravityMutex);''')
replace('source','    CVPixelBufferRetain(depthBuffer);','''    if (_sdaSurfelCount.load(std::memory_order_acquire) >= _sdaMaximumSurfelCount.load(std::memory_order_acquire)) { return; }
    CVPixelBufferRetain(depthBuffer);''')
replace('source','    dispatch_async(_inputQueue, ^{\n        _inputQueue_stopped = NO;','''    dispatch_async(_inputQueue, ^{
        // Calls already enqueued when the model crosses the quota must not
        // reopen the input queue or retain another camera frame indefinitely.
        if (_sdaSurfelCount.load(std::memory_order_acquire) >= _sdaMaximumSurfelCount.load(std::memory_order_acquire)) {
            CVPixelBufferRelease(depthBuffer);
            CVPixelBufferRelease(colorBuffer);
            return;
        }
        _inputQueue_stopped = NO;''')
replace('source','''                                                                                                        _inputQueue_statistics.consecutiveLostTrackingCount);
        
        if (_includesDepthBuffersInMetadata)''','''                                                                                                        _inputQueue_statistics.consecutiveLostTrackingCount);
        // Upstream ends after 8 rejected frames (<0.3s at 30fps). Keep the
        // exact ICP acceptance decision; only give rejected frames 3s to recover.
        const bool terminalTracking = _sdaTrackingRecovery.terminal(pbfMetadata.isMerged,
                                                                   NSProcessInfo.processInfo.systemUptime);
        if (!pbfMetadata.isMerged) {
            metadata.result = terminalTracking ? SCAssimilatedFrameResultFailed : SCAssimilatedFrameResultLostTracking;
        }
        // The insertion site enforces the cap. Quota is not a tracking failure:
        // publish the bounded count so the owner can preserve and process it.
        _sdaSurfelCount.store(_modelQueue_model->getSurfels().size(), std::memory_order_release);
        
        if (_includesDepthBuffersInMetadata)''')
replace('source','    auto metadata = _modelQueue_model->assimilate(','''    _surfelFusionConfig.maximumSurfelCount = _sdaMaximumSurfelCount.load(std::memory_order_acquire);
    auto metadata = _modelQueue_model->assimilate(''')
# The upstream initial-frame configuration must not erase the caller's near clip.
replace('source','    _surfelFusionConfig.minDepth = 0;','    // Preserve the caller-provided near-depth exclusion.')
replace('fusionHeader','#import "SurfelIndexMap.hpp"','#import "SurfelIndexMap.hpp"\n#include "SdaHrtfMesh.h"')
replace('fusionHeader','    float minDepth = 0;','    size_t maximumSurfelCount = SDA_HRTF_MAXIMUM_SUPPORT_POINTS;\n    float minDepth = 0;')
replace('fusionSource','''                _addValuesAsNewSurfel(incomingPositionInModelFrame,''','''                // Strict pre-insertion bound: never allocate a whole extra depth frame.
                // Existing matched surfels still use the original integration path.
                if (surfels.size() >= surfelFusionConfiguration.maximumSurfelCount) continue;
                _addValuesAsNewSurfel(incomingPositionInModelFrame,''')
replace('source','- (void)reset\n{','''- (void)reset
{
    [self sdaResetWithCompletion:nil];
}

- (void)sdaResetWithCompletion:(nullable dispatch_block_t)completion
{''')
replace('source','            _modelQueue_model->reset();','''            _modelQueue_model->reset();
            _sdaSurfelCount.store(0, std::memory_order_release);
            _sdaTrackingRecovery.reset();
            if (completion != nil) { dispatch_async(dispatch_get_main_queue(), completion); }''')
recovery=Path(__file__).resolve().parents[1]/'apps/mobile/modules/sda-core/ios/PersonalHrtfNative/SdaTrackingRecovery.hpp'
recoveryTarget=root/Path(paths['source'][0]).parent/'SdaTrackingRecovery.hpp'
recoveryTarget.write_bytes(recovery.read_bytes())
record={str(recoveryTarget.relative_to(root)):hashlib.sha256(recovery.read_bytes()).hexdigest()}
meshHeader=Path(__file__).resolve().parents[1]/'apps/mobile/modules/sda-core/ios/PersonalHrtfNative/SdaHrtfMesh.h'
for key in ['fusionHeader']:
 target=root/Path(paths[key][0]).parent/'SdaHrtfMesh.h'
 target.write_bytes(meshHeader.read_bytes());record[str(target.relative_to(root))]=hashlib.sha256(meshHeader.read_bytes()).hexdigest()
for key,(path,_)in paths.items():
 data=texts[key].encode();(root/path).write_bytes(data);record[path]=hashlib.sha256(data).hexdigest()
(root.parent/'fusion-lifecycle-patch.json').write_text(json.dumps({'pinnedCommit':'8c3bc698944ec41edaa6f627db15a6573e8e2af2','patches':['atomic model-thread point-count snapshot','memory-configured strict per-insertion point cap; capture limit preserves measured model','serialized gravity estimator access','3s monotonic recovery of rejected frames without relaxing ICP acceptance','model reset completion after memory release'],'sha256':record},indent=2)+'\n')
