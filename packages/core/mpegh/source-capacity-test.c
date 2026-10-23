/* Synthetic capture test; no user audio. */
#include <assert.h>
#include <math.h>
#include <stdio.h>
#include "impeghd_api.h"
IA_ERRORCODE ia_mpegh_dec_create(pVOID a,pVOID b){(void)a;(void)b;return 0;}
IA_ERRORCODE ia_mpegh_dec_delete(pVOID a){(void)a;return 0;}
IA_ERRORCODE ia_mpegh_dec_init(pVOID a,pVOID b,pVOID c){(void)a;(void)b;(void)c;return 0;}
IA_ERRORCODE ia_mpegh_dec_execute(pVOID a,pVOID b,pVOID c){(void)a;(void)b;(void)c;return 0;}
extern void sda_capture_pcm(int,int,int,int,int,int,float (*)[1024]);
extern void sda_capture_object(int,int,float,float,float,float,float,float,float,float,int,int);
extern int sda_info(int);
extern void sda_capture_reference(int,int,float **);
extern void *sda_reference(void);
extern void *sda_pcm(void);
extern void *sda_metadata(void);
static float tagged[128][1024];
int main(void){
 for(int c=0;c<128;c++)for(int s=0;s<16;s++)tagged[c][s]=32768.0f*((c+1)/256.0f+s/4096.0f);
 const int counts[]={24,64,128};
 for(int i=0;i<3;i++){
  int count=counts[i];sda_capture_pcm(16,count,0,count,0,0,tagged);
  assert(sda_info(3)==16&&sda_info(4)==count&&sda_info(5)==count&&sda_info(12)==0);
  float *pcm=sda_pcm();
  for(int c=0;c<count;c++)for(int s=0;s<16;s++)assert(pcm[c*16+s]==(c+1)/256.0f+s/4096.0f);
  for(int c=0;c<count;c++)for(int s=0;s<16;s++)sda_capture_object(c,s,0,0,1,1,0,0,0,0,0,1);
  assert(sda_info(9)==count*16&&sda_info(12)==0);
  float *md=sda_metadata();assert(md[(count*16-1)*12]==count-1);assert(md[(count*16-1)*12+1]==15);
 }
 sda_capture_object(0,0,0,0,1,1,0,0,0,0,0,1);assert(sda_info(12)==2&&sda_info(9)==128*16);
 sda_capture_pcm(16,129,0,129,0,0,tagged);assert(sda_info(3)==0&&sda_info(12)==1);
 sda_capture_pcm(1025,128,0,128,0,0,tagged);assert(sda_info(3)==0&&sda_info(12)==1);
 sda_capture_pcm(16,128,0,128,0,0,tagged);sda_capture_object(128,0,0,0,1,1,0,0,0,0,0,1);assert(sda_info(12)==3);
 sda_capture_pcm(16,128,0,128,0,0,tagged);assert(sda_info(12)==0&&sda_info(9)==0);
 float ref_left[]={65536.0f,-98304.0f,32768.0f},ref_right[]={-131072.0f,16384.0f,0.0f};
 float *reference[]={ref_left,ref_right};sda_capture_reference(3,2,reference);
 assert(sda_info(13)==3&&sda_info(14)==2);
 float *master=sda_reference();assert(master[0]==2.0f&&master[1]==-4.0f&&master[2]==-3.0f&&master[3]==0.5f);
 sda_capture_reference(3,12,reference);assert(sda_info(13)==0&&sda_info(14)==0);
 puts("{\"sourceLimit\":128,\"testedLanes\":[24,64,128],\"pcmAndMetadataPreserved\":true,\"overflowRejected\":true,\"resetVerified\":true}");
 return 0;
}
