#import <Foundation/Foundation.h>
#include "../apps/native-renderer/src/metal_bridge.h"
#include <vector>
#include <cmath>
#include <cstdio>
#include <algorithm>
static bool run(size_t rows,size_t bins,size_t parts){
 char error[512]={};sda_metal_set_allowed(1);
 void *ctx=sda_metal_create(rows,bins,parts,error,sizeof(error));if(!ctx){fprintf(stderr,"create: %s\n",error);return false;}
 size_t n=rows*bins;std::vector<float> in(n*parts*6),out(n*4+8,-919.25f),ref(n*4,0);
 for(size_t i=0;i<in.size();i++)in[i]=((int)((i*7919)%1021)-510)/511.f;
 auto original=in;
 for(size_t p=0;p<parts;p++){const float *v=in.data()+p*6*n;
  for(size_t i=0;i<n;i++)for(size_t ear=0;ear<2;ear++){
   float hr=v[(2+2*ear)*n+i],hi=v[(3+2*ear)*n+i];
   ref[(2*ear)*n+i]+=v[i]*hr-v[n+i]*hi;ref[(2*ear+1)*n+i]+=v[i]*hi+v[n+i]*hr;
  }
 }
 sda_metal_set_allowed(0);uint64_t before=sda_metal_completed_batches();
 if(sda_metal_predict(ctx,in.data(),in.size(),out.data()+4,n*4,100000000,error,sizeof(error))!=-2){sda_metal_free(ctx);return false;}
 if(sda_metal_completed_batches()!=before){sda_metal_free(ctx);return false;}
 sda_metal_set_allowed(1);
 if(sda_metal_predict(ctx,in.data(),in.size()-1,out.data()+4,n*4,100000000,error,sizeof(error))==0){sda_metal_free(ctx);return false;}
 for(int step=0;step<4;step++){
  int rc=sda_metal_predict(ctx,in.data(),in.size(),out.data()+4,n*4,100000000,error,sizeof(error));
  if(rc){fprintf(stderr,"prediction: %d %s\n",rc,error);sda_metal_free(ctx);return false;}
 }
 bool guards=in==original;for(int i=0;i<4;i++)guards=guards&&out[i]==-919.25f&&out[out.size()-1-i]==-919.25f;
 double energy=0,delta=0;float peak=0,maxerr=0;
 for(size_t i=0;i<ref.size();i++){float v=out[i+4];if(!std::isfinite(v)){sda_metal_free(ctx);return false;}double d=(double)v-ref[i];energy+=(double)ref[i]*ref[i];delta+=d*d;peak=std::max(peak,std::abs(ref[i]));maxerr=std::max(maxerr,(float)std::abs(d));}
 bool exact=delta<=energy*1e-12&&maxerr<=peak*4e-6;
 printf("Metal rows=%zu bins=%zu parts=%zu rmsRelative=%.9g peakRelative=%.9g GPUCompleted=%llu precision=%s\n",rows,bins,parts,std::sqrt(delta/std::max(energy,1e-300)),maxerr/std::max(peak,1e-30f),(unsigned long long)sda_metal_completed_batches(),exact?"pass":"reject");
 sda_metal_free(ctx);return exact&&guards;
}
int main(){@autoreleasepool{
 for(size_t p=1;p<=4;p++)if(!run(8,17,p))return 1;
 if(!run(32,1025,2))return 2;
 sda_metal_set_allowed(0);char error[512]={};if(sda_metal_create(8,17,1,error,sizeof(error)))return 3;
 puts("PASS actual Metal GPU FP32 results, background submission rejection, guards and context cleanup");return 0;
}}
