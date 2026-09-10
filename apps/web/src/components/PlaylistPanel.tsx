import {AudioLines, ListMusic, Pause, Play, Trash2, X} from "lucide-react";

export default function PlaylistPanel({items,currentId,paused,onPlay,onRemove,onClear,onClose}:{
  items:readonly {id:string;title:string}[];currentId:string|null;paused:boolean;
  onPlay:(id:string)=>void;onRemove:(id:string)=>void;onClear:()=>void;onClose:()=>void;
}){
  return <section className="panel float-panel playlist-panel" aria-label="播放列表">
    <header className="playlist-head">
      <div className="playlist-heading"><ListMusic size={20} aria-hidden="true"/><h2>播放列表</h2><span className="playlist-count">{items.length} 首</span></div>
      <button className="playlist-close" aria-label="关闭播放列表" onClick={onClose}><X size={17}/></button>
    </header>
    {items.length===0?<div className="playlist-empty"><ListMusic size={30}/><strong>还没有歌曲</strong><p>打开音频文件或添加文件夹，即可开始收听。</p></div>:<ol className="playlist-items">
      {items.map((item,index)=>{
        const current=item.id===currentId;
        const extension=item.title.match(/\.(wav|m4a|mp4|mkv|flac|mp3|aac|ac4|ec3|eac3|ac3|ogg|opus|aiff|aif)$/i);
        const title=extension?item.title.slice(0,-extension[0].length):item.title;
        return <li key={item.id} className={current?"current":""} aria-current={current?"true":undefined}>
          <button className="playlist-select" onClick={()=>onPlay(item.id)} aria-label={`播放 ${item.title}`}>
            <span className="playlist-index" aria-hidden="true">{current?(paused?<Pause size={17}/>:<AudioLines size={18}/>):<><span className="playlist-number">{String(index+1).padStart(2,"0")}</span><Play className="playlist-hover-play" size={16}/></>}</span>
            <span className="playlist-copy"><b title={item.title}>{title}</b><small>{current?(paused?"已暂停":"正在播放"):"待播放"}{extension?` · ${extension[1]!.toUpperCase()}`:""}</small></span>
          </button>
          <button className="playlist-remove" onClick={()=>onRemove(item.id)} aria-label={`移除 ${item.title}`}><X size={15}/></button>
        </li>;
      })}
    </ol>}
    {items.length>0&&<footer className="playlist-footer"><span>按列表顺序播放</span><button onClick={onClear} className="playlist-clear"><Trash2 size={14}/>清空列表</button></footer>}
  </section>;
}
