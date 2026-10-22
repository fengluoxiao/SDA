#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include "metal_bridge.h"
#include <atomic>
#include <mutex>
#include <memory>
#include <cstdio>
#include <cstring>
#include <algorithm>
static std::atomic<bool> allowed{false};
static std::atomic<uint64_t> completed{0};
// Lifecycle only waits for the short submission/scheduling section, never GPU completion.
static std::mutex submission;
static void fail(char *out,size_t cap,NSString *text){if(out&&cap)snprintf(out,cap,"%s",(text?:@"Metal audio error").UTF8String);}
struct Context {
 __strong id<MTLDevice> device;
 __strong id<MTLCommandQueue> queue;
 __strong id<MTLComputePipelineState> pipeline;
 __strong id<MTLBuffer> input,output;
 size_t rows,bins,parts;
 bool retired=false;
};
static const char *kernel = R"metal(
#include <metal_stdlib>
using namespace metal;
kernel void spectral(device const float *v [[buffer(0)]],device float *out [[buffer(1)]],
 constant uint2 &shape [[buffer(2)]],uint i [[thread_position_in_grid]]) {
 uint plane=shape.x,parts=shape.y;if(i>=plane)return;
 float lr=0.0f,li=0.0f,rr=0.0f,ri=0.0f;
 for(uint p=0;p<parts;p++) {
  uint b=p*6*plane+i;float xr=v[b],xi=v[b+plane];
  float ar=xr*v[b+2*plane],bi=xi*v[b+3*plane],ai=xr*v[b+3*plane],br=xi*v[b+2*plane];
  float cr=xr*v[b+4*plane],di=xi*v[b+5*plane],ci=xr*v[b+5*plane],dr=xi*v[b+4*plane];
  lr=lr+(ar-bi);li=li+(ai+br);rr=rr+(cr-di);ri=ri+(ci+dr);
 }
 out[i]=lr;out[plane+i]=li;out[2*plane+i]=rr;out[3*plane+i]=ri;
}
)metal";
void sda_metal_set_allowed(int value){
 std::lock_guard<std::mutex> guard(submission);allowed.store(value!=0,std::memory_order_release);
}
uint64_t sda_metal_completed_batches(void){return completed.load(std::memory_order_relaxed);}
void *sda_metal_create(size_t rows,size_t bins,size_t parts,char *error,size_t cap){@autoreleasepool{@try{
 if(!allowed.load(std::memory_order_acquire)){fail(error,cap,@"GPU-disallowed-in-background");return nullptr;}
 if(rows<1||rows>256||bins<2||bins>4097||parts<1||parts>4||rows*bins*(6*parts+4)>32*1024*1024/4){fail(error,cap,@"invalid-Metal-audio-shape");return nullptr;}
 auto c=std::make_unique<Context>();c->rows=rows;c->bins=bins;c->parts=parts;c->device=MTLCreateSystemDefaultDevice();
 if(!c->device){fail(error,cap,@"Metal-device-unavailable");return nullptr;}
 c->queue=[c->device newCommandQueueWithMaxCommandBufferCount:2];
 MTLCompileOptions *options=[MTLCompileOptions new];options.fastMathEnabled=NO;NSError *e=nil;
 id<MTLLibrary> library=[c->device newLibraryWithSource:[NSString stringWithUTF8String:kernel] options:options error:&e];
 if(!library){fail(error,cap,e.localizedDescription);return nullptr;}
 c->pipeline=[c->device newComputePipelineStateWithFunction:[library newFunctionWithName:@"spectral"] error:&e];
 if(!c->pipeline||!c->queue){fail(error,cap,e.localizedDescription);return nullptr;}
 c->input=[c->device newBufferWithLength:rows*bins*parts*6*sizeof(float) options:MTLResourceStorageModeShared];
 c->output=[c->device newBufferWithLength:rows*bins*4*sizeof(float) options:MTLResourceStorageModeShared];
 if(!c->input||!c->output){fail(error,cap,@"Metal-buffer-allocation-failed");return nullptr;}
 return c.release();
}@catch(NSException *e){fail(error,cap,e.reason);return nullptr;}}}
int sda_metal_predict(void *handle,const float *input,size_t ni,float *output,size_t no,uint64_t timeout_ns,char *error,size_t cap){@autoreleasepool{@try{
 Context *c=static_cast<Context *>(handle);
 if(!c||!input||!output||ni!=c->rows*c->bins*c->parts*6||no!=c->rows*c->bins*4){fail(error,cap,@"Metal-buffer-contract-mismatch");return -1;}
 if(c->retired){fail(error,cap,@"Metal-context-retired-after-timeout");return -7;}
 if(!allowed.load(std::memory_order_acquire)){fail(error,cap,@"GPU-disallowed-in-background");return -2;}
 memcpy(c->input.contents,input,ni*sizeof(float));
 id<MTLCommandBuffer> command=[c->queue commandBuffer];id<MTLComputeCommandEncoder> encoder=[command computeCommandEncoder];
 if(!command||!encoder){fail(error,cap,@"Metal-command-unavailable");return -3;}
 [encoder setComputePipelineState:c->pipeline];[encoder setBuffer:c->input offset:0 atIndex:0];[encoder setBuffer:c->output offset:0 atIndex:1];
 uint32_t shape[2]={(uint32_t)(c->rows*c->bins),(uint32_t)c->parts};[encoder setBytes:shape length:sizeof(shape) atIndex:2];
 NSUInteger width=std::min((NSUInteger)256,c->pipeline.maxTotalThreadsPerThreadgroup);
 [encoder dispatchThreads:MTLSizeMake(shape[0],1,1) threadsPerThreadgroup:MTLSizeMake(width,1,1)];[encoder endEncoding];
 dispatch_semaphore_t done=dispatch_semaphore_create(0);dispatch_semaphore_t scheduled=dispatch_semaphore_create(0);
 [command addCompletedHandler:^(id<MTLCommandBuffer>){dispatch_semaphore_signal(done);}];
 [command addScheduledHandler:^(id<MTLCommandBuffer>){dispatch_semaphore_signal(scheduled);}];
 {
  std::lock_guard<std::mutex> guard(submission);
  if(!allowed.load(std::memory_order_acquire)){fail(error,cap,@"GPU-disallowed-before-submit");return -2;}
  [command commit];
  // Bound scheduling by the warmup/live deadline; never wait for completion on UIKit.
  if(dispatch_semaphore_wait(scheduled,dispatch_time(DISPATCH_TIME_NOW,(int64_t)std::min(timeout_ns,(uint64_t)100000000)))!=0){c->retired=true;fail(error,cap,@"Metal-scheduling-deadline");return -4;}
 }
 if(dispatch_semaphore_wait(done,dispatch_time(DISPATCH_TIME_NOW,(int64_t)std::min(timeout_ns,(uint64_t)100000000)))!=0){c->retired=true;fail(error,cap,@"Metal-completion-deadline");return -4;}
 if(command.status!=MTLCommandBufferStatusCompleted){fail(error,cap,command.error.localizedDescription);return -5;}
 memcpy(output,c->output.contents,no*sizeof(float));completed.fetch_add(1,std::memory_order_relaxed);return 0;
}@catch(NSException *e){fail(error,cap,e.reason);return -6;}}}
void sda_metal_free(void *handle){@autoreleasepool{delete static_cast<Context *>(handle);}}
