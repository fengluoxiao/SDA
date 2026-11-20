// Compiled into the verification framework only, never the packaged app.
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#import <StandardCyborgFusion/SCFusionBundle.h>
#include "../Algorithm/PBFModel.hpp"
#include "../MetalDepthProcessor/MetalSurfelIndexMap.hpp"
#include "../MetalDepthProcessor/MetalDepthProcessor.hpp"
#include "SdaTrackingRecovery.hpp"
#include <cmath>
#include <cstdio>

// Failure injection wraps the real renderer; accepted frames still use Metal.
class SdaTestIndexMap final : public SurfelIndexMap {
    std::shared_ptr<MetalSurfelIndexMap> real;
public:
    bool failNext = false;
    explicit SdaTestIndexMap(std::shared_ptr<MetalSurfelIndexMap> map):real(map) {}
    bool draw(const std::vector<Surfel>& s,const Eigen::Matrix4f& m,const RawFrame& f,std::vector<uint32_t>& out) override {
        if(failNext) { failNext=false; return false; }
        return real->draw(s,m,f,out);
    }
    bool drawForColor(const Surfel* s,size_t n,Eigen::Matrix4f m,size_t w,size_t h,std::vector<uint32_t>& out) override {
        return real->drawForColor(s,n,m,w,h,out);
    }
    Eigen::Matrix4f getViewProjectionMatrix() override { return real->getViewProjectionMatrix(); }
};

extern "C" __attribute__((visibility("default"))) uint32_t sdaTestTrackingFusion() {
 @autoreleasepool {
    printf("SDA tracking fixture: creating Metal resources\n");
    auto device = MTLCreateSystemDefaultDevice();
    NSError *error = nil;
    auto library = [device newDefaultLibraryWithBundle:[SCFusionBundle fusionBundle] error:&error];
    auto queue = [device newCommandQueue];
    if (!device || !library || !queue) return 1;
    printf("SDA tracking fixture: creating index map\n");
    auto index = std::make_shared<SdaTestIndexMap>(std::make_shared<MetalSurfelIndexMap>(device, library, queue));
    printf("SDA tracking fixture: creating depth processor\n");
    MetalDepthProcessor processor(device, library, queue);
    PBFModel model(index, 937);
    PBFConfiguration pbf;
    pbf.icpDownsampleFraction = 0.1;
    ICPConfiguration icp;
    icp.maxIterations = 32;
    icp.tolerance = 0.00002;
    icp.threadCount = 2;
    SurfelFusionConfiguration fusion;
    fusion.minDepth = 0.15;
    fusion.maxDepth = 0.65;
    fusion.maximumSurfelCount = 100000;
    fusion.cullLowConfidence = false;
    const int width=160, height=120;
    sc3d::PerspectiveCamera camera;
    camera.setIntrinsicMatrixReferenceSize(math::Vec2(width,height));
    camera.setNominalIntrinsicMatrix(math::Mat3x3(160,0,80, 0,160,60, 0,0,1));
    printf("SDA tracking fixture: allocating calibrated frame\n");
    auto frame = std::make_unique<ProcessedFrame>(camera,width,height);
    auto prepare = [&](bool visible) {
        for(int y=0;y<height;y++)for(int x=0;x<width;x++) {
            const auto i=y*width+x;
            const float nx=(x-80)/80.0f, ny=(y-60)/60.0f;
            // Non-planar, asymmetric synthetic depth. No claim of physical ear accuracy.
            frame->rawFrame.depths[i]=visible ? 0.40f + 0.025f*nx*nx + 0.035f*ny*ny
                - 0.035f*std::exp(-18*(nx*nx+ny*ny)) + 0.005f*nx*ny : 1.0f;
            frame->rawFrame.colors[i]=math::Vec3(0.5,0.4,0.3);
        }
        processor.computeFrameValues(*frame,frame->rawFrame);
    };
    prepare(false);
    auto blank=model.assimilate(*frame,pbf,icp,fusion,0.5);
    if(blank.isMerged || !model.getSurfels().empty()) return 12;
    printf("SDA tracking fixture: processing visible depth\n");
    prepare(true);
    printf("SDA tracking fixture: fusing initial depth\n");
    auto first=model.assimilate(*frame,pbf,icp,fusion,1.0);
    printf("SDA tracking fixture initial: merged=%d points=%zu\n",first.isMerged,model.getSurfels().size());
    if(!first.isMerged || model.getSurfels().size()<1000) return 2;
    int accepted=0;
    for(int i=1;i<=16;i++) {
        auto result=model.assimilate(*frame,pbf,icp,fusion,1.0+i/30.0);
        if(result.isMerged) accepted++;
    }
    printf("SDA tracking fixture stationary: accepted=%d/16\n",accepted);
    if(accepted<8) return 3;
    const auto saved=model.getSurfels();
    const Eigen::Matrix4f savedPose=model.getCurrentExtrinsicMatrix();
    index->failNext = true;
    for(int i=0;i<12 && index->failNext;i++) {
        auto failedDraw=model.assimilate(*frame,pbf,icp,fusion,2.0+i/30.0);
        if(failedDraw.isMerged || model.getSurfels().size()!=saved.size() ||
           !model.getCurrentExtrinsicMatrix().isApprox(savedPose,0)) return 13;
    }
    if(index->failNext) return 13;
    SdaTrackingRecovery recovery;
    recovery.terminal(true,2);
    prepare(false);
    for(int i=0;i<=10;i++) {
        auto result=model.assimilate(*frame,pbf,icp,fusion,3.0+i);
        if(result.isMerged || recovery.terminal(result.isMerged,3.0+i)) return 4;
        if(model.getSurfels().size()!=saved.size() ||
           !model.getCurrentExtrinsicMatrix().isApprox(savedPose,0)) return 5;
        for(size_t k=0;k<saved.size();k++)
            if(!model.getSurfels()[k].position.isApprox(saved[k].position,0) ||
               !model.getSurfels()[k].normal.isApprox(saved[k].normal,0) ||
               model.getSurfels()[k].weight!=saved[k].weight) return 6;
    }
    if(!recovery.needsReturnToView(13)) return 7;
    prepare(true);
    bool recovered=false;
    for(int i=0;i<12;i++) {
        auto result=model.assimilate(*frame,pbf,icp,fusion,14.0+i/30.0);
        if(recovery.terminal(result.isMerged,14.0+i/30.0)) return 8;
        if(result.isMerged) { recovered=true; break; }
    }
    if(!recovered || recovery.needsReturnToView(15)) return 9;
    if(!model.getCurrentExtrinsicMatrix().isApprox(savedPose,0.001f)) return 10;
    model.reset();
    if(!model.getSurfels().empty()) return 11;
    printf("SDA tracking fixture: 10s blank rejected, surfels/pose unchanged, return-to-view recovered, reset passed\n");
    return 0;
 }
}
