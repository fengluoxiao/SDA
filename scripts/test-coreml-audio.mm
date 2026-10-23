#import <Foundation/Foundation.h>
#include "../apps/native-renderer/src/coreml_bridge.h"
#include <vector>
#include <cstdio>
#include <cmath>
#include <cstring>
#include <algorithm>
#include <chrono>
static bool run(NSString *path,size_t rows,size_t bins,size_t parts,int cpu) {
    NSData *bytes=[NSData dataWithContentsOfFile:path];if(!bytes){fprintf(stderr,"missing fixture\n");return false;}
    char error[512]={};void *m=sda_coreml_create((const uint8_t *)bytes.bytes,bytes.length,rows,bins,parts*6,cpu,error,sizeof(error));
    if(!m){fprintf(stderr,"create: %s\n",error);return false;}
    const size_t n=rows*bins;std::vector<float> in(n*parts*6),out(n*4+8,-919.25f),ref(n*4,0.f);
    for(size_t i=0;i<in.size();i++)in[i]=((int)((i*7919)%1021)-510)/511.f;
    auto original=in;
    for(size_t p=0;p<parts;p++){const float *v=in.data()+p*6*n;
        for(size_t i=0;i<n;i++)for(size_t ear=0;ear<2;ear++){
            float hr=v[(2+2*ear)*n+i],hi=v[(3+2*ear)*n+i];
            ref[(2*ear)*n+i]+=v[i]*hr-v[n+i]*hi;ref[(2*ear+1)*n+i]+=v[i]*hi+v[n+i]*hr;
        }
    }
    // Contract errors must return without writing outside the packet.
    if(sda_coreml_predict(m,in.data(),in.size()-1,out.data()+4,n*4,error,sizeof(error))==0){sda_coreml_free(m);return false;}
    double elapsed=0;
    for(int step=0;step<4;step++){
        auto start=std::chrono::steady_clock::now();
        int rc=sda_coreml_predict(m,in.data(),in.size(),out.data()+4,n*4,error,sizeof(error));
        elapsed+=std::chrono::duration<double,std::micro>(std::chrono::steady_clock::now()-start).count();
        if(rc){fprintf(stderr,"prediction: %s\n",error);sda_coreml_free(m);return false;}
    }
    if(in!=original){fprintf(stderr,"input mutated\n");sda_coreml_free(m);return false;}
    for(int i=0;i<4;i++)if(out[i]!=-919.25f||out[out.size()-1-i]!=-919.25f){sda_coreml_free(m);return false;}
    double energy=0,delta=0;float peak=0,maxerr=0;
    for(size_t i=0;i<ref.size();i++){float actual=out[i+4];if(!std::isfinite(actual)){sda_coreml_free(m);return false;}
        double d=(double)actual-ref[i];energy+=(double)ref[i]*ref[i];delta+=d*d;peak=std::max(peak,std::abs(ref[i]));maxerr=std::max(maxerr,(float)std::abs(d));}
    bool production_precision=(delta<=energy*1e-12&&maxerr<=peak*4e-6);
    printf("rows=%zu bins=%zu partitions=%zu request=%s rmsRelative=%.9g peakRelative=%.9g meanUs=%.1f precisionGate=%s hardwareExecutionVerified=false\n",
        rows,bins,parts,cpu?"CPUOnly-test":"CPUAndNeuralEngine",std::sqrt(delta/std::max(energy,1e-300)),maxerr/std::max(peak,1e-30f),elapsed/4,production_precision?"pass":"reject");
    sda_coreml_free(m);
    // FP32 regression uses CPUOnly; ANE internal precision is evaluated rather
    // than weakening the production gate to make this optional request pass.
    return !cpu||production_precision;
}
static bool overlapping_contexts(NSString *path) {
 NSData *bytes=[NSData dataWithContentsOfFile:path];char error[512]={};
 NSURL *cache=[[NSFileManager.defaultManager URLsForDirectory:NSCachesDirectory inDomains:NSUserDomainMask] firstObject];
 NSURL *parent=[cache URLByAppendingPathComponent:@"sda-coreml-spectral-v2" isDirectory:YES];
 auto count=[&](){return [NSFileManager.defaultManager contentsOfDirectoryAtURL:parent includingPropertiesForKeys:nil options:0 error:nil].count;};
 NSUInteger initial=count();
 void *a=sda_coreml_create((const uint8_t *)bytes.bytes,bytes.length,8,17,6,1,error,sizeof(error));
 void *b=sda_coreml_create((const uint8_t *)bytes.bytes,bytes.length,8,17,6,1,error,sizeof(error));
 if(!a||!b){sda_coreml_free(a);sda_coreml_free(b);return false;}
 bool ok=count()==initial+2;sda_coreml_free(a);ok=ok&&count()==initial+1;sda_coreml_free(b);ok=ok&&count()==initial;
 if(ok)puts("PASS independent Core ML model-cache lifetimes");return ok;
}
int main(int argc,char **argv){@autoreleasepool{
    if(argc!=2)return 2;NSString *root=[NSString stringWithUTF8String:argv[1]];
    for(size_t p=1;p<=4;p++)if(!run([root stringByAppendingPathComponent:[NSString stringWithFormat:@"spectral-%zu.mlmodel",p]],8,17,p,1))return 3;
    if(!run([root stringByAppendingPathComponent:@"spectral-live-shape.mlmodel"],32,1025,2,1))return 4;
    if(!overlapping_contexts([root stringByAppendingPathComponent:@"spectral-1.mlmodel"]))return 6;
    int available=sda_coreml_neural_engine_available();printf("neuralEngineVisible=%d (availability does not prove execution)\n",available);
    if(available==1&&!run([root stringByAppendingPathComponent:@"spectral-2.mlmodel"],8,17,2,0))return 5;
    puts("PASS public Core ML graph, FP32 contract, buffers and cleanup");return 0;
}}
