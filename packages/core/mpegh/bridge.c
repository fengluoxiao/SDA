/* SDA MPEG-H bridge, 2026. Upstream decoder is unmodified outside the build hooks.
 * One instance per dedicated worker. No rendered speaker channels are labelled as objects. */
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include "impeghd_api.h"
#include "impeghd_memory_standards.h"
#define LIMIT 64
#define SAMPLES 4096
static ia_input_config in;
static ia_output_config out;
static float pcm[LIMIT * SAMPLES], metadata[LIMIT * 16 * 12];
static int frames, channels, objects, beds, hoa, layout, rows, created;
static int bed_indices[LIMIT];
void sda_capture_bed(int ch,int cicp){if(ch>=0&&ch<LIMIT)bed_indices[ch]=cicp;}
int sda_bed(int ch){return ch>=0&&ch<LIMIT?bed_indices[ch]:-1;}
static void *alloc_mem(UWORD32 n, UWORD32 a) { return calloc(1, n + a); }
void sda_capture_pcm(int n, int ch, int bed, int obj, int ho, int cicp, float src[][1024]) {
  frames=n; channels=ch; beds=bed; objects=obj; hoa=ho; layout=cicp; rows=0;
  for(int c=0;c<LIMIT;c++)bed_indices[c]=-1;
  if(n>SAMPLES || ch>LIMIT) {frames=0;return;}
  for(int c=0;c<ch;c++) for(int s=0;s<n;s++) pcm[c*n+s]=src[c][s]/32768.0f;
}
void sda_capture_object(int id,int offset,float az,float el,float radius,float gain,
                        float w,float h,float d,float diffuse,int screen,int duration) {
  if(rows>=LIMIT*16)return;
  float *p=metadata+12*rows++;
  p[0]=id;p[1]=offset;p[2]=az;p[3]=el;p[4]=radius;p[5]=gain;
  p[6]=w;p[7]=h;p[8]=d;p[9]=diffuse;p[10]=screen;p[11]=duration;
}
void sda_close(void) {if(created)ia_mpegh_dec_delete(&out);created=0;memset(&in,0,sizeof(in));memset(&out,0,sizeof(out));frames=rows=0;}
int sda_open_layout(int raw, int layout) {
  sda_close();in.ui_mhas_flag=1;in.ui_raw_flag=raw;in.ui_pcm_wd_sz=24;
  in.ui_cicp_layout_idx=layout;in.i_preset_id=-1;
  out.malloc_mpegh=alloc_mem;out.free_mpegh=free;
  int err=ia_mpegh_dec_create(&in,&out);created=1;return err;
}
int sda_open(int raw) { return sda_open_layout(raw, 2); }
void *sda_input(void){return out.mem_info_table[2].mem_ptr;}
int sda_capacity(void){return out.ui_inp_buf_size;}
int sda_decode(int n){
  frames=rows=0;in.num_inp_bytes=n;out.num_out_bytes=0;out.i_bytes_consumed=0;
  int err=out.ui_init_done?ia_mpegh_dec_execute(out.pv_ia_process_api_obj,&in,&out):ia_mpegh_dec_init(out.pv_ia_process_api_obj,&in,&out);
  if(err||!out.num_out_bytes)frames=rows=0;
  return err;
}
int sda_info(int key){switch(key){case 0:return out.i_bytes_consumed;case 1:return out.ui_init_done;case 2:return out.i_samp_freq;case 3:return frames;case 4:return channels;case 5:return objects;case 6:return beds;case 7:return hoa;case 8:return layout;case 9:return rows;case 10:return out.num_out_bytes;case 11:return out.i_num_chan;default:return 0;}}
void *sda_pcm(void){return pcm;}
void *sda_metadata(void){return metadata;}
void *sda_rendered(void){return out.mem_info_table[3].mem_ptr;}
