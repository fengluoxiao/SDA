# Reproducible lifecycle adaptation of the pinned native iOS reconstructor.
from pathlib import Path
import sys,hashlib,json
root=Path(sys.argv[1])
paths={
 'header':('StandardCyborgFusion/Sources/include/StandardCyborgFusion/SCReconstructionManager.h','a673e231eaa60ddd8568e2435e65d46c00400af6f322d418fd28d80b7251a7d2'),
 'source':('StandardCyborgFusion/Sources/StandardCyborgFusion/Public/SCReconstructionManager.mm','7f9cc2b02d8f061c4cb30316335b71b2edd422696b6a9b9bb753386a70242f60')}
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
/** SDA: completion runs on main after the model queue has released its state. */
- (void)sdaResetWithCompletion:(nullable dispatch_block_t)completion NS_SWIFT_NAME(sdaReset(completion:));

/** The gravity vector of the current scan. */''')
replace('source','#import <iostream>','#import <iostream>\n#include <atomic>\n#include <mutex>\n#include "SdaTrackingRecovery.hpp"')
replace('source','    GravityEstimator _gravityEstimator;','''    GravityEstimator _gravityEstimator;
    std::mutex _sdaGravityMutex;
    std::atomic<size_t> _sdaSurfelCount;
    SdaTrackingRecovery _sdaTrackingRecovery;''')
replace('source','        _wroteIntrinsicsToFile = NO;','        _wroteIntrinsicsToFile = NO;\n        _sdaSurfelCount.store(0, std::memory_order_relaxed);')
replace('source','- (simd_float3)gravity\n{','''- (NSUInteger)sdaSurfelCount
{
    return (NSUInteger)_sdaSurfelCount.load(std::memory_order_acquire);
}

- (simd_float3)gravity
{
    std::lock_guard<std::mutex> guard(_sdaGravityMutex);''')
replace('source','- (void)accumulateDeviceMotion:(CMDeviceMotion *)deviceMotion\n{','''- (void)accumulateDeviceMotion:(CMDeviceMotion *)deviceMotion
{
    std::lock_guard<std::mutex> guard(_sdaGravityMutex);''')
replace('source','    CVPixelBufferRetain(depthBuffer);','''    if (_sdaSurfelCount.load(std::memory_order_acquire) > 500000) { return; }
    CVPixelBufferRetain(depthBuffer);''')
replace('source','    dispatch_async(_inputQueue, ^{\n        _inputQueue_stopped = NO;','''    dispatch_async(_inputQueue, ^{
        // Calls already enqueued when the model crosses the quota must not
        // reopen the input queue or retain another camera frame indefinitely.
        if (_sdaSurfelCount.load(std::memory_order_acquire) > 500000) {
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
        // Only the model queue reads the live vector. Stop on this frame;
        // at most one input depth frame can take the model past the quota.
        _sdaSurfelCount.store(_modelQueue_model->getSurfels().size(), std::memory_order_release);
        if (_sdaSurfelCount.load(std::memory_order_acquire) > 500000) {
            metadata.result = SCAssimilatedFrameResultFailed;
        }
        
        if (_includesDepthBuffersInMetadata)''')
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
for key,(path,_)in paths.items():
 data=texts[key].encode();(root/path).write_bytes(data);record[path]=hashlib.sha256(data).hexdigest()
(root.parent/'fusion-lifecycle-patch.json').write_text(json.dumps({'pinnedCommit':'8c3bc698944ec41edaa6f627db15a6573e8e2af2','patches':['atomic model-thread point-count snapshot','500000-point quota checked on producer/model queues with at most one depth frame overshoot','serialized gravity estimator access','3s monotonic recovery of rejected frames without relaxing ICP acceptance','model reset completion after memory release'],'sha256':record},indent=2)+'\n')
