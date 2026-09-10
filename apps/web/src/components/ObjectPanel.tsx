import { memo } from "react";
import { AudioLines, Headphones, Orbit, Volume2, VolumeX } from "lucide-react";
import type { BinauralRenderMetadata, VisualObject } from "@sda/player";

interface ObjectPanelProps {
  onClose?:()=>void;
  objects: readonly VisualObject[];
  mutedIds: ReadonlySet<number>;
  soloIds: ReadonlySet<number>;
  /** Worklet-confirmed post-gain/post-mute object signal IDs. */
  soundingIds: ReadonlySet<number>;
  binauralMetadata: BinauralRenderMetadata | null;
  onToggleMute: (id: number) => void;
  /** clearAll = Ctrl/Cmd+点击：取消全部独奏。 */
  onToggleSolo: (id: number, clearAll: boolean) => void;
  className?: string;
}

const formatDistance = (object: VisualObject): string | null => {
  if (object.distanceInfinite) return "∞";
  if (object.distanceM !== null) return `${object.distanceM.toFixed(1)}m`;
  return null;
};

export const ObjectPanel = memo(function ObjectPanel({
  objects,
  mutedIds,
  soloIds,
  soundingIds,
  binauralMetadata,
  onToggleMute,
  onToggleSolo,
  className,
  onClose,
}: ObjectPanelProps) {
  return (
    <div className={`panel obj-panel object-browser${className ? ` ${className}` : ""}`} aria-label="音频对象">
      <div className="obj-head">
        <h2><Orbit size={19} aria-hidden="true"/>音频对象 <span className="obj-count">{objects.length}</span></h2>
        {soloIds.size > 0 && (
          <button
            className="obj-clear-solo"
            title="取消全部独听（也可 Ctrl/Cmd+点击独听按钮）"
            onClick={() => onToggleSolo(-1, true)}
          >
            取消独听 · {soloIds.size}
          </button>
        )}
      </div>
      <div className="object-legend"><span>对象与空间位置</span><span><i/>有声信号</span></div>
      {objects.length===0&&<div className="object-empty"><Orbit size={28}/><p>播放带对象的音频后，在这里查看和控制。</p></div>}
      <ul className="objects">
        {objects.map((object) => {
          const muted = mutedIds.has(object.id);
          const soloed = soloIds.has(object.id);
          const silenced = soloIds.size > 0 ? !soloed || muted : muted;
          const sounding = !silenced && soundingIds.has(object.id);
          const distance = formatDistance(object);
          return (
            <li
              key={object.id}
              className={`obj-row${soloed ? " obj-solo" : ""}${silenced ? " obj-muted" : ""}${sounding ? " obj-sounding" : ""}`}
            >
              <span className="object-mark" aria-hidden="true">{sounding?<AudioLines size={18}/>:<Orbit size={18}/>}</span>
              <span className="obj-info">
                <span className="object-title"><b>对象 {object.id}</b><span>{silenced?"已静音":soloed?"独听":sounding?"有声":"待声"}</span></span>
                <span className="obj-pos">{object.hasPos?object.pos.map((v,i)=><span key={i}><em>{["X","Y","Z"][i]}</em>{v.toFixed(2)}</span>):"位置未提供"}</span>
                <span className="obj-sub">
                  {{room:"房间定位",screen:"屏幕定位",speaker:"音箱定位"}[object.anchor]}
                  {distance !== null && ` · ${distance}`}
                  {object.gainDb !== 0 && ` · ${object.gainDb > 0 ? "+" : ""}${object.gainDb}dB`}
                </span>
              </span>
              <span className="obj-ms">
                <button
                  className={`obj-ms-btn${muted ? " m-on" : ""}`}
                  title={muted ? "取消静音" : "静音此对象"}
                  aria-label={`${muted?"取消静音":"静音"}对象 ${object.id}`}
                  aria-pressed={muted}
                  onClick={() => onToggleMute(object.id)}
                >
                  {muted?<VolumeX size={16}/>:<Volume2 size={16}/>}
                </button>
                <button
                  className={`obj-ms-btn${soloed ? " s-on" : ""}`}
                  title={soloed ? "取消独听（Ctrl/Cmd+点击取消全部）" : "独听此对象（Ctrl/Cmd+点击取消全部）"}
                  aria-label={`独听对象 ${object.id}`}
                  aria-pressed={soloed}
                  onClick={(event) => onToggleSolo(object.id, event.ctrlKey || event.metaKey)}
                >
                  <Headphones size={16}/>
                </button>
              </span>
            </li>
          );
        })}
      </ul>
      <details className="object-metadata"><summary>双耳渲染信息</summary><p>{binauralMetadata?.available?"已读取双耳元数据，但缺少声道与对象的对应关系，暂不能逐对象显示渲染模式。":"当前音频未提供可读取的双耳渲染模式元数据。"}</p></details>
    </div>
  );
});
