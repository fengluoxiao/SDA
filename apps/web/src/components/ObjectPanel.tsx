import { memo } from "react";
import type { BinauralRenderMetadata, VisualObject } from "@sda/player";

interface ObjectPanelProps {
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

const formatPos = (object: VisualObject): string =>
  object.hasPos ? object.pos.map((value) => value.toFixed(2)).join(", ") : "—";

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
}: ObjectPanelProps) {
  return (
    <div className={`panel obj-panel${className ? ` ${className}` : ""}`}>
      <div className="obj-head">
        <h2>对象 <span className="obj-count">{objects.length}</span></h2>
        {soloIds.size > 0 && (
          <button
            className="obj-clear-solo"
            title="取消全部独奏（也可 Ctrl+点击任意 S）"
            onClick={() => onToggleSolo(-1, true)}
          >
            取消独奏 ×{soloIds.size}
          </button>
        )}
      </div>
      <p className="obj-note">
        {binauralMetadata?.available
          ? "DBMD ordinal 缺少 bed/object 元素映射"
          : "当前输入未携带可读取的 Binaural Render Mode"}
      </p>
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
              <span className="obj-id">#{object.id}</span>
              <span className="obj-info">
                <span className="obj-pos">({formatPos(object)})</span>
                <span className="obj-sub">
                  {object.anchor}
                  {distance !== null && ` · ${distance}`}
                  {object.gainDb !== 0 && ` · ${object.gainDb > 0 ? "+" : ""}${object.gainDb}dB`}
                </span>
              </span>
              <span className="obj-ms">
                <button
                  className={`obj-ms-btn${muted ? " m-on" : ""}`}
                  title={muted ? "取消静音" : "静音此对象"}
                  onClick={() => onToggleMute(object.id)}
                >
                  M
                </button>
                <button
                  className={`obj-ms-btn${soloed ? " s-on" : ""}`}
                  title={soloed ? "取消此对象独奏（Ctrl+点击取消全部独奏）" : "独奏此对象（Ctrl+点击取消全部独奏）"}
                  onClick={(event) => onToggleSolo(object.id, event.ctrlKey || event.metaKey)}
                >
                  S
                </button>
              </span>
            </li>
          );
        })}
      </ul>
    </div>
  );
});
