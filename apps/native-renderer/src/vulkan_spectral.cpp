// Vulkan 1.0 system compute only. All work and cleanup stay on the compute worker.
#define VK_NO_PROTOTYPES
#include <vulkan/vulkan.h>
#include <dlfcn.h>
#include <time.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <new>
struct SdaVkInfo {uint32_t vendor,device,api,driver,queue_family;char name[256];};
namespace {
constexpr uint64_t BUDGET=32ull*1024*1024;
uint64_t monotonic_ns(){timespec t{};clock_gettime(CLOCK_MONOTONIC,&t);return uint64_t(t.tv_sec)*1000000000ull+uint64_t(t.tv_nsec);}
struct Buffer {VkBuffer buffer=VK_NULL_HANDLE;VkDeviceMemory memory=VK_NULL_HANDLE;void* mapped=nullptr;bool coherent=false;uint32_t flags=0;VkDeviceSize allocated=0;};
struct Timings {uint64_t upload=0,dispatch_wait=0,readback=0,total=0,host_cpu=0,input_flags=0,output_flags=0,input_allocated=0,output_allocated=0;};
uint64_t thread_cpu_ns(){timespec t{};if(clock_gettime(CLOCK_THREAD_CPUTIME_ID,&t)!=0)return 0;return uint64_t(t.tv_sec)*1000000000ull+uint64_t(t.tv_nsec);}
#define DEVICE_FUNCTIONS(X) \
 X(vkDestroyDevice) X(vkDeviceWaitIdle) X(vkGetDeviceQueue) \
 X(vkCreateBuffer) X(vkDestroyBuffer) X(vkGetBufferMemoryRequirements) \
 X(vkAllocateMemory) X(vkFreeMemory) X(vkBindBufferMemory) X(vkMapMemory) X(vkUnmapMemory) \
 X(vkFlushMappedMemoryRanges) X(vkInvalidateMappedMemoryRanges) \
 X(vkCreateDescriptorSetLayout) X(vkDestroyDescriptorSetLayout) X(vkCreatePipelineLayout) X(vkDestroyPipelineLayout) \
 X(vkCreateShaderModule) X(vkDestroyShaderModule) X(vkCreateComputePipelines) X(vkDestroyPipeline) \
 X(vkCreateDescriptorPool) X(vkDestroyDescriptorPool) X(vkAllocateDescriptorSets) X(vkUpdateDescriptorSets) \
 X(vkCreateCommandPool) X(vkDestroyCommandPool) X(vkAllocateCommandBuffers) X(vkResetCommandBuffer) \
 X(vkBeginCommandBuffer) X(vkEndCommandBuffer) X(vkCmdPipelineBarrier) \
 X(vkCmdBindPipeline) X(vkCmdBindDescriptorSets) X(vkCmdPushConstants) X(vkCmdDispatch) \
 X(vkCreateFence) X(vkDestroyFence) X(vkWaitForFences) X(vkResetFences) X(vkQueueSubmit)
struct State {
 void* library=nullptr;VkInstance instance=VK_NULL_HANDLE;VkPhysicalDevice physical=VK_NULL_HANDLE;
 VkDevice device=VK_NULL_HANDLE;VkQueue queue=VK_NULL_HANDLE;VkPhysicalDeviceProperties props{};
 VkPhysicalDeviceMemoryProperties memory_props{};SdaVkInfo info{};
 PFN_vkGetInstanceProcAddr get_instance=nullptr;PFN_vkDestroyInstance destroy_instance=nullptr;
#define DECLARE(name) PFN_##name name=nullptr;
 DEVICE_FUNCTIONS(DECLARE)
#undef DECLARE
 VkDescriptorSetLayout descriptor_layout=VK_NULL_HANDLE;VkPipelineLayout pipeline_layout=VK_NULL_HANDLE;
 VkPipeline pipeline=VK_NULL_HANDLE;VkDescriptorPool descriptor_pool=VK_NULL_HANDLE;VkDescriptorSet descriptor=VK_NULL_HANDLE;
 VkCommandPool command_pool=VK_NULL_HANDLE;VkCommandBuffer command=VK_NULL_HANDLE;VkFence fence=VK_NULL_HANDLE;
 uint8_t resident_parts[256]{};bool in_flight=false;Timings timings{};Buffer input,output;uint32_t plane=0,parts=0,bins=0;char error[256]{};
 bool fail(const char* what,int result=-1){snprintf(error,sizeof(error),"%s (%d)",what,result);return false;}
 bool check(VkResult result,const char* what){return result==VK_SUCCESS || fail(what,result);}
 void release(Buffer& b){
  if(b.mapped)vkUnmapMemory(device,b.memory);
  if(b.buffer)vkDestroyBuffer(device,b.buffer,nullptr);
  if(b.memory)vkFreeMemory(device,b.memory,nullptr);b={};
 }
 ~State(){
  // Never free a timed-out submission's storage while the GPU is using it.
  // The audio renderer has already returned to its retained CPU calculation.
  if(device && !vkDestroyDevice)return; // A broken loader must not destroy an instance with a live device.
  if(device){
   if(vkDeviceWaitIdle)vkDeviceWaitIdle(device);
   release(input);release(output);
   if(fence)vkDestroyFence(device,fence,nullptr);
   if(command_pool)vkDestroyCommandPool(device,command_pool,nullptr);
   if(descriptor_pool)vkDestroyDescriptorPool(device,descriptor_pool,nullptr);
   if(pipeline)vkDestroyPipeline(device,pipeline,nullptr);
   if(pipeline_layout)vkDestroyPipelineLayout(device,pipeline_layout,nullptr);
   if(descriptor_layout)vkDestroyDescriptorSetLayout(device,descriptor_layout,nullptr);
   if(vkDestroyDevice)vkDestroyDevice(device,nullptr);
  }
  if(instance && destroy_instance)destroy_instance(instance,nullptr);
  // NODELETE keeps the system loader's TLS callbacks mapped through thread exit.
  if(library)dlclose(library);
 }
 bool open(const uint32_t* code,size_t bytes){
  library=dlopen("libvulkan.so",RTLD_NOW|RTLD_LOCAL|RTLD_NODELETE);
  if(!library)return fail("system Vulkan loader unavailable");
  get_instance=reinterpret_cast<PFN_vkGetInstanceProcAddr>(dlsym(library,"vkGetInstanceProcAddr"));
  if(!get_instance)return fail("vkGetInstanceProcAddr unavailable");
  auto create=reinterpret_cast<PFN_vkCreateInstance>(get_instance(VK_NULL_HANDLE,"vkCreateInstance"));
  if(!create)return fail("vkCreateInstance unavailable");
  VkApplicationInfo app{VK_STRUCTURE_TYPE_APPLICATION_INFO};app.pApplicationName="SDA audio compute";app.apiVersion=VK_API_VERSION_1_0;
  VkInstanceCreateInfo ci{VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO};ci.pApplicationInfo=&app;
  if(!check(create(&ci,nullptr,&instance),"create Vulkan instance"))return false;
  destroy_instance=reinterpret_cast<PFN_vkDestroyInstance>(get_instance(instance,"vkDestroyInstance"));
  if(!destroy_instance)return fail("vkDestroyInstance unavailable");
#define INSTANCE(name) auto name=reinterpret_cast<PFN_##name>(get_instance(instance,#name));if(!name)return fail(#name " unavailable");
  INSTANCE(vkEnumeratePhysicalDevices)
  INSTANCE(vkGetPhysicalDeviceProperties)
  INSTANCE(vkGetPhysicalDeviceQueueFamilyProperties)
  INSTANCE(vkGetPhysicalDeviceMemoryProperties)
  INSTANCE(vkCreateDevice)
  uint32_t count=0;
  if(!check(vkEnumeratePhysicalDevices(instance,&count,nullptr),"enumerate devices") || count==0 || count>32)return fail("no bounded GPU inventory");
  VkPhysicalDevice devices[32]{};
  if(!check(vkEnumeratePhysicalDevices(instance,&count,devices),"read GPU inventory"))return false;
  int best=-1;
  for(uint32_t d=0;d<count;++d){
   VkPhysicalDeviceProperties p{};vkGetPhysicalDeviceProperties(devices[d],&p);
   if(p.deviceType==VK_PHYSICAL_DEVICE_TYPE_CPU || strstr(p.deviceName,"SwiftShader") || strstr(p.deviceName,"llvmpipe") || strstr(p.deviceName,"lavapipe"))continue;
   uint32_t n=0;vkGetPhysicalDeviceQueueFamilyProperties(devices[d],&n,nullptr);if(!n || n>64)continue;
   VkQueueFamilyProperties families[64]{};vkGetPhysicalDeviceQueueFamilyProperties(devices[d],&n,families);
   for(uint32_t f=0;f<n;++f)if(families[f].queueCount && (families[f].queueFlags&VK_QUEUE_COMPUTE_BIT)){
    int score=(families[f].queueFlags&VK_QUEUE_GRAPHICS_BIT)?1:2;
    if(score>best){best=score;physical=devices[d];props=p;info.queue_family=f;}
   }
  }
  if(!physical)return fail("no hardware Vulkan compute queue");
  if(props.limits.maxComputeWorkGroupInvocations<64 || props.limits.maxComputeWorkGroupSize[0]<64 || props.limits.maxPushConstantsSize<12)return fail("insufficient compute limits");
  vkGetPhysicalDeviceMemoryProperties(physical,&memory_props);
  float priority=0.5f;VkDeviceQueueCreateInfo q{VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO};q.queueFamilyIndex=info.queue_family;q.queueCount=1;q.pQueuePriorities=&priority;
  VkDeviceCreateInfo dc{VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO};dc.queueCreateInfoCount=1;dc.pQueueCreateInfos=&q;
  if(!check(vkCreateDevice(physical,&dc,nullptr,&device),"create compute device"))return false;
  vkDestroyDevice=reinterpret_cast<PFN_vkDestroyDevice>(get_instance(instance,"vkDestroyDevice"));
  auto get_device=reinterpret_cast<PFN_vkGetDeviceProcAddr>(get_instance(instance,"vkGetDeviceProcAddr"));
  if(!get_device)return fail("vkGetDeviceProcAddr unavailable");
#define LOAD(name) name=reinterpret_cast<PFN_##name>(get_device(device,#name));if(!name)return fail(#name " unavailable");
  DEVICE_FUNCTIONS(LOAD)
#undef LOAD
#undef INSTANCE
  vkGetDeviceQueue(device,info.queue_family,0,&queue);
  VkDescriptorSetLayoutBinding bindings[2]{};
  for(uint32_t i=0;i<2;++i){bindings[i].binding=i;bindings[i].descriptorType=VK_DESCRIPTOR_TYPE_STORAGE_BUFFER;bindings[i].descriptorCount=1;bindings[i].stageFlags=VK_SHADER_STAGE_COMPUTE_BIT;}
  VkDescriptorSetLayoutCreateInfo dl{VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO};dl.bindingCount=2;dl.pBindings=bindings;
  if(!check(vkCreateDescriptorSetLayout(device,&dl,nullptr,&descriptor_layout),"create descriptor layout"))return false;
  VkPushConstantRange push{VK_SHADER_STAGE_COMPUTE_BIT,0,12};
  VkPipelineLayoutCreateInfo pl{VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO};pl.setLayoutCount=1;pl.pSetLayouts=&descriptor_layout;pl.pushConstantRangeCount=1;pl.pPushConstantRanges=&push;
  if(!check(vkCreatePipelineLayout(device,&pl,nullptr,&pipeline_layout),"create pipeline layout"))return false;
  VkShaderModuleCreateInfo sh{VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO};sh.codeSize=bytes;sh.pCode=code;VkShaderModule shader=VK_NULL_HANDLE;
  if(!check(vkCreateShaderModule(device,&sh,nullptr,&shader),"create FP32 shader"))return false;
  VkComputePipelineCreateInfo cp{VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO};cp.layout=pipeline_layout;
  cp.stage.sType=VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO;cp.stage.stage=VK_SHADER_STAGE_COMPUTE_BIT;cp.stage.module=shader;cp.stage.pName="main";
  VkResult built=vkCreateComputePipelines(device,VK_NULL_HANDLE,1,&cp,nullptr,&pipeline);vkDestroyShaderModule(device,shader,nullptr);
  if(!check(built,"create compute pipeline"))return false;
  VkDescriptorPoolSize pool_size{VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,2};VkDescriptorPoolCreateInfo dp{VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO};dp.maxSets=1;dp.poolSizeCount=1;dp.pPoolSizes=&pool_size;
  if(!check(vkCreateDescriptorPool(device,&dp,nullptr,&descriptor_pool),"create descriptor pool"))return false;
  VkDescriptorSetAllocateInfo da{VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO};da.descriptorPool=descriptor_pool;da.descriptorSetCount=1;da.pSetLayouts=&descriptor_layout;
  if(!check(vkAllocateDescriptorSets(device,&da,&descriptor),"allocate descriptors"))return false;
  VkCommandPoolCreateInfo pc{VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO};pc.flags=VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT;pc.queueFamilyIndex=info.queue_family;
  if(!check(vkCreateCommandPool(device,&pc,nullptr,&command_pool),"create command pool"))return false;
  VkCommandBufferAllocateInfo ca{VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO};ca.commandPool=command_pool;ca.level=VK_COMMAND_BUFFER_LEVEL_PRIMARY;ca.commandBufferCount=1;
  if(!check(vkAllocateCommandBuffers(device,&ca,&command),"allocate compute commands"))return false;
  VkFenceCreateInfo fc{VK_STRUCTURE_TYPE_FENCE_CREATE_INFO};
  if(!check(vkCreateFence(device,&fc,nullptr,&fence),"create completion fence"))return false;
  info.vendor=props.vendorID;info.device=props.deviceID;info.api=props.apiVersion;info.driver=props.driverVersion;snprintf(info.name,sizeof(info.name),"%s",props.deviceName);return true;
 }

 bool create_buffer(Buffer& b,VkDeviceSize bytes,uint64_t& used){
  if(bytes>props.limits.maxStorageBufferRange)return fail("storage range exceeded");
  VkBufferCreateInfo bi{VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO};bi.size=bytes;bi.usage=VK_BUFFER_USAGE_STORAGE_BUFFER_BIT;bi.sharingMode=VK_SHARING_MODE_EXCLUSIVE;
  if(!check(vkCreateBuffer(device,&bi,nullptr,&b.buffer),"create storage buffer"))return false;
  VkMemoryRequirements req{};vkGetBufferMemoryRequirements(device,b.buffer,&req);
  if(req.size>BUDGET || used>BUDGET-req.size)return fail("32 MiB total audio buffer budget exceeded");used+=req.size;
  uint32_t type=UINT32_MAX;int best=-1;
  for(uint32_t i=0;i<memory_props.memoryTypeCount;++i){auto f=memory_props.memoryTypes[i].propertyFlags;
   if((req.memoryTypeBits&(1u<<i)) && (f&VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT)){
    // CPU touches every input/output byte; prefer cached mappings over uncached coherence.
    int score=((f&VK_MEMORY_PROPERTY_HOST_CACHED_BIT)?4:0)+((f&VK_MEMORY_PROPERTY_HOST_COHERENT_BIT)?1:0);
    if(score>best){best=score;type=i;}
   }
  }
  if(type==UINT32_MAX)return fail("no host-visible GPU memory");
  VkMemoryAllocateInfo ma{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO};ma.allocationSize=req.size;ma.memoryTypeIndex=type;
  if(!check(vkAllocateMemory(device,&ma,nullptr,&b.memory),"allocate audio storage") || !check(vkBindBufferMemory(device,b.buffer,b.memory,0),"bind audio storage"))return false;
  b.flags=memory_props.memoryTypes[type].propertyFlags;b.allocated=req.size;
  b.coherent=(b.flags&VK_MEMORY_PROPERTY_HOST_COHERENT_BIT)!=0;
  return check(vkMapMemory(device,b.memory,0,VK_WHOLE_SIZE,0,&b.mapped),"map audio storage");
 }
 bool configure(uint32_t n,uint32_t p,uint64_t cpu_bytes,uint32_t b=0){
  if(!n || !p || p>4 || cpu_bytes>BUDGET || (b && (n%b || n/b>256)))return fail("invalid bounded audio shape");
  uint64_t groups=(uint64_t(n)+63)/64;if(groups>props.limits.maxComputeWorkGroupCount[0])return fail("compute dispatch limit exceeded");
  if(in_flight && !check(vkWaitForFences(device,1,&fence,VK_TRUE,UINT64_MAX),"drain previous submission"))return false;
  in_flight=false;plane=0;parts=0;
  if(!check(vkResetCommandBuffer(command,0),"reset compute commands"))return false;
  release(input);release(output);uint64_t used=cpu_bytes;
  if(!create_buffer(input,uint64_t(n)*6*p*sizeof(float),used) || !create_buffer(output,(uint64_t(n)*4+(b?1:0))*sizeof(float),used))return false;
  // Padded/unprepared rows start with finite zero coefficients.
  memset(input.mapped,0,uint64_t(n)*6*p*sizeof(float));memset(resident_parts,0,sizeof(resident_parts));memset(output.mapped,0,uint64_t(n)*4*sizeof(float));if(!output.coherent){VkMappedMemoryRange zero{VK_STRUCTURE_TYPE_MAPPED_MEMORY_RANGE};zero.memory=output.memory;zero.size=VK_WHOLE_SIZE;if(!check(vkFlushMappedMemoryRanges(device,1,&zero),"initialize host output mapping"))return false;}
  VkDescriptorBufferInfo buffers[2]={{input.buffer,0,uint64_t(n)*6*p*sizeof(float)},{output.buffer,0,(uint64_t(n)*4+(b?1:0))*sizeof(float)}};
  VkWriteDescriptorSet writes[2]{};
  for(uint32_t i=0;i<2;++i){writes[i].sType=VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET;writes[i].dstSet=descriptor;writes[i].dstBinding=i;writes[i].descriptorCount=1;writes[i].descriptorType=VK_DESCRIPTOR_TYPE_STORAGE_BUFFER;writes[i].pBufferInfo=&buffers[i];}
  vkUpdateDescriptorSets(device,2,writes,0,nullptr);
  VkCommandBufferBeginInfo cb{VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO};
  if(!check(vkBeginCommandBuffer(command,&cb),"begin compute commands"))return false;
  VkBufferMemoryBarrier before{VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER};before.srcAccessMask=VK_ACCESS_HOST_WRITE_BIT;before.dstAccessMask=VK_ACCESS_SHADER_READ_BIT;before.srcQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED;before.dstQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED;before.buffer=input.buffer;before.size=VK_WHOLE_SIZE;
  VkBufferMemoryBarrier initial[2]={before,before};initial[1].buffer=output.buffer;initial[1].dstAccessMask=VK_ACCESS_SHADER_READ_BIT|VK_ACCESS_SHADER_WRITE_BIT;
  vkCmdPipelineBarrier(command,VK_PIPELINE_STAGE_HOST_BIT,VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT,0,0,nullptr,b?2:1,initial,0,nullptr);
  vkCmdBindPipeline(command,VK_PIPELINE_BIND_POINT_COMPUTE,pipeline);vkCmdBindDescriptorSets(command,VK_PIPELINE_BIND_POINT_COMPUTE,pipeline_layout,0,1,&descriptor,0,nullptr);
  uint32_t shape[3]={n,p,b};vkCmdPushConstants(command,pipeline_layout,VK_SHADER_STAGE_COMPUTE_BIT,0,12,shape);vkCmdDispatch(command,static_cast<uint32_t>(groups),1,1);
  VkBufferMemoryBarrier after{VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER};after.srcAccessMask=VK_ACCESS_SHADER_WRITE_BIT;after.dstAccessMask=VK_ACCESS_HOST_READ_BIT;after.srcQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED;after.dstQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED;after.buffer=output.buffer;after.size=VK_WHOLE_SIZE;
  vkCmdPipelineBarrier(command,VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT,VK_PIPELINE_STAGE_HOST_BIT,0,0,nullptr,1,&after,0,nullptr);
  if(!check(vkEndCommandBuffer(command),"record compute commands"))return false;plane=n;parts=p;bins=b;return true;
 }
 int finish_run(int result,uint64_t wall_start,uint64_t cpu_start){
  timings.total=monotonic_ns()-wall_start;uint64_t cpu_end=thread_cpu_ns();timings.host_cpu=cpu_start&&cpu_end>=cpu_start?cpu_end-cpu_start:0;return result;
 }
 int run(const float* src,size_t inputs,float* dst,size_t outputs,uint64_t timeout,const uint8_t* dirty=nullptr,const uint8_t* active=nullptr){
  if(!plane || inputs!=uint64_t(plane)*6*parts || outputs!=uint64_t(plane)*4){fail("audio packet shape mismatch");return -1;}
  auto start=monotonic_ns(),cpu_start=thread_cpu_ns();timings={};timings.input_flags=input.flags;timings.output_flags=output.flags;timings.input_allocated=input.allocated;timings.output_allocated=output.allocated;
  auto done=[&](int result){return finish_run(result,start,cpu_start);};
  if(in_flight){fail("previous GPU work still pending");return done(-1);}
  uint8_t previous[256]{};if(bins)memcpy(previous,resident_parts,sizeof(previous));
  if(src==static_cast<const float*>(input.mapped)) {
   if(active&&bins)memcpy(resident_parts,active,plane/bins);
  }else if(dirty && bins){
   auto mapped=static_cast<float*>(input.mapped);size_t signal=size_t(plane)*2*parts;
   memcpy(previous,resident_parts,sizeof(previous));
   if(active){
    for(uint32_t row=0;row<plane/bins;row++){for(uint32_t part=0;part<parts;part++){
     size_t x=(size_t(part)*plane+size_t(row)*bins)*2;
     if(active[row]&(1u<<part))memcpy(mapped+x,src+x,size_t(bins)*2*sizeof(float));
     else if(previous[row]&(1u<<part))memset(mapped+x,0,size_t(bins)*2*sizeof(float));
    }resident_parts[row]=active[row];}
   }else memcpy(mapped,src,signal*sizeof(float));
   size_t stride=size_t(parts)*bins*4;
   for(uint32_t row=0;row<plane/bins;row++)if(dirty[row])memcpy(mapped+signal+row*stride,src+signal+row*stride,stride*sizeof(float));
  }else memcpy(input.mapped,src,inputs*sizeof(float));
  if(bins){
   static_cast<uint32_t*>(output.mapped)[size_t(plane)*4]=0;
   if(!output.coherent){VkMappedMemoryRange range{VK_STRUCTURE_TYPE_MAPPED_MEMORY_RANGE};range.memory=output.memory;range.size=VK_WHOLE_SIZE;if(!check(vkFlushMappedMemoryRanges(device,1,&range),"flush GPU validation flag"))return done(-1);}
  }
  if(!input.coherent){VkMappedMemoryRange range{VK_STRUCTURE_TYPE_MAPPED_MEMORY_RANGE};range.memory=input.memory;range.size=VK_WHOLE_SIZE;if(!check(vkFlushMappedMemoryRanges(device,1,&range),"flush audio input"))return done(-1);}
  timings.upload=monotonic_ns()-start;
  if(timings.upload>=timeout){fail("GPU upload exceeded audio deadline");return done(1);}
  uint64_t dispatch_start=monotonic_ns();
  if(!check(vkResetFences(device,1,&fence),"reset completion fence"))return done(-1);
  VkSubmitInfo submit{VK_STRUCTURE_TYPE_SUBMIT_INFO};submit.commandBufferCount=1;submit.pCommandBuffers=&command;
  if(!check(vkQueueSubmit(queue,1,&submit,fence),"submit audio compute"))return done(-1);in_flight=true;
  uint64_t spent=monotonic_ns()-start;VkResult waited=vkWaitForFences(device,1,&fence,VK_TRUE,spent>=timeout?0:timeout-spent);
  timings.dispatch_wait=monotonic_ns()-dispatch_start;
  if(waited==VK_TIMEOUT){fail("GPU completion exceeded audio deadline",waited);return done(1);}
  if(!check(waited,"wait for audio compute"))return done(-1);in_flight=false;
  uint64_t readback_start=monotonic_ns();
  if(!output.coherent){VkMappedMemoryRange range{VK_STRUCTURE_TYPE_MAPPED_MEMORY_RANGE};range.memory=output.memory;range.size=VK_WHOLE_SIZE;if(!check(vkInvalidateMappedMemoryRanges(device,1,&range),"invalidate audio output"))return done(-1);}
  if(bins && static_cast<const uint32_t*>(output.mapped)[size_t(plane)*4]!=0){fail("nonfinite GPU output");return done(-1);}
  if(dst==static_cast<float*>(output.mapped)) {
   // Invalidation above and the ownership transfer make the completed mapping
   // directly readable by the renderer. No output memcpy or CPU zeroing.
  }else if(active&&bins){auto mapped=static_cast<const float*>(output.mapped);for(uint32_t row=0;row<plane/bins;row++){
   size_t base=size_t(row)*bins*4;
   if(active[row])memcpy(dst+base,mapped+base,size_t(bins)*4*sizeof(float));
   else if(previous[row])memset(dst+base,0,size_t(bins)*4*sizeof(float));
  }}else memcpy(dst,output.mapped,outputs*sizeof(float));timings.readback=monotonic_ns()-readback_start;
  if(monotonic_ns()-start>timeout){fail("GPU readback exceeded audio deadline");return done(1);}return done(0);
 }

};
void copy_error(State* s,char* out,size_t capacity){if(out && capacity)snprintf(out,capacity,"%s",s?s->error:"Vulkan allocation failed");}
}
extern "C" {
void* sda_vk_open(const uint32_t* code,size_t bytes,SdaVkInfo* info,char* error,size_t capacity){
 void* storage=malloc(sizeof(State));State* s=storage?new(storage) State():nullptr;if(!s || !code || !bytes || bytes%4 || !s->open(code,bytes)){copy_error(s,error,capacity);if(s){s->~State();free(s);}return nullptr;}if(info)*info=s->info;return s;
}
int sda_vk_configure(void* handle,uint32_t plane,uint32_t parts,uint64_t cpu_bytes,char* error,size_t capacity){
 auto s=static_cast<State*>(handle);if(!s)return -1;bool ok=s->configure(plane,parts,cpu_bytes);if(!ok)copy_error(s,error,capacity);return ok?0:-1;
}
int sda_vk_run(void* handle,const float* src,size_t inputs,float* dst,size_t outputs,uint64_t timeout,char* error,size_t capacity){
 auto s=static_cast<State*>(handle);if(!s || !src || !dst)return -1;int result=s->run(src,inputs,dst,outputs,timeout);if(result)copy_error(s,error,capacity);return result;
}
int sda_vk_configure_resident(void* handle,uint32_t rows,uint32_t parts,uint32_t bins,uint64_t cpu,char*err,size_t cap){
 auto s=static_cast<State*>(handle);if(!s || !rows || rows>256 || bins<2 || bins>4097)return -1;
 bool ok=s->configure(rows*bins,parts,cpu,bins);if(!ok)copy_error(s,err,cap);return ok?0:-1;
}
int sda_vk_run_resident(void*handle,const float*src,size_t inputs,float*dst,size_t outputs,const uint8_t*dirty,const uint8_t*active,uint64_t timeout,char*err,size_t cap){
 auto s=static_cast<State*>(handle);if(!s || !src || !dst || !dirty || !s->bins)return -1;
 int rc=s->run(src,inputs,dst,outputs,timeout,dirty,active);if(rc)copy_error(s,err,cap);return rc;
}
int sda_vk_mapped_packet(void*handle,float**input,size_t*inputs,float**output,size_t*outputs){auto s=static_cast<State*>(handle);if(!s||!s->plane||!s->bins||!input||!inputs||!output||!outputs)return -1;*input=static_cast<float*>(s->input.mapped);*output=static_cast<float*>(s->output.mapped);*inputs=size_t(s->plane)*6*s->parts;*outputs=size_t(s->plane)*4;return 0;}
void sda_vk_timings(void* handle,Timings* out){if(handle && out)*out=static_cast<State*>(handle)->timings;}
void sda_vk_close(void* handle){if(handle){auto s=static_cast<State*>(handle);s->~State();free(s);}}
}
