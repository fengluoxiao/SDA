#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <math.h>
struct Info{uint32_t vendor,device,api,driver,queue;char name[256];};
struct Stats{uint64_t upload,dispatch,readback,total,cpu,input_flags,output_flags,input_bytes,output_bytes;};
extern "C"{void* sda_vk_open(const uint32_t*,size_t,Info*,char*,size_t);int sda_vk_configure(void*,uint32_t,uint32_t,uint64_t,char*,size_t);int sda_vk_run(void*,const float*,size_t,float*,size_t,uint64_t,char*,size_t);int sda_vk_configure_resident(void*,uint32_t,uint32_t,uint32_t,uint64_t,char*,size_t);int sda_vk_run_resident(void*,const float*,size_t,float*,size_t,const uint8_t*,const uint8_t*,uint64_t,char*,size_t);void sda_vk_timings(void*,Stats*);void sda_vk_close(void*);int sda_vk_mapped_packet(void*,float**,size_t*,float**,size_t*);}
uint32_t rng=0x12345678u;float next(){rng^=rng<<13;rng^=rng>>17;rng^=rng<<5;return (float(int32_t(rng>>8))-8388608.0f)/8388608.0f;}
int main(int argc,char**argv){
 if(argc!=2)return 3;FILE*f=fopen(argv[1],"rb");if(!f)return 3;fseek(f,0,SEEK_END);long bytes=ftell(f);rewind(f);if(bytes<=0||bytes>65536||bytes%4)return 3;auto code=(uint32_t*)malloc(bytes);if(fread(code,1,bytes,f)!=(size_t)bytes)return 3;fclose(f);
 Info info{};char error[256]{};void*vk=sda_vk_open(code,bytes,&info,error,sizeof(error));free(code);if(!vk){printf("{\"error\":\"%s\"}\n",error);return 4;}
 printf("{\"device\":\"%s\",\"cases\":[",info.name);bool all=true;bool nonfinite_rejected=false;
 const uint32_t shapes[][3]={{8,17,1},{8,17,3},{16,513,2},{32,1025,2},{32,1025,4},{64,1025,2},{64,2049,2},{128,1025,2},{256,513,2},{256,1025,2}};
 for(unsigned c=0;c<sizeof(shapes)/sizeof(shapes[0]);++c){uint32_t rows=shapes[c][0],bins=shapes[c][1],parts=shapes[c][2],n=rows*bins;size_t ni=(size_t)n*6*parts,no=(size_t)n*4;float*input=nullptr;float*output=nullptr;auto ref=(float*)calloc(no,4);if(!ref){sda_vk_close(vk);return 6;}float scale=c%2?0.00001f:1.0f;

  auto reference=[&](){for(size_t j=0;j<no;++j)ref[j]=0;for(uint32_t part=0;part<parts;part++)for(uint32_t row=0;row<rows;row++)for(uint32_t bin=0;bin<bins;bin++){uint32_t i=row*bins+bin;size_t x=(size_t(part)*n+i)*2,h=size_t(2)*n*parts+((size_t(row)*parts+part)*bins+bin)*4;for(uint32_t ear=0;ear<2;ear++){float hr=input[h+ear*2],hi=input[h+ear*2+1];ref[size_t(row)*bins*4+ear*bins*2+bin*2]+=input[x]*hr-input[x+1]*hi;ref[size_t(row)*bins*4+ear*bins*2+bin*2+1]+=input[x]*hi+input[x+1]*hr;}}};
  bool ok=sda_vk_configure_resident(vk,rows,parts,bins,uint64_t(no)*4+rows*32,error,sizeof(error))==0;
  auto active=(uint8_t*)calloc(rows,1);auto dirty=(uint8_t*)calloc(rows,1);for(uint32_t row=0;row<rows;row++)dirty[row]=1;
  float* mapped_input=nullptr;float* mapped_output=nullptr;size_t mapped_inputs=0,mapped_outputs=0;
  ok=ok&&sda_vk_mapped_packet(vk,&mapped_input,&mapped_inputs,&mapped_output,&mapped_outputs)==0&&mapped_inputs==ni&&mapped_outputs==no;
  if(ok){input=mapped_input;output=mapped_output;for(size_t j=0;j<ni;j++)input[j]=next()*scale;}
  uint64_t elapsed=0;Stats s{};if(ok)for(int run=0;run<5;++run){
    for(uint32_t row=0;row<rows;row++)dirty[row]=(run==0 || (run==3&&row==0))?1:0;
    for(uint32_t row=0;row<rows;row++){
      active[row]=(run==2&&(row%2))?0:uint8_t((1u<<parts)-1);
      if(run==4&&row==0)active[row]&=uint8_t(~(1u<<(parts-1)));
      for(uint32_t part=0;part<parts;part++)for(uint32_t bin=0;bin<bins;bin++){
        size_t x=(size_t(part)*n+row*bins+bin)*2;
        input[x]=(active[row]&(1u<<part))?next()*scale:0.0f;input[x+1]=(active[row]&(1u<<part))?next()*scale:0.0f;
      }
    }
    if(run==3)for(size_t j=size_t(2)*n*parts;j<size_t(2)*n*parts+size_t(parts)*bins*4;j++)input[j]*=-0.6f;
    reference();
    if(sda_vk_run_resident(vk,mapped_input,ni,mapped_output,no,dirty,active,100000000,error,sizeof(error))!=0){ok=false;break;}
    double en=0,er=0,pk=0,de=0;for(size_t j=0;j<no;j++){double diff=double(output[j])-ref[j];en+=double(ref[j])*ref[j];er+=diff*diff;pk=fmax(pk,fabs(ref[j]));de=fmax(de,fabs(diff));}if(er>en*1e-12||de>pk*4e-6){ok=false;break;}
    sda_vk_timings(vk,&s);elapsed+=s.total;
  }
  if(ok&&c+1==sizeof(shapes)/sizeof(shapes[0])){float original=mapped_input[0];mapped_input[0]=NAN;
   nonfinite_rejected=sda_vk_run_resident(vk,mapped_input,ni,mapped_output,no,dirty,active,100000000,error,sizeof(error))!=0;
   mapped_input[0]=original;ok=nonfinite_rejected&&sda_vk_run_resident(vk,mapped_input,ni,mapped_output,no,dirty,active,100000000,error,sizeof(error))==0;
  }free(active);free(dirty);
  double energy=0,err=0,peak=0,delta=0;if(ok)for(size_t j=0;j<no;++j){double d=double(output[j])-ref[j];if(!isfinite(output[j]))ok=false;energy+=double(ref[j])*ref[j];err+=d*d;peak=fmax(peak,fabs(ref[j]));delta=fmax(delta,fabs(d));}
  bool parity=ok&&err<=energy*1e-12&&delta<=peak*4e-6;all=all&&parity;
  printf("%s{\"rows\":%u,\"bins\":%u,\"parts\":%u,\"scale\":%.7g,\"passed\":%s,\"relativeRms\":%.9g,\"relativePeak\":%.9g,\"averageWallNs\":%llu,\"lastHostCpuNs\":%llu,\"inputMemoryFlags\":%llu,\"outputMemoryFlags\":%llu}",c?",":"",rows,bins,parts,scale,parity?"true":"false",energy?sqrt(err/energy):0,peak?delta/peak:0,(unsigned long long)(elapsed/5),(unsigned long long)s.cpu,(unsigned long long)s.input_flags,(unsigned long long)s.output_flags);fflush(stdout);free(ref);
 }
 bool over_budget=sda_vk_configure_resident(vk,256,4,1025,uint64_t(256)*1025*16+256*32,error,sizeof(error))!=0;all=all&&over_budget;
 printf("],\"allPassed\":%s,\"mappedPackets\":true,\"nonfiniteRejectionVerified\":%s,\"overBudgetRejected\":%s}\n",all?"true":"false",nonfinite_rejected?"true":"false",over_budget?"true":"false");sda_vk_close(vk);return all?0:5;
}
