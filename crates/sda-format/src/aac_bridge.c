#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include "neaacdec.h"

void *sda_aac_open(const unsigned char *asc,size_t length,uint32_t *rate,uint32_t *channels){
 if(!asc||length<2||length>64)return NULL;
 NeAACDecHandle handle=NeAACDecOpen();if(!handle)return NULL;
 NeAACDecConfigurationPtr config=NeAACDecGetCurrentConfiguration(handle);
 config->outputFormat=FAAD_FMT_FLOAT;config->downMatrix=0;config->defObjectType=LC;
 if(!NeAACDecSetConfiguration(handle,config)){NeAACDecClose(handle);return NULL;}
 unsigned long sample_rate=0;unsigned char count=0;
 if(NeAACDecInit2(handle,(unsigned char*)asc,(unsigned long)length,&sample_rate,&count)!=0||!sample_rate||!count||count>8){NeAACDecClose(handle);return NULL;}
 *rate=(uint32_t)sample_rate;*channels=count;return handle;
}
int sda_aac_decode(void *handle,const unsigned char *packet,size_t length,const float **pcm,uint32_t *frames,uint32_t *channels,uint32_t *rate,unsigned char *positions){
 if(!handle||!packet||!length||length>1048576)return -1;
 NeAACDecFrameInfo info;memset(&info,0,sizeof(info));
 void *decoded=NeAACDecDecode(handle,&info,(unsigned char*)packet,(unsigned long)length);
 if(info.error)return (int)info.error;
 if(!info.channels){*frames=0;return 0;}
 if(info.channels>8||info.samples%info.channels||info.samples>8*2048)return -2;
 if(info.samples&&!decoded)return -3;
 *pcm=(const float*)decoded;*frames=(uint32_t)(info.samples/info.channels);*channels=info.channels;*rate=(uint32_t)info.samplerate;
 memcpy(positions,info.channel_position,info.channels);return 0;
}
void sda_aac_close(void *handle){if(handle)NeAACDecClose(handle);}
