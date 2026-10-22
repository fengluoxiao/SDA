#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <math.h>
struct Info{uint32_t vendor,device,api,driver,queue;char name[256];};
struct Stats{uint64_t upload,dispatch,readback,total,cpu,input_flags,output_flags,input_bytes,output_bytes;};
extern "C"{void* sda_vk_open(const uint32_t*,size_t,Info*,char*,size_t);int sda_vk_configure(void*,uint32_t,uint32_t,uint64_t,char*,size_t);int sda_vk_run(void*,const float*,size_t,float*,size_t,uint64_t,char*,size_t);void sda_vk_timings(void*,Stats*);void sda_vk_close(void*);}
uint32_t rng=0x12345678u;float next(){rng^=rng<<13;rng^=rng>>17;rng^=rng<<5;return (float(int32_t(rng>>8))-8388608.0f)/8388608.0f;}
int main(int argc,char**argv){
 if(argc!=2)return 3;FILE*f=fopen(argv[1],"rb");if(!f)return 3;fseek(f,0,SEEK_END);long bytes=ftell(f);rewind(f);if(bytes<=0||bytes>65536||bytes%4)return 3;auto code=(uint32_t*)malloc(bytes);if(fread(code,1,bytes,f)!=(size_t)bytes)return 3;fclose(f);
 Info info{};char error[256]{};void*vk=sda_vk_open(code,bytes,&info,error,sizeof(error));free(code);if(!vk){printf("{\"error\":\"%s\"}\n",error);return 4;}
 printf("{\"device\":\"%s\",\"cases\":[",info.name);bool all=true;
 const uint32_t shapes[][3]={{8,17,1},{8,17,3},{16,513,2},{32,1025,2},{32,1025,4},{64,1025,2},{64,2049,2},{128,1025,2},{256,513,2}};
 for(unsigned c=0;c<sizeof(shapes)/sizeof(shapes[0]);++c){uint32_t rows=shapes[c][0],bins=shapes[c][1],parts=shapes[c][2],n=rows*bins;size_t ni=(size_t)n*6*parts,no=(size_t)n*4;auto input=(float*)malloc(ni*4);auto output=(float*)malloc(no*4);auto ref=(float*)calloc(no,4);if(!input||!output||!ref){free(input);free(output);free(ref);sda_vk_close(vk);return 6;}float scale=c%2?0.00001f:1.0f;
  for(size_t j=0;j<ni;++j)input[j]=next()*scale;
  for(uint32_t p=0;p<parts;++p){const float*v=input+(size_t)p*6*n;for(uint32_t i=0;i<n;++i)for(uint32_t ear=0;ear<2;++ear){float hr=v[(2+2*ear)*n+i],hi=v[(3+2*ear)*n+i];ref[(2*ear)*n+i]+=v[i]*hr-v[n+i]*hi;ref[(2*ear+1)*n+i]+=v[i]*hi+v[n+i]*hr;}}
  bool ok=sda_vk_configure(vk,n,parts,((uint64_t)ni+2*no)*4,error,sizeof(error))==0;
  uint64_t elapsed=0;Stats s{};if(ok)for(int run=0;run<3;++run){if(sda_vk_run(vk,input,ni,output,no,100000000,error,sizeof(error))!=0){ok=false;break;}sda_vk_timings(vk,&s);elapsed+=s.total;}
  double energy=0,err=0,peak=0,delta=0;if(ok)for(size_t j=0;j<no;++j){double d=double(output[j])-ref[j];if(!isfinite(output[j]))ok=false;energy+=double(ref[j])*ref[j];err+=d*d;peak=fmax(peak,fabs(ref[j]));delta=fmax(delta,fabs(d));}
  bool parity=ok&&err<=energy*1e-12&&delta<=peak*4e-6;all=all&&parity;
  printf("%s{\"rows\":%u,\"bins\":%u,\"parts\":%u,\"scale\":%.7g,\"passed\":%s,\"relativeRms\":%.9g,\"relativePeak\":%.9g,\"averageWallNs\":%llu,\"lastHostCpuNs\":%llu,\"inputMemoryFlags\":%llu,\"outputMemoryFlags\":%llu}",c?",":"",rows,bins,parts,scale,parity?"true":"false",energy?sqrt(err/energy):0,peak?delta/peak:0,(unsigned long long)(elapsed/3),(unsigned long long)s.cpu,(unsigned long long)s.input_flags,(unsigned long long)s.output_flags);fflush(stdout);free(input);free(output);free(ref);
 }
 printf("],\"allPassed\":%s}\n",all?"true":"false");sda_vk_close(vk);return all?0:5;
}
