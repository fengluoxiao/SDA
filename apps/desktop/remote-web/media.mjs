export function createMediaPicker(request){
 const $=id=>document.getElementById(id),dialog=$('media-picker'),content=$('media-content'),note=$('media-note');let history=[],current=null,busy=false,revision=0;
 function button(text,action){const b=document.createElement('button');b.type='button';b.textContent=text;b.onclick=action;return b;}
 function state(value){busy=value;dialog.setAttribute('aria-busy',String(value));for(const b of dialog.querySelectorAll('button:not(#media-close)'))b.disabled=value;}
 async function add(id){if(busy)return;const version=++revision;state(true);note.textContent='正在加入主机播放列表…';try{await request('mediaOpen',id);if(version===revision)note.textContent='已加入主机播放列表';}catch(e){if(version===revision)note.textContent=e.message;}finally{if(version===revision)state(false);}}
 async function load(id=null,back=false){if(busy)return;const version=++revision;state(true);note.textContent='正在读取…';try{const data=await request('mediaList',id);if(version!==revision)return;
 if(!back&&current!==id)history.push(current);current=id;content.replaceChildren();$('media-back').hidden=!id;$('media-add').hidden=!id;$('media-title').textContent=data.name||'主机媒体';
 function rows(entries){if(!entries.length){const p=document.createElement('p');p.className='muted';p.textContent='暂无记录';content.append(p);return;}for(const entry of entries){const b=button('',()=>entry.directory?void load(entry.id):void add(entry.id));b.className='media-row';const icon=document.createElement('span');icon.className='media-kind';icon.textContent=entry.directory?'▱':'♪';icon.ariaHidden='true';const name=document.createElement('span');name.textContent=entry.name;const detail=document.createElement('small');detail.textContent=entry.directory?'打开 ›':'加入列表';b.append(icon,name,detail);content.append(b);}}
 if(id)rows(data.entries);else for(const [title,entries] of [['收藏',data.favorites],['最近打开',data.recent]]){const h=document.createElement('h3');h.textContent=title;content.append(h);rows(entries);}
 note.textContent=id?'仅显示支持的媒体文件':'只查看主机的收藏和最近目录';
 }catch(e){if(version===revision)note.textContent=e.message;}finally{if(version===revision)state(false);}}
 $('media-open').onclick=()=>{$('player-settings').dispatchEvent(new Event('menu-close'));history=[];current=null;dialog.showModal();void load();};
 $('media-close').onclick=()=>dialog.close();dialog.addEventListener('close',()=>{revision++;busy=false;});
 $('media-back').onclick=()=>void load(history.pop()??null,true);$('media-add').onclick=()=>void add(current);
 return {close:()=>dialog.close()};
}
