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
paths.update({'poissonPLY':('CppDependencies/PoissonRecon/Sources/src/PlyFile.cpp','70b36492100d3aec7fd9fdd3d1a510ce00fb698304a60dc371a6b42db8972a80')})
paths.update({'meshGeometry':('StandardCyborgFusion/Sources/StandardCyborgFusion/Public/SCMesh+Geometry.mm','1c6919d110f23858e2e65161ade73b3afabbb7f51eeea119d88227281247cd93')})
paths.update({'meshFileIO':('StandardCyborgFusion/Sources/StandardCyborgFusion/Public/SCMesh+FileIO.mm','549c91181745f71ea9cf495c504d0fda673bc3456c9d36b42934d539309833c7')})
paths.update({'meshFileHeader':('StandardCyborgFusion/Sources/include/StandardCyborgFusion/SCMesh+FileIO.h','573591298053a8f72d2f9090a5ce80dc46e83300a3e6c067855ef0c6664fe0d3')})
paths.update({'geometryPLY':('scsdk/Sources/standard_cyborg/src/io/ply/GeometryFileIO_PLY.cpp','23a884d3f2fbd86e212e28a4182568ccc772d5e824cb6ed92e79052f38da33c4')})
paths.update({'happly':('CppDependencies/happly/include/happly.h','f6bb34ad0102a3f12ef3389b4221af4020c5aed0938bf88b56ec9b00e13c4c35')})
paths.update({'pbfSource':('StandardCyborgFusion/Sources/StandardCyborgFusion/Algorithm/PBFModel.cpp','0a0f89c8e12f822f1ed8588a344a8c2b328426932f6f394ae17f7551f2068bd2')})
paths['pbfConfig']=('StandardCyborgFusion/Sources/StandardCyborgFusion/DataStructures/PBFConfiguration.hpp','52a3ccfa2cee8c92d44630c82be8c0987309b86eec51344f407f9cb88384f3ed')
paths['icpSource']=('StandardCyborgFusion/Sources/StandardCyborgFusion/Algorithm/ICP.cpp','4b13cabef52cc60d7a68a8ecbad72fb993823b4079065ceffdfcc6ba57d60743')
paths['icpHeader']=('StandardCyborgFusion/Sources/include/StandardCyborgFusion/ICP.hpp','de7d7ee8c37dd6e060b5fcb8936e375a377841d493d3b664a8e0ad6ea34a0cb2')
for key,name,digest in [
 ('depthDataHeader','MetalDepthProcessorData.hpp','019aa4d2eeb6dcbe2c43dba9bd7ae45588b32747269ca986355578001e8c8b8d'),
 ('depthData','MetalDepthProcessorData.mm','b4820bd3a97146f18490610755476e918133033e0c2018d9d94fb46aaa2b7b77'),
 ('depthProcessor','MetalDepthProcessor.mm','49a7b25a6451b5095cfe9117c41635953aa99d4f79c185d9d4116f6b9ba19d4f'),
 ('computeEngine','MetalComputeEngine.mm','8d790e7a4bf9eb9e8e28da16261ffadf6d73743f97a4538c149140548a309ea9'),
 ('indexMap','MetalSurfelIndexMap.mm','c04576bf3cbb471c0857249fc0243138e9969a253a074070e7f72711a90027be')]:
 paths[key]=('StandardCyborgFusion/Sources/StandardCyborgFusion/MetalDepthProcessor/'+name,digest)
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
/** SDA: sustained loss asks the user to return to the last view; does not discard the model. */
@property (atomic, readonly) BOOL sdaNeedsTrackingRecovery;
@property (atomic, readonly) BOOL sdaIsAligning;
/** SDA: completion runs on main after the model queue has released its state. */
- (void)sdaResetWithCompletion:(nullable dispatch_block_t)completion NS_SWIFT_NAME(sdaReset(completion:));

/** The gravity vector of the current scan. */''')
replace('source','#import <iostream>','#import <iostream>\n#include <atomic>\n#include <mutex>\n#include <algorithm>\n#include "SdaTrackingRecovery.hpp"\n#include "../Algorithm/SdaHrtfMesh.h"')
replace('source','    GravityEstimator _gravityEstimator;','''    GravityEstimator _gravityEstimator;
    std::mutex _sdaGravityMutex;
    std::atomic<size_t> _sdaSurfelCount;
    std::atomic<size_t> _sdaMaximumSurfelCount;
    SdaTrackingRecovery _sdaTrackingRecovery;
    std::atomic<bool> _sdaIsAligning;
    std::atomic<bool> _sdaNeedsTrackingRecovery;''')
replace('source','        _wroteIntrinsicsToFile = NO;','        _wroteIntrinsicsToFile = NO;\n        _sdaSurfelCount.store(0, std::memory_order_relaxed);\n        _sdaNeedsTrackingRecovery.store(false, std::memory_order_relaxed);\n        _sdaIsAligning.store(false, std::memory_order_relaxed);\n        _sdaMaximumSurfelCount.store(500000, std::memory_order_relaxed);')
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
- (BOOL)sdaIsAligning
{
    return _sdaIsAligning.load(std::memory_order_acquire);
}
- (BOOL)sdaNeedsTrackingRecovery
{
    return _sdaNeedsTrackingRecovery.load(std::memory_order_acquire);
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
        // exact ICP acceptance decision; never end a scan merely for losing tracking.
        const bool terminalTracking = _sdaTrackingRecovery.terminal(pbfMetadata.isMerged,
                                                                   NSProcessInfo.processInfo.systemUptime);
        _sdaNeedsTrackingRecovery.store(_sdaTrackingRecovery.needsReturnToView(NSProcessInfo.processInfo.systemUptime), std::memory_order_release);
        _sdaIsAligning.store(_sdaTrackingRecovery.needsAlignment(NSProcessInfo.processInfo.systemUptime), std::memory_order_release);
        if (!pbfMetadata.isMerged) {
            metadata.result = terminalTracking ? SCAssimilatedFrameResultFailed : SCAssimilatedFrameResultLostTracking;
        }
        // The insertion site enforces the cap. Quota is not a tracking failure:
        // publish the bounded count so the owner can preserve and process it.
        _sdaSurfelCount.store(_modelQueue_model->getSurfels().size(), std::memory_order_release);
        
        if (_includesDepthBuffersInMetadata)''')
replace('source','    auto metadata = _modelQueue_model->assimilate(','''    _surfelFusionConfig.maximumSurfelCount = _sdaMaximumSurfelCount.load(std::memory_order_acquire);
    auto metadata = _modelQueue_model->assimilate(''')
# LostTracking is recoverable: keep the newest pending input rather than
# falling through to terminal shutdown and deleting it on every rejected frame.
replace('source','                    // Intentional fall-through','                    break; // Recoverable loss: keep processing the latest input.')
# Apply the same bounded rotation-angle calculation exercised by the native tests.
replace('pbfSource','#import <cmath>','#import <cmath>\n#include "../Public/SdaTrackingRecovery.hpp"\n#include "../Public/SdaTrackingPrediction.hpp"')
for axis in ['X','Y','Z']:
 replace('pbfSource',f'acosf(current{axis}.dot(previous{axis}))',f'SdaTrackingRecovery::axisAngle(current{axis}.dot(previous{axis}), current{axis}.norm() * previous{axis}.norm())')
replace('pbfSource','currentFrameMeta->timestamp == previousFrameMeta->timestamp','currentFrameMeta->timestamp <= previousFrameMeta->timestamp')
# The upstream ICP reports success for an empty cloud. An out-of-range/blank
# frame must not be marked aligned or become the next tracking reference.
replace('pbfSource','    ICPResult icpResult = ICP::run(icpConfig,','''    if (downsampledSourceCloud.getPositions().size() < 32 || _ICPTargetCloud->getPositions().size() < 32) {
        ICPResult rejected;
        rejected.succeeded = false;
        return rejected;
    }
    ICPResult icpResult = ICP::run(icpConfig,''')


# Internal registration strategies permit controlled same-engine comparisons.
replace('pbfConfig','struct PBFConfiguration {','struct PBFConfiguration {\n    bool sdaMotionPrediction = true;\n    bool sdaDistributedSamples = true;')
# Point-to-point RMS can oscillate tangentially despite stable point-to-plane
# geometry. Stop only after three tiny rigid updates with sufficient close support.
replace('icpHeader','struct ICPConfiguration {','struct ICPConfiguration {\n    bool sdaGeometricConvergence = true;')
replace('icpSource','#include <dispatch/dispatch.h>','#include <dispatch/dispatch.h>\n#include "../Public/SdaTrackingConvergence.hpp"')
replace('icpSource','    float computeRMSCorrespondenceError()','    float pointToPlaneRMS(size_t& inliers) const {\n        inliers=0;\n        double squared=0;\n        for(size_t i=0;i<sourceVertices.size();i++) {\n            if (!((*weights)(i)>0)) continue;\n            const auto n=standard_cyborg::toVector3f(targetNormals[i]);\n            const auto d=standard_cyborg::toVector3f(sourceVertices[i])-standard_cyborg::toVector3f(targetVertices[i]);\n            const float length=n.norm();\n            if(!n.allFinite() || !d.allFinite() || !(length>0)) return NAN;\n            const double error=d.dot(n)/length;\n            squared+=error*error; ++inliers;\n        }\n        return inliers ? float(std::sqrt(squared/inliers)) : NAN;\n    }\n    float computeRMSCorrespondenceError()')
replace('icpSource','    float previousError = 1e10;','    SdaTrackingConvergence geometricConvergence;\n    float previousError = 1e10;')
replace('icpSource','        relativeError = fabsf(rmsError - previousError) / rmsError;','        size_t inliers=0;\n        const float planeRms=correspondence.pointToPlaneRMS(inliers);\n        const Eigen::Matrix3f r=sourceTransformAdjustment.block<3,3>(0,0);\n        const Eigen::Vector3f skew(r(2,1)-r(1,2),r(0,2)-r(2,0),r(1,0)-r(0,1));\n        const float angle=std::atan2(skew.norm()*0.5f,(r.trace()-1)*0.5f);\n        const bool stable=config.sdaGeometricConvergence && geometricConvergence.observe(\n            sourceTransformAdjustment.block<3,1>(0,3).norm(),angle,planeRms,inliers,vertexCount);\n        relativeError = stable ? 0 : fabsf(rmsError - previousError) / rmsError;')

# A short pose prediction is an ICP initial guess only. Retry without it if
# the original convergence or velocity checks reject it; never fuse a guess.
replace('pbfSource','        ICPResult icpResult = _runICP(frame, surfelFusionConfiguration, icpConfig, pbfConfig);\n\n        Matrix4f extrinsicMatrixTmp = toMatrix4f(icpResult.sourceTransform) * _extrinsicMatrix;','        const Matrix4f acceptedPose = _extrinsicMatrix;\n        Matrix4f prediction = acceptedPose;\n        const auto older = _nthMostRecentValidFrameMetadata(1);\n        const bool predict = pbfConfig.sdaMotionPrediction && previousFrameMeta && older &&\n            sdaPredictTrackingPose(older->viewMatrix,previousFrameMeta->viewMatrix,\n                                   older->timestamp,previousFrameMeta->timestamp,currentTime,prediction);\n        Matrix4f extrinsicMatrixTmp = acceptedPose;\n        ICPResult icpResult;\n        for (int attempt=0; attempt<(predict ? 2 : 1); ++attempt) {\n        _extrinsicMatrix = predict && attempt==0 ? prediction : acceptedPose;\n        icpResult = _runICP(frame, surfelFusionConfiguration, icpConfig, pbfConfig);\n        extrinsicMatrixTmp = toMatrix4f(icpResult.sourceTransform) * _extrinsicMatrix;')
replace('pbfSource','        if (frameMeta.icpUnusedIterationFraction > 0) {\n            _extrinsicMatrix = extrinsicMatrixTmp;','        if (frameMeta.icpUnusedIterationFraction > 0) break;\n        }\n        _extrinsicMatrix = acceptedPose;\n        if (frameMeta.icpUnusedIterationFraction > 0) {\n            _extrinsicMatrix = extrinsicMatrixTmp;')

# The original raster walk stops at maxCount, often before reaching the lower
# part of the ear. Preserve its weighting and sample count, but distribute the
# visited pixels over the entire image before applying the stochastic filters.
replace('pbfSource','        for (off_t i = 0; i < pointCount && filteredCount < maxCount; ++i) {','''        size_t stride = std::max<size_t>(1, size_t(pointCount * 0.6180339887498948));
        auto gcd = [](size_t a, size_t b) { while (b) { size_t r=a%b; a=b; b=r; } return a; };
        while (pointCount > 1 && gcd(stride,pointCount) != 1) ++stride;
        size_t sampleIndex = 0;
        for (size_t visited = 0; visited < pointCount && filteredCount < maxCount; ++visited) {
            const size_t i = pbfConfig.sdaDistributedSamples ? sampleIndex : visited;
            sampleIndex = (sampleIndex + stride) % pointCount;''')

# Side/grazing depth may contain NaNs: comparisons with NaN are false,
# so upstream's range and stochastic weight checks alone admit invalid points.
# Use only finite, positive-weight samples meeting the existing fusion confidence.
replace('pbfSource','            float depth = frame.rawFrame.depths[i];','''            float depth = frame.rawFrame.depths[i];
            const auto position = standard_cyborg::toVector3f(frame.positions[i]);
            const auto normal = standard_cyborg::toVector3f(frame.normals[i]);
            if (!std::isfinite(depth) || !std::isfinite(frame.weights[i]) || frame.weights[i] <= 0 ||
                !(frame.inputConfidences[i] >= surfelFusionConfiguration.inputConfidenceThreshold) ||
                !position.allFinite() || !normal.allFinite() || normal.squaredNorm() <= 0) continue;''')

# The same invalid samples must not reach the retained measured surface after
# a valid subset has aligned. NaN also passes ordinary range/angle comparisons.
replace('fusionSource','#import "PBFDefinitions.h"','#import "PBFDefinitions.h"\n#include <cmath>')
replace('fusionSource','if (depth <= surfelFusionConfiguration.minDepth || depth > surfelFusionConfiguration.maxDepth)',
                       'if (!std::isfinite(depth) || depth <= surfelFusionConfiguration.minDepth || depth > surfelFusionConfiguration.maxDepth)')
replace('fusionSource','if (inputConfidence < surfelFusionConfiguration.inputConfidenceThreshold)',
                       'if (!(inputConfidence >= surfelFusionConfiguration.inputConfidenceThreshold))')
replace('fusionSource','            const float incomingSurfelSize = frame.surfelSizes[index];','''            const float incomingSurfelSize = frame.surfelSizes[index];
            if (!incomingPosition.allFinite() || !incomingNormal.allFinite() ||
                incomingPosition.squaredNorm() <= 0 || incomingNormal.squaredNorm() <= 0 ||
                !std::isfinite(incomingSurfelSize) || incomingSurfelSize <= 0 ||
                !std::isfinite(frame.weights[index]) || frame.weights[index] <= 0) continue;''')

# Do not seed a scan from a tiny/invalid fragment that cannot supply a later
# ICP target. A failed GPU frame has negative weights and cannot initialize it.
replace('pbfSource','    if (_surfels.size() > 0) {','''    if (_surfels.empty()) {
        if (!std::isfinite(pbfConfig.icpDownsampleFraction) || pbfConfig.icpDownsampleFraction <= 0) return frameMeta;
        const double seedEstimate = std::ceil(32.0 / std::min(1.0f, pbfConfig.icpDownsampleFraction));
        if (seedEstimate > frame.positions.size()) return frameMeta;
        const size_t minimumSeed = (size_t)seedEstimate;
        size_t usable = 0;
        for (size_t i = 0; i < frame.positions.size() && usable < minimumSeed; ++i) {
            const float depth = rawFrame.depths[i];
            const Vector3f position = standard_cyborg::toVector3f(frame.positions[i]);
            const Vector3f normal = standard_cyborg::toVector3f(frame.normals[i]);
            if (std::isfinite(depth) && depth > surfelFusionConfiguration.minDepth && depth <= surfelFusionConfiguration.maxDepth &&
                frame.inputConfidences[i] >= surfelFusionConfiguration.inputConfidenceThreshold && frame.weights[i] > 0 &&
                position.allFinite() && normal.allFinite() && position.norm() > 0 &&
                -position.dot(normal) / position.norm() >= std::cos(surfelFusionConfiguration.maxSurfelIncidenceThreshold)) ++usable;
        }
        if (usable < minimumSeed) return frameMeta;
    }
    if (_surfels.size() > 0) {''')

replace('pbfSource',"        DEBUG_LOG(\"Frame couldn't be fused.\");","        _extrinsicMatrix = previousFrameMeta ? previousFrameMeta->viewMatrix : Matrix4f::Identity();\n        DEBUG_LOG(\"Frame couldn't be fused.\");")
# Borrowed vector storage is not a VM-backed Metal allocation contract. Own
# bounded working buffers and copy only valid bytes, never padded vector pages.
replace('depthDataHeader','    size_t width;','    bool sdaCompleted = false;\n    size_t width;')
replace('depthData','#import <iostream>','#import <iostream>\n#include <cstring>')
replace('depthData','    width = widthIn;','    sdaCompleted = false;\n    width = widthIn;')
for vector,buffer,ctype in [('points','pointsBuffer','math::Vec3'),('normals','normalsBuffer','math::Vec3'),('weights','weightsBuffer','float'),('inputConfidences','inputConfidencesBuffer','float'),('surfelSizes','surfelSizesBuffer','float')]:
 size={'points':'pointsBufferSize','normals':'normalsBufferSize','weights':'weightsBufferSize','inputConfidences':'inputConfidencesSize','surfelSizes':'surfelSizesSize'}[vector]
 replace('depthData',f'roundUpToMultiple({vector}.size() * sizeof({ctype}), 4096)',f'{vector}.size() * sizeof({ctype})')
 replace('depthData',f'    {buffer} = [device newBufferWithBytesNoCopy:{vector}.data() length:{size} options:0 deallocator:NULL];',f'''    if ({buffer} == nil || {buffer}.length != {size}) {{
        {buffer} = [device newBufferWithLength:{size} options:MTLResourceStorageModeShared];
    }}
    if ({buffer}.contents != nullptr && {size} > 0) std::memcpy({buffer}.contents, {vector}.data(), {size});''')
replace('computeEngine','    [commandBuffer waitUntilCompleted];','    [commandBuffer waitUntilCompleted];\n    depthProcessorData.sdaCompleted = commandBuffer.status == MTLCommandBufferStatusCompleted;')
replace('depthProcessor','#import <vector>','#import <vector>\n#include <algorithm>\n#include <cstring>')
replace('depthProcessor','    [_computeEngine runWithDepthProcessorData:_depthProcessorData];','''    // Do not expose stale GPU outputs after an allocation/command failure.
    auto reject = [&]() {
        std::fill(frame.weights.begin(), frame.weights.end(), -1.0f);
        std::fill(frame.inputConfidences.begin(), frame.inputConfidences.end(), 0.0f);
    };
    if (!_depthProcessorData.pointsBuffer || !_depthProcessorData.normalsBuffer ||
        !_depthProcessorData.weightsBuffer || !_depthProcessorData.inputConfidencesBuffer ||
        !_depthProcessorData.surfelSizesBuffer || !_depthProcessorData.workBuffer ||
        !_depthProcessorData.depthTexture || !_depthProcessorData.workTexture || !_depthProcessorData.smoothedDepthTexture) {
        reject(); return;
    }
    [_computeEngine runWithDepthProcessorData:_depthProcessorData];
    if (!_depthProcessorData.sdaCompleted) { reject(); return; }
    std::memcpy(frame.positions.data(), _depthProcessorData.pointsBuffer.contents, frame.positions.size() * sizeof(math::Vec3));
    std::memcpy(frame.normals.data(), _depthProcessorData.normalsBuffer.contents, frame.normals.size() * sizeof(math::Vec3));
    std::memcpy(frame.weights.data(), _depthProcessorData.weightsBuffer.contents, frame.weights.size() * sizeof(float));
    std::memcpy(frame.inputConfidences.data(), _depthProcessorData.inputConfidencesBuffer.contents, frame.inputConfidences.size() * sizeof(float));
    std::memcpy(frame.surfelSizes.data(), _depthProcessorData.surfelSizesBuffer.contents, frame.surfelSizes.size() * sizeof(float));''')
start=texts['indexMap'].index('        // On iOS devices, we write directly into indexLookups')
end=texts['indexMap'].index('        _indexTexture.label',start)
replace('indexMap',texts['indexMap'][start:end],'''        descriptor.usage = MTLTextureUsageRenderTarget;
#if !TARGET_OS_OSX
        descriptor.storageMode = MTLStorageModeShared;
#endif
        _indexTexture = [_device newTextureWithDescriptor:descriptor];
''')
replace('indexMap','''        surfelsBuffer = [_device newBufferWithBytesNoCopy:(void *)&surfels[0]
                                                   length:roundUpToMultiple(sizeof(Surfel) * surfels.size(), 4096)
                                                  options:0
                                              deallocator:NULL];''','''        surfelsBuffer = [_device newBufferWithBytes:surfels.data()
                                                   length:sizeof(Surfel) * surfels.size()
                                                  options:MTLResourceStorageModeShared];''')
replace('indexMap','''        surfelsBuffer = [_device newBufferWithBytesNoCopy:(void *)surfels
                                                   length:roundUpToMultiple(sizeof(Surfel) * surfelCount, 4096)
                                                  options:0
                                              deallocator:NULL];''','''        surfelsBuffer = [_device newBufferWithBytes:surfels
                                                   length:sizeof(Surfel) * surfelCount
                                                  options:MTLResourceStorageModeShared];''')
replace('indexMap','''    // On iOS devices, we've written directly into indexLookups, so no need to copy the result back out
#if TARGET_OS_OSX
    [_indexTexture getBytes:indexLookups.data()
                bytesPerRow:rawFrame.width * sizeof(uint32_t)
                 fromRegion:MTLRegionMake2D(0, 0, rawFrame.width, rawFrame.height)
                mipmapLevel:0];
#endif''','''    if (commandBuffer.status != MTLCommandBufferStatusCompleted) return false;
    [_indexTexture getBytes:indexLookups.data()
                bytesPerRow:rawFrame.width * sizeof(uint32_t)
                 fromRegion:MTLRegionMake2D(0, 0, rawFrame.width, rawFrame.height)
                mipmapLevel:0];''')
replace('indexMap','_indexTexture.width != rawFrame.width)', '_indexTexture.width != rawFrame.width || _indexTexture.height != rawFrame.height)')
replace('indexMap','_depthTexture.width != rawFrame.width)', '_depthTexture.width != rawFrame.width || _depthTexture.height != rawFrame.height)')

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
            _sdaNeedsTrackingRecovery.store(false, std::memory_order_release);
            _sdaIsAligning.store(false, std::memory_order_release);
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

# PLY float properties preserve double_val. Auxiliary integer representations must
# not cast negative/out-of-range floats to unsigned or overflow signed integers.
replace('poissonPLY','#include <math.h>','''#include <math.h>
#include <limits>
template<class Integer> static Integer sdaPlyInteger(double value) {
    if (value != value) return 0;
    if (value <= static_cast<double>(std::numeric_limits<Integer>::lowest())) return std::numeric_limits<Integer>::lowest();
    if (value >= static_cast<double>(std::numeric_limits<Integer>::max())) return std::numeric_limits<Integer>::max();
    return static_cast<Integer>(value);
}''')
assert texts['poissonPLY'].count('int_val = (int)double_val;') == 5
texts['poissonPLY']=texts['poissonPLY'].replace('int_val = (int)double_val;','int_val = sdaPlyInteger<int>(double_val);')
assert texts['poissonPLY'].count('uint_val = (unsigned int)double_val;') == 5
texts['poissonPLY']=texts['poissonPLY'].replace('uint_val = (unsigned int)double_val;','uint_val = sdaPlyInteger<unsigned int>(double_val);')
assert texts['poissonPLY'].count('longlong_val = (long long)double_val;') == 5
texts['poissonPLY']=texts['poissonPLY'].replace('longlong_val = (long long)double_val;','longlong_val = sdaPlyInteger<long long>(double_val);')
assert texts['poissonPLY'].count('ulonglong_val = (unsigned long long)double_val;') == 3
texts['poissonPLY']=texts['poissonPLY'].replace('ulonglong_val = (unsigned long long)double_val;','ulonglong_val = sdaPlyInteger<unsigned long long>(double_val);')

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

# Poisson surfaces have no UVs. Copy only present attributes rather than reading
# vertexCount bytes from null/short optional vectors (observed simulator SIGSEGV).
replace('meshGeometry','''return [NSData dataWithBytes:geo.getColors().data()
                          length:geo.vertexCount() * sizeof(Vec3)];''','''return [NSData dataWithBytes:geo.getColors().data()
                          length:geo.getColors().size() * sizeof(Vec3)];''')
replace('meshGeometry','''return [NSData dataWithBytes:(const void *)normalizedNormals.data()
                          length:geo.vertexCount() * sizeof(Vec3)];''','''return [NSData dataWithBytes:(const void *)normalizedNormals.data()
                          length:normalizedNormals.size() * sizeof(Vec3)];''')
replace('meshGeometry','''return [NSData dataWithBytes:(const void *)geo.getTexCoords().data()
                          length:geo.vertexCount() * sizeof(Vec2)];''','''return [NSData dataWithBytes:(const void *)geo.getTexCoords().data()
                          length:geo.getTexCoords().size() * sizeof(Vec2)];''')
replace('meshFileHeader','- (instancetype)initWithPLYPath:','- (nullable instancetype)initWithPLYPath:')
replace('meshFileIO','#import <vector>','#import <vector>\n#include <exception>')
replace('meshFileIO','''    sc3d::Geometry geometry;
    io::ply::ReadGeometryFromPLYFile(geometry, [PLYPath UTF8String]);
    
    return [SCMesh meshFromGeometry:geometry
                    textureJPEGPath:JPEGPath];''','''    try {
        sc3d::Geometry geometry;
        if (!io::ply::ReadGeometryFromPLYFile(geometry, [PLYPath UTF8String])) return nil;
        return [SCMesh meshFromGeometry:geometry textureJPEGPath:JPEGPath];
    } catch (const std::exception&) { return nil; }
      catch (...) { return nil; }''')
replace('geometryPLY','''        for (auto faceList : faceData) {
            faces.push_back(Face3({faceList[0], faceList[1], faceList[2]}));''','''        for (auto faceList : faceData) {
            if (faceList.size() != 3) return false;
            faces.push_back(Face3({faceList[0], faceList[1], faceList[2]}));''')

# Reject incomplete scalar/list records before indexing or resizing. Binary list
# lengths are bounded before allocating; empty lists do not dereference front().
assert texts['happly'].count("tokens[currEntry]") == 9
texts['happly']=texts['happly'].replace("tokens[currEntry]","tokens.at(currEntry)")
assert texts['happly'].count("iss >> count;") == 4
texts['happly']=texts['happly'].replace("iss >> count;","if (!(iss >> count)) throw std::runtime_error(\"PLY parser: invalid element/list count\");")
assert texts['happly'].count("iss >> intVal;") == 4
texts['happly']=texts['happly'].replace("iss >> intVal;","if (!(iss >> intVal)) throw std::runtime_error(\"PLY parser: invalid integer value\");")
assert texts['happly'].count("iss >> data.back();") == 1
texts['happly']=texts['happly'].replace("iss >> data.back();","if (!(iss >> data.back())) throw std::runtime_error(\"PLY parser: invalid scalar value\");")
assert texts['happly'].count("iss >> data.back()[iCount];") == 1
texts['happly']=texts['happly'].replace("iss >> data.back()[iCount];","if (!(iss >> data.back()[iCount])) throw std::runtime_error(\"PLY parser: invalid list value\");")
assert texts['happly'].count('    currEntry++;\n\n    data.emplace_back();') == 1
texts['happly']=texts['happly'].replace('    currEntry++;\n\n    data.emplace_back();','    currEntry++;\n    if (count > tokens.size()-currEntry) throw std::runtime_error("PLY parser: incomplete list");\n\n    data.emplace_back();')
assert texts['happly'].count('  currEntry++;\n\n  std::vector<') == 2
texts['happly']=texts['happly'].replace('  currEntry++;\n\n  std::vector<','  currEntry++;\n  if (count > tokens.size()-currEntry) throw std::runtime_error("PLY parser: incomplete list");\n\n  std::vector<')
assert texts['happly'].count("    stream.read((char*)&data.back(), sizeof(T));") == 2
texts['happly']=texts['happly'].replace("    stream.read((char*)&data.back(), sizeof(T));","    stream.read((char*)&data.back(), sizeof(T));\n    if (!stream) throw std::runtime_error(\"PLY parser: truncated binary scalar\");")
assert texts['happly'].count("    stream.read(((char*)&count), listCountBytes);") == 2
texts['happly']=texts['happly'].replace("    stream.read(((char*)&count), listCountBytes);","    stream.read(((char*)&count), listCountBytes);\n    if (!stream) throw std::runtime_error(\"PLY parser: truncated binary list count\");")
assert texts['happly'].count("    data.back().resize(count);\n    stream.read((char*)&data.back().front(), count * sizeof(T));") == 2
texts['happly']=texts['happly'].replace("    data.back().resize(count);\n    stream.read((char*)&data.back().front(), count * sizeof(T));","    if (count > 1048576) throw std::runtime_error(\"PLY parser: binary list length exceeds local bound\");\n    data.back().resize(count);\n    if (count && !stream.read((char*)data.back().data(), count * sizeof(T))) throw std::runtime_error(\"PLY parser: truncated binary list\");")
assert texts['happly'].count("        std::getline(inStream, line);") == 1
texts['happly']=texts['happly'].replace("        std::getline(inStream, line);","        if (!std::getline(inStream, line)) throw std::runtime_error(\"PLY parser: truncated ASCII record\");")

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
prediction=recovery.with_name('SdaTrackingPrediction.hpp')
predictionTarget=recoveryTarget.with_name(prediction.name)
predictionTarget.write_bytes(prediction.read_bytes())
record[str(predictionTarget.relative_to(root))]=hashlib.sha256(prediction.read_bytes()).hexdigest()
convergence=recovery.with_name('SdaTrackingConvergence.hpp')
convergenceTarget=recoveryTarget.with_name(convergence.name)
convergenceTarget.write_bytes(convergence.read_bytes())
record[str(convergenceTarget.relative_to(root))]=hashlib.sha256(convergence.read_bytes()).hexdigest()
cleanup=Path(__file__).resolve().parents[1]/'apps/mobile/modules/sda-core/ios/PersonalHrtfNative/SdaPoissonMeshCleanup.hpp'
cleanupTarget=root/Path(paths['poissonAdapter'][0]).parent/cleanup.name
cleanupTarget.write_bytes(cleanup.read_bytes());record[str(cleanupTarget.relative_to(root))]=hashlib.sha256(cleanup.read_bytes()).hexdigest()
meshHeader=Path(__file__).resolve().parents[1]/'apps/mobile/modules/sda-core/ios/PersonalHrtfNative/SdaHrtfMesh.h'
for key in ['fusionHeader']:
 target=root/Path(paths[key][0]).parent/'SdaHrtfMesh.h'
 target.write_bytes(meshHeader.read_bytes());record[str(target.relative_to(root))]=hashlib.sha256(meshHeader.read_bytes()).hexdigest()
for key,(path,_)in paths.items():
 data=texts[key].encode();(root/path).write_bytes(data);record[path]=hashlib.sha256(data).hexdigest()
(root.parent/'fusion-lifecycle-patch.json').write_text(json.dumps({'pinnedCommit':'8c3bc698944ec41edaa6f627db15a6573e8e2af2','patches':['atomic model-thread point-count snapshot','memory-configured strict per-insertion point cap; capture limit preserves measured model','serialized gravity estimator access','recoverable tracking loss retains model and pending input; normalized rotation angle avoids acos NaN','model reset completion after memory release'],'sha256':record},indent=2)+'\n')
