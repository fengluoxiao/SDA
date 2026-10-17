// Shared Windows WASM / Android native decoder preparation. Never duplicate capture patches.
import {execFileSync} from 'node:child_process';
import {readFileSync,writeFileSync,existsSync,mkdirSync,cpSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {fileURLToPath} from 'node:url';
export const root=resolve(fileURLToPath(new URL('..',import.meta.url)));
export function prepareMpegh(build) {
const upstream=join(root,'vendor/libmpegh'), pin='f7ff0ac78d4d83f0b853bf2dff2ef075c92724f8';
const run=(cmd,args)=>execFileSync(cmd,args,{cwd:root,stdio:'inherit',windowsHide:true});
if(!existsSync(join(upstream,'README.md'))){run('git',['clone','https://github.com/ittiam-systems/libmpegh.git',upstream]);run('git',['-C',upstream,'checkout',pin]);}
if(execFileSync('git',['-C',upstream,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==pin)throw Error('Unexpected libmpegh revision');
mkdirSync(build,{recursive:true});
const dec=join(build,'decoder');cpSync(join(upstream,'decoder'),dec,{recursive:true});
const types=join(dec,'impeghd_type_def.h');
// Use the host libc types on wasm32 and native 64-bit targets. Decoder maths is unchanged.
writeFileSync(types,readFileSync(types,'utf8')
  .replace(/^typedef .*\b(?:size_t|ptrdiff_t|intptr_t);.*$/gm,'')
  .replace('#define IMPEGHD_TYPE_DEF_H','#define IMPEGHD_TYPE_DEF_H\n#include <stddef.h>\n#include <stdint.h>'));

// The upstream MHAS parser reserves the entire ~1 MiB DRC payload on every
// call, including packets without loudness metadata. Only str_loud_info is used.
// Keep the exact same parser/member type, but not its unrelated config/gain data:
// iOS dispatch workers can have a 544 KiB stack (confirmed in a device crash).
let mhas=readFileSync(join(dec,'impeghd_mhas_parse.c'),'utf8').replaceAll('\r\n','\n');
const drcLocal='      ia_drc_payload_struct str_drc_payload;';
const drcMember='&str_drc_payload.str_loud_info,';
if(mhas.split(drcLocal).length!==2 || mhas.split(drcMember).length!==2)throw Error('MHAS stack patch anchor changed');
mhas=mhas.replace(drcLocal,'      ia_drc_loudness_info_set_struct str_loud_info;')
  .replace(drcMember,'&str_loud_info,');
writeFileSync(join(dec,'impeghd_mhas_parse.c'),mhas);

let source=readFileSync(join(upstream,'decoder/ia_core_coder_decode_main.c'),'utf8').replaceAll('\r\n','\n');
source=source.replace('#include <math.h>',`/* SDA capture hooks added 2026-09-14; original license retained. */
void sda_capture_pcm(int,int,int,int,int,int,float (*)[1024]);
void sda_capture_bed(int,int);
void sda_capture_object(int,int,float,float,float,float,float,float,float,float,int,int);
#include <math.h>`);
const pcmAnchor='  if (mpegh_dec_handle->p_config->extrn_rend_flag && ia_signals_3da->num_ch > 0)';
if(!source.includes(pcmAnchor))throw Error('PCM hook anchor changed');
const capture=`  { int cicp=0;
    for(int g=0;g<ia_signals_3da->num_sig_group;g++) if(ia_signals_3da->group_type[g]==0) {cicp=ia_signals_3da->audio_ch_layout[g].cicp_spk_layout_idx;break;}
    sda_capture_pcm(pstr_dec_data->str_usac_data.ccfl,
      ia_signals_3da->num_ch+ia_signals_3da->num_audio_obj+ia_signals_3da->num_hoa_transport_ch,
      ia_signals_3da->num_ch,ia_signals_3da->num_audio_obj,ia_signals_3da->num_hoa_transport_ch,cicp,
      pstr_dec_data->str_usac_data.time_sample_vector);
    int ch=0,group=0;
    for(int g=0;g<ia_signals_3da->num_sig_group;g++) if(ia_signals_3da->group_type[g]==0){
      ia_speaker_config_3d *cfg=ia_signals_3da->differs_from_ref_layout[g]?&ia_signals_3da->audio_ch_layout[group]:&pstr_asc->ref_spk_layout;
      group++;
      for(int c=0;c<ia_signals_3da->num_sig[g];c++){
        int idx=-1,type=cfg->spk_layout_type;
        int layout=cfg->cicp_spk_layout_idx;
        if(type==0 && layout>0 && layout<=20 && layout!=8 && c<impgehd_cicp_get_num_ls[layout])idx=ia_cicp_idx_ls_set_map_tbl[layout][c];
        if(type==1)idx=cfg->cicp_spk_idx[c];
        if(type==2 && cfg->str_flex_spk.str_flex_spk_descr[c].is_cicp_spk_idx)idx=cfg->str_flex_spk.str_flex_spk_descr[c].cicp_spk_idx;
        sda_capture_bed(ch++,idx);
      }
    }
  }
`;
// ASI group member IDs index decoded signals, never an output-layout CICP index.
source=source.replace('pstr_dec_data->str_frame_data.str_audio_specific_config.channel_configuration,',
  'ia_signals_3da->num_ch + ia_signals_3da->num_audio_obj + ia_signals_3da->num_hoa_transport_ch,');
// Capture after default group selection/gain processing, before object rendering.
const afterMdp='  for (ele = 0; ele < num_elements; ele++)\n  {\n    if ((ID_EXT_ELE_UNI_DRC';
if(!source.includes(afterMdp))throw Error('MDP hook anchor changed');
source=source.replace(afterMdp,capture+afterMdp);
const objAnchor='        if (mpegh_dec_handle->p_config->extrn_rend_flag)';
if(!source.includes(objAnchor))throw Error('Object hook anchor changed');
source=source.replace(objAnchor,`        { ia_oam_dec_state_struct *md=&pstr_dec_data->str_obj_ren_dec_state.str_obj_md_dec_state;
          int duration=pstr_usac_config->obj_md_cfg.frame_length;
          int total=pstr_usac_config->obj_md_cfg.cc_frame_length/duration, current=0;
          for(int f=0;f<total;f++) {
            if(total==1)current=1;
            else if(md->sub_frame_obj_md_present[f]){current++;if(current>total)current%=total;}
            else if(f==0)current=total;
            for(int ob=0;ob<md->num_objects;ob++) {int i=(current-1)*md->num_objects+ob;
              sda_capture_object(ob,f*duration,md->azimuth_descaled[i],md->elevation_descaled[i],
                md->radius_descaled[i],md->gain_descaled[i],md->spread_width_descaled[i],
                md->spread_height_descaled[i],md->spread_depth_descaled[i],
                pstr_dec_data->str_enh_obj_md_frame.diffuseness[ob],
                pstr_usac_config->obj_md_cfg.is_screen_rel_obj[ob],duration);
            }}
        }
${objAnchor}`);
writeFileSync(join(build,'ia_core_coder_decode_main.c'),source);
const cmake=readFileSync(join(upstream,'CMakeLists.txt'),'utf8');
const sources=[...cmake.slice(cmake.indexOf('add_library'),cmake.indexOf(')',cmake.indexOf('add_library'))).matchAll(/decoder\/[\w]+\.c/g)].map(m=>m[0].endsWith('/ia_core_coder_decode_main.c')?join(build,'ia_core_coder_decode_main.c'):join(build,m[0]));
return {pin,includes:dec,sources:[...sources,join(root,'packages/core/mpegh/bridge.c')]};
}
if(process.argv[1] && resolve(process.argv[1])===fileURLToPath(import.meta.url)) {
  const build=resolve(process.argv[2]);
  const manifest=prepareMpegh(build);
  writeFileSync(join(build,'sources.json'),JSON.stringify(manifest));
}
