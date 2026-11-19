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
paths.update({
 'meshHeader':('StandardCyborgFusion/Sources/include/StandardCyborgFusion/SCMeshingOperation.h','6bd85770a10690a591c4e4e67a16d5c36a5764a607d115ce3405811e3256abac'),
 'meshSource':('StandardCyborgFusion/Sources/StandardCyborgFusion/Public/SCMeshingOperation.mm','e64ac9b0ee5ce62cec5e6f7fbfb71cfc14b8883ccc53b0a5eb85aaaf0a9fc740'),
 'poissonHeader':('CppDependencies/PoissonRecon/Sources/include/PoissonRecon/MeshingOperation.h','08b13e1113358ced36c341a8132c940dd6f997d07c4a20d673ada72424ab18b2'),
 'poissonSource':('CppDependencies/PoissonRecon/Sources/src/MeshingOperation.mm','e1c8920d10b74a7395b316b1162c99598b1fc94c830de6d32ad79b7c6937873e'),
})
paths.update({'pointCloudPLY':('StandardCyborgFusion/Sources/StandardCyborgFusion/IO/PointCloudIO_PLY.mm','b6d3e5febf75beb4eef50315986af30a05d8102d1ebae62461095bb19f7e9b64')})
paths.update({'poissonAdapter':('CppDependencies/PoissonRecon/Sources/include/PoissonRecon/PoissonReconExecute.hpp','4e4ef9bf0dff584cfeb3a1b51ce11ab69e1d4cd3a9311d59293615f7219eb441')})
paths.update({'poissonPreprocessor':('CppDependencies/PoissonRecon/Sources/include/PoissonRecon/PreProcessor.h','57a763b67cc432657261018e8f72b46104a6973b82e171ae64e85e1cd286ce33')})
texts={}
for key,(path,digest) in paths.items():
 data=(root/path).read_bytes()
 if hashlib.sha256(data).hexdigest()!=digest:raise ValueError('Pinned reconstructor source changed: '+path)
 texts[key]=data.decode()
def replace(key,old,new):
 if texts[key].count(old)!=1:raise ValueError('Reconstruction patch anchor is not unique: '+old[:70])
 texts[key]=texts[key].replace(old,new)
# Use upstream's checked arithmetic and atomic publication paths in every Poisson translation unit.
replace('poissonPreprocessor','//#define SANITIZED_PR','#define SANITIZED_PR')
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



# Inference is advisory and sampled. Do not attach raw buffers to every queued
# main-thread callback: one outstanding metadata pair, at most twice a second.
replace('source','    std::atomic<size_t> _sdaSurfelCount;','    std::atomic<size_t> _sdaSurfelCount;\n    std::atomic<bool> _sdaCoverageMetadataPending;\n    double _sdaLastCoverageMetadataTime;')
replace('source','        _sdaSurfelCount.store(0, std::memory_order_relaxed);','        _sdaSurfelCount.store(0, std::memory_order_relaxed);\n        _sdaCoverageMetadataPending.store(false, std::memory_order_relaxed);\n        _sdaLastCoverageMetadataTime = 0;')
replace('source','        if (_includesDepthBuffersInMetadata) {','''        const double coverageTime = NSProcessInfo.processInfo.systemUptime;
        const bool includeCoverage = (_includesDepthBuffersInMetadata || _includesColorBuffersInMetadata)
            && coverageTime - _sdaLastCoverageMetadataTime >= 0.5
            && !_sdaCoverageMetadataPending.exchange(true, std::memory_order_acq_rel);
        if (includeCoverage) _sdaLastCoverageMetadataTime = coverageTime;
        if (_includesDepthBuffersInMetadata && includeCoverage) {''')
replace('source','        if (_includesColorBuffersInMetadata) {','        if (_includesColorBuffersInMetadata && includeCoverage) {')
replace('source','''            if (metadata.colorBuffer != NULL) {
                CVPixelBufferRelease(metadata.colorBuffer);
            }''','''            if (metadata.colorBuffer != NULL) {
                CVPixelBufferRelease(metadata.colorBuffer);
            }
            if (includeCoverage) _sdaCoverageMetadataPending.store(false, std::memory_order_release);''')

# Surfel normals encode a small metric radius. Four decimal places can erase
# every component; preserve Float32 precision and reject truncated numeric rows.
replace('pointCloudPLY','#define PLY_LINE_LENGTH 128','#define PLY_LINE_LENGTH 512')
replace('pointCloudPLY','else if (sscanf(line, "%f %f %f %f %f %f %f %f %f %f", &x, &y, &z, &nx, &ny, &nz, &r, &g, &b, &surfelRadius)) {','else if (sscanf(line, "%f %f %f %f %f %f %f %f %f %f", &x, &y, &z, &nx, &ny, &nz, &r, &g, &b, &surfelRadius) == 10) {')
replace('pointCloudPLY','            float scaleFactor = (normalizeNormals ? 1.0 : surfelRadius) / normalMagnitude;', '''            if (!std::isfinite(normalMagnitude) || normalMagnitude <= 0 || !std::isfinite(surfelRadius) || surfelRadius <= 0 || !std::isfinite(x) || !std::isfinite(y) || !std::isfinite(z)) {
                fclose(fileHandle); return false;
            }
            float scaleFactor = (normalizeNormals ? 1.0 : surfelRadius) / normalMagnitude;''')
replace('pointCloudPLY','%.4f %.4f %.4f %.4f %.4f %.4f %d %d %d %f','%.9g %.9g %.9g %.9g %.9g %.9g %d %d %d %.9g')
replace('pointCloudPLY','''    fclose(file);
    
    return true;
}''','''    const bool complete = ferror(file) == 0;
    const bool closed = fclose(file) == 0;
    return complete && closed;
}''')

replace('poissonAdapter','#include "PreProcessor.h"','#include "PreProcessor.h"\n#include "SdaPoissonMeshCleanup.hpp"')
replace('poissonAdapter','    // Write the output PLY file','    sdaCleanPoissonMesh(vertexCollector.vertices, faceCollector.faces);\n\n    // Write the output PLY file')
replace('poissonAdapter','PLY::DefaultFileType(), comments','PLY_BINARY_LE, comments')

# Preserve operation state/cancellation and report actual solver/publication failures.
for key in ['meshHeader','poissonHeader']:
 replace(key,'@interface '+('SCMeshingOperation' if key=='meshHeader' else 'MeshingOperation')+' : NSOperation',
              '@interface '+('SCMeshingOperation' if key=='meshHeader' else 'MeshingOperation')+' : NSOperation\n@property (atomic, readonly, nullable) NSError *sdaError;'.replace('\\n','\n'))
replace('meshSource','- (void)main\n{','- (NSError *)sdaError { return _operation.sdaError; }\n\n- (void)main\n{')
replace('meshSource','    [_operation cancel];','    [super cancel];\n    [_operation cancel];')
replace('poissonSource','#import <PoissonRecon/ExecuteEntryFunctions.hpp>','#import <PoissonRecon/ExecuteEntryFunctions.hpp>\n#include <exception>')
replace('poissonSource','@implementation MeshingOperation {','@interface MeshingOperation ()\n@property (atomic, readwrite, nullable) NSError *sdaError;\n@end\n\n@implementation MeshingOperation {')
replace('poissonSource','    if (![weakSelf isCancelled]) {\n        PoissonReconExecute', '    try {\n    if (![weakSelf isCancelled]) {\n        PoissonReconExecute')
replace('poissonSource','            [[NSFileManager defaultManager] moveItemAtPath:tempPoissonOutputPathString toPath:_outputFilePath error:NULL];', '''            NSError *error = nil;
            if (![[NSFileManager defaultManager] moveItemAtPath:tempPoissonOutputPathString toPath:_outputFilePath error:&error]) {
                self.sdaError = error;
            }''')
replace('poissonSource','    [[NSFileManager defaultManager] removeItemAtPath:tempPoissonOutputPathString error:NULL];', '''    } catch (const std::exception& error) {
        self.sdaError = [NSError errorWithDomain:@"SDA.Poisson" code:1 userInfo:@{NSLocalizedDescriptionKey:[NSString stringWithUTF8String:error.what()] ?: @"Poisson exception"}];
    } catch (...) {
        self.sdaError = [NSError errorWithDomain:@"SDA.Poisson" code:2 userInfo:@{NSLocalizedDescriptionKey:@"Poisson reconstruction failed"}];
    }
    if (!self.isCancelled && self.sdaError == nil) {
        NSNumber *size = nil;
        [[NSURL fileURLWithPath:_outputFilePath] getResourceValue:&size forKey:NSURLFileSizeKey error:NULL];
        if (size == nil || size.unsignedLongLongValue < 128) {
            self.sdaError = [NSError errorWithDomain:@"SDA.Poisson" code:3 userInfo:@{NSLocalizedDescriptionKey:@"Poisson produced no surface file"}];
        }
    }
    [[NSFileManager defaultManager] removeItemAtPath:tempPoissonOutputPathString error:NULL];''')

recovery=Path(__file__).resolve().parents[1]/'apps/mobile/modules/sda-core/ios/PersonalHrtfNative/SdaTrackingRecovery.hpp'
recoveryTarget=root/Path(paths['source'][0]).parent/'SdaTrackingRecovery.hpp'
recoveryTarget.write_bytes(recovery.read_bytes())
record={str(recoveryTarget.relative_to(root)):hashlib.sha256(recovery.read_bytes()).hexdigest()}
cleanup=Path(__file__).resolve().parents[1]/'apps/mobile/modules/sda-core/ios/PersonalHrtfNative/SdaPoissonMeshCleanup.hpp'
cleanupTarget=root/Path(paths['poissonAdapter'][0]).parent/cleanup.name
cleanupTarget.write_bytes(cleanup.read_bytes());record[str(cleanupTarget.relative_to(root))]=hashlib.sha256(cleanup.read_bytes()).hexdigest()
meshHeader=Path(__file__).resolve().parents[1]/'apps/mobile/modules/sda-core/ios/PersonalHrtfNative/SdaHrtfMesh.h'
for key in ['fusionHeader']:
 target=root/Path(paths[key][0]).parent/'SdaHrtfMesh.h'
 target.write_bytes(meshHeader.read_bytes());record[str(target.relative_to(root))]=hashlib.sha256(meshHeader.read_bytes()).hexdigest()
for key,(path,_)in paths.items():
 data=texts[key].encode();(root/path).write_bytes(data);record[path]=hashlib.sha256(data).hexdigest()
(root.parent/'fusion-lifecycle-patch.json').write_text(json.dumps({'pinnedCommit':'8c3bc698944ec41edaa6f627db15a6573e8e2af2','patches':['atomic model-thread point-count snapshot','memory-configured strict per-insertion point cap; capture limit preserves measured model','serialized gravity estimator access','3s monotonic recovery of rejected frames without relaxing ICP acceptance','model reset completion after memory release'],'sha256':record},indent=2)+'\n')
