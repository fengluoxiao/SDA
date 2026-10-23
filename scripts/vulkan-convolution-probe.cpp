// Experimental full-chain probe, not included in the shipped renderer.
#define sda_vk_open probe_spectral_open
#define sda_vk_configure probe_spectral_configure
#define sda_vk_run probe_spectral_run
#define sda_vk_timings probe_spectral_timings
#define sda_vk_close probe_spectral_close
#define sda_vk_configure_resident probe_spectral_configure_resident
#define sda_vk_run_resident probe_spectral_run_resident
#include "../apps/native-renderer/src/vulkan_spectral.cpp"
#undef sda_vk_open
#undef sda_vk_configure
#undef sda_vk_run
#undef sda_vk_timings
#undef sda_vk_close
#undef sda_vk_configure_resident
#undef sda_vk_run_resident
struct Chain : State {
 PFN_vkGetFenceStatus status=nullptr;uint64_t poll_ns=0;
 uint32_t rows=0;size_t ni=0,no=0,pcm_start=0,filter_start=0;bool initialized=false;
 bool configure_chain(uint32_t r,uint32_t p,uint64_t cpu_bytes){
  if(!r || r>256 || !p || p>4 || cpu_bytes>BUDGET || props.limits.maxComputeSharedMemorySize<16384)return fail("unsupported convolution shape/shared memory");
  rows=0;ni=r+4096+(size_t)r*p*(2048+1025*4);no=(size_t)r*2048;pcm_start=r+4096;filter_start=pcm_start+(size_t)r*p*2048;
  if(in_flight)return fail("previous GPU work pending");
  if(!check(vkResetCommandBuffer(command,0),"reset chain commands"))return false;
  release(input);release(output);uint64_t used=cpu_bytes;
  if(!create_buffer(input,ni*4,used)||!create_buffer(output,no*4,used))return false;
  VkDescriptorBufferInfo buffers[2]={{input.buffer,0,ni*4},{output.buffer,0,no*4}};VkWriteDescriptorSet writes[2]{};
  for(uint32_t i=0;i<2;i++){writes[i].sType=VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET;writes[i].dstSet=descriptor;writes[i].dstBinding=i;writes[i].descriptorCount=1;writes[i].descriptorType=VK_DESCRIPTOR_TYPE_STORAGE_BUFFER;writes[i].pBufferInfo=&buffers[i];}
  vkUpdateDescriptorSets(device,2,writes,0,nullptr);
  VkCommandBufferBeginInfo cb{VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO};if(!check(vkBeginCommandBuffer(command,&cb),"begin chain commands"))return false;
  VkBufferMemoryBarrier before{VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER};before.srcAccessMask=VK_ACCESS_HOST_WRITE_BIT;before.dstAccessMask=VK_ACCESS_SHADER_READ_BIT;before.srcQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED;before.dstQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED;before.buffer=input.buffer;before.size=VK_WHOLE_SIZE;
  vkCmdPipelineBarrier(command,VK_PIPELINE_STAGE_HOST_BIT,VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT,0,0,nullptr,1,&before,0,nullptr);
  vkCmdBindPipeline(command,VK_PIPELINE_BIND_POINT_COMPUTE,pipeline);vkCmdBindDescriptorSets(command,VK_PIPELINE_BIND_POINT_COMPUTE,pipeline_layout,0,1,&descriptor,0,nullptr);
  uint32_t dims[2]={r,p};vkCmdPushConstants(command,pipeline_layout,VK_SHADER_STAGE_COMPUTE_BIT,0,8,dims);vkCmdDispatch(command,r,1,1);
  VkBufferMemoryBarrier after{VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER};after.srcAccessMask=VK_ACCESS_SHADER_WRITE_BIT;after.dstAccessMask=VK_ACCESS_HOST_READ_BIT;after.srcQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED;after.dstQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED;after.buffer=output.buffer;after.size=VK_WHOLE_SIZE;
  vkCmdPipelineBarrier(command,VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT,VK_PIPELINE_STAGE_HOST_BIT,0,0,nullptr,1,&after,0,nullptr);
  if(!check(vkEndCommandBuffer(command),"end chain commands"))return false;rows=r;parts=p;initialized=false;return true;
 }
 int execute_chain(const float*src,float*dst,bool filters_changed,uint64_t timeout){
  auto start=monotonic_ns(),cpu=thread_cpu_ns();timings={};timings.input_flags=input.flags;timings.output_flags=output.flags;timings.input_allocated=input.allocated;timings.output_allocated=output.allocated;
  auto done=[&](int rc){return finish_run(rc,start,cpu);};
  if(!rows || in_flight){fail("invalid/pending chain packet");return done(-1);}
  auto mapped=static_cast<float*>(input.mapped);
  if(!initialized)memcpy(mapped,src,ni*4);
  else {memcpy(mapped,src,rows*4);memcpy(mapped+pcm_start,src+pcm_start,(filter_start-pcm_start)*4);if(filters_changed)memcpy(mapped+filter_start,src+filter_start,(ni-filter_start)*4);}
  if(!input.coherent){VkMappedMemoryRange range{VK_STRUCTURE_TYPE_MAPPED_MEMORY_RANGE};range.memory=input.memory;range.size=VK_WHOLE_SIZE;if(!check(vkFlushMappedMemoryRanges(device,1,&range),"flush chain input"))return done(-1);}
  initialized=true;timings.upload=monotonic_ns()-start;if(timings.upload>=timeout){fail("chain upload deadline");return done(1);}
  auto dispatch_start=monotonic_ns();if(!check(vkResetFences(device,1,&fence),"reset chain fence"))return done(-1);
  VkSubmitInfo submit{VK_STRUCTURE_TYPE_SUBMIT_INFO};submit.commandBufferCount=1;submit.pCommandBuffers=&command;
  if(!check(vkQueueSubmit(queue,1,&submit,fence),"submit convolution chain"))return done(-1);in_flight=true;
  auto spent=monotonic_ns()-start;VkResult waited=VK_NOT_READY;
  if(status && poll_ns){auto until=monotonic_ns()+poll_ns;do{waited=status(device,fence);if(waited!=VK_NOT_READY)break;}while(monotonic_ns()<until && monotonic_ns()-start<timeout);}
  if(waited==VK_NOT_READY){spent=monotonic_ns()-start;waited=vkWaitForFences(device,1,&fence,VK_TRUE,spent>=timeout?0:timeout-spent);}timings.dispatch_wait=monotonic_ns()-dispatch_start;
  if(waited==VK_TIMEOUT){fail("chain completion deadline",waited);return done(1);}if(!check(waited,"wait convolution chain"))return done(-1);in_flight=false;
  auto read=monotonic_ns();if(!output.coherent){VkMappedMemoryRange range{VK_STRUCTURE_TYPE_MAPPED_MEMORY_RANGE};range.memory=output.memory;range.size=VK_WHOLE_SIZE;if(!check(vkInvalidateMappedMemoryRanges(device,1,&range),"invalidate chain output"))return done(-1);}
  memcpy(dst,output.mapped,no*4);timings.readback=monotonic_ns()-read;if(monotonic_ns()-start>timeout){fail("chain readback deadline");return done(1);}return done(0);
 }
};
extern "C" {
void* sda_chain_open(const uint32_t*code,size_t bytes,SdaVkInfo*info,char*err,size_t cap){void*mem=malloc(sizeof(Chain));auto s=mem?new(mem)Chain():nullptr;if(!s||!code||!bytes||bytes%4||!s->open(code,bytes)){copy_error(s,err,cap);if(s){s->~Chain();free(s);}return nullptr;}if(info)*info=s->info;return s;}
void sda_chain_set_poll(void*ptr,uint64_t ns){auto s=static_cast<Chain*>(ptr);if(!s)return;auto get=reinterpret_cast<PFN_vkGetDeviceProcAddr>(s->get_instance(s->instance,"vkGetDeviceProcAddr"));s->status=get?reinterpret_cast<PFN_vkGetFenceStatus>(get(s->device,"vkGetFenceStatus")):nullptr;s->poll_ns=ns>200000?200000:ns;}
int sda_chain_configure(void*ptr,uint32_t rows,uint32_t parts,uint64_t cpu,char*err,size_t cap){auto s=static_cast<Chain*>(ptr);if(!s)return -1;bool ok=s->configure_chain(rows,parts,cpu);if(!ok)copy_error(s,err,cap);return ok?0:-1;}
int sda_chain_run(void*ptr,const float*input,float*output,int filters_changed,uint64_t timeout,char*err,size_t cap){auto s=static_cast<Chain*>(ptr);if(!s||!input||!output)return -1;int rc=s->execute_chain(input,output,filters_changed!=0,timeout);if(rc)copy_error(s,err,cap);return rc;}
void sda_chain_stats(void*ptr,Timings*out){if(ptr&&out)*out=static_cast<Chain*>(ptr)->timings;}
void sda_chain_close(void*ptr){if(ptr){auto s=static_cast<Chain*>(ptr);s->~Chain();free(s);}}
}
