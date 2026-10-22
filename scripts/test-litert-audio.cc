// Checks exact production model bytes with the public LiteRT 2.3.0 C API.
// CPU test mode validates schema/ABI/buffers; it cannot prove NPU execution.
#include "litert/c/litert_compiled_model.h"
#include "litert/c/litert_environment.h"
#include "litert/c/litert_model.h"
#include "litert/c/litert_options.h"
#include "litert/c/litert_tensor_buffer.h"
#include <vector>
#include <string>
#include <cstdio>
#include <cstdlib>
#include <cmath>
#include <cstring>
#include <algorithm>
static_assert(sizeof(LiteRtRankedTensorType)==72);
static_assert(sizeof(LiteRtEnvOption)==24);
#define CHECK(x) do {int c=(x);if(c){fprintf(stderr,"%s => %d\n",#x,c);return false;}}while(0)
static bool run(const char *root,int parts,int rows,int bins){
 char path[1024];snprintf(path,sizeof(path),"%s/spectral-%s.tflite",root,parts==0?"live":std::to_string(parts).c_str());if(parts==0)parts=2;
 LiteRtEnvOption option{};option.tag=kLiteRtEnvOptionTagAutoRegisterAccelerators;option.value.type=kLiteRtAnyTypeInt;option.value.int_value=kLiteRtHwAcceleratorCpu;
 LiteRtEnvironment env{};CHECK(LiteRtCreateEnvironment(1,&option,&env));
 LiteRtModel model{};CHECK(LiteRtCreateModelFromFile(env,path,&model));
 LiteRtOptions options{};CHECK(LiteRtCreateOptions(&options));CHECK(LiteRtSetOptionsHardwareAccelerators(options,kLiteRtHwAcceleratorCpu));
 LiteRtCompiledModel compiled{};CHECK(LiteRtCreateCompiledModel(env,model,options,&compiled));
 bool noncpu=true;CHECK(LiteRtCompiledModelIsNonCpuFullyAccelerated(compiled,&noncpu));if(noncpu){fprintf(stderr,"CPU test reported non-CPU\n");return false;}
 LiteRtSubgraph sg{};CHECK(LiteRtGetModelSubgraph(model,0,&sg));
 const size_t n=(size_t)rows*bins;std::vector<float> input(n*parts*6),reference(n*4,0),output(n*4);
 for(size_t i=0;i<input.size();i++)input[i]=((int)(i*7919%1021)-510)/511.f;
 for(int p=0;p<parts;p++){auto v=input.data()+p*6*n;for(size_t i=0;i<n;i++)for(int e=0;e<2;e++){
  float h=v[(2+2*e)*n+i],k=v[(3+2*e)*n+i];reference[2*e*n+i]+=v[i]*h-v[n+i]*k;reference[(2*e+1)*n+i]+=v[i]*k+v[n+i]*h;
 }}
 std::vector<LiteRtTensorBuffer> ins,outs;
 for(int kind=0;kind<2;kind++)for(int i=0;i<(kind?4:parts*6);i++){
  LiteRtTensor tensor{};CHECK(kind?LiteRtGetSubgraphOutput(sg,i,&tensor):LiteRtGetSubgraphInput(sg,i,&tensor));
  LiteRtRankedTensorType ty{};CHECK(LiteRtGetRankedTensorType(tensor,&ty));if(ty.element_type!=kLiteRtElementTypeFloat32||ty.layout.rank!=2||ty.layout.dimensions[0]!=rows||ty.layout.dimensions[1]!=bins)return false;
  LiteRtTensorBufferRequirements req{};CHECK(kind?LiteRtGetCompiledModelOutputBufferRequirements(compiled,0,i,&req):LiteRtGetCompiledModelInputBufferRequirements(compiled,0,i,&req));
  LiteRtTensorBuffer b{};CHECK(LiteRtCreateManagedTensorBufferFromRequirements(env,&ty,req,&b));(kind?outs:ins).push_back(b);
  if(!kind){void *ptr{};CHECK(LiteRtLockTensorBuffer(b,&ptr,kLiteRtTensorBufferLockModeWrite));memcpy(ptr,input.data()+i*n,n*sizeof(float));CHECK(LiteRtUnlockTensorBuffer(b));}
 }
 CHECK(LiteRtRunCompiledModel(compiled,0,ins.size(),ins.data(),outs.size(),outs.data()));
 for(int i=0;i<4;i++){void *ptr{};CHECK(LiteRtLockTensorBuffer(outs[i],&ptr,kLiteRtTensorBufferLockModeRead));memcpy(output.data()+i*n,ptr,n*sizeof(float));CHECK(LiteRtUnlockTensorBuffer(outs[i]));}
 double energy=0,error=0;float peak=0,maxerr=0;for(size_t i=0;i<output.size();i++){
  if(!std::isfinite(output[i]))return false;double delta=(double)output[i]-reference[i];energy+=(double)reference[i]*reference[i];error+=delta*delta;peak=std::max(peak,std::abs(reference[i]));maxerr=std::max(maxerr,(float)std::abs(delta));
 }
 bool pass=error<=energy*1e-12&&maxerr<=peak*4e-6;
 printf("parts=%d rows=%d bins=%d relativeRms=%.9g relativePeak=%.9g precisionGate=%s backend=CPU-test NPUExecutionVerified=false\n",parts,rows,bins,std::sqrt(error/std::max(energy,1e-300)),maxerr/std::max(peak,1e-30f),pass?"pass":"reject");
 for(auto b:ins)LiteRtDestroyTensorBuffer(b);for(auto b:outs)LiteRtDestroyTensorBuffer(b);LiteRtDestroyCompiledModel(compiled);LiteRtDestroyOptions(options);LiteRtDestroyModel(model);LiteRtDestroyEnvironment(env);return pass;
}
int main(int argc,char **argv){if(argc!=2)return 2;for(int p=1;p<=4;p++)if(!run(argv[1],p,8,17))return 3;if(!run(argv[1],0,32,1025))return 4;return 0;}
