import { memo } from "react";

/**
 * iOS 26 风格迷你播放器（macOS 布局）：通栏液态玻璃底条 —
 * 左侧封面+曲名，中间传输控制+进度条，右侧对象数+音量。
 * 液态玻璃为纯 CSS 实现：backdrop-filter 磨砂 + 内高光描边 + 斜向镜面光泽。
 */

export interface TrackInfo {
  codec: string;
  sampleRate: number;
  /** Container/header channel_count. For EC-3 JOC this can be its 2-channel core. */
  channels: number;
  container: string;
  /** Actual fixed E-AC-3/other codec channels before object reconstruction. */
  rawBedLabels?: string[];
  /** Object URL for embedded MP4/MKV cover art, when available. */
  coverUrl?: string;
  /** Decoded fixed bed labels from actual PCM frames. */
  bedLabels?: string[];
  /** Decoded dynamic object PCM channels from actual PCM frames. */
  objectChannels?: number;
  /** 歌曲标题：容器元数据（MKV Title / 音轨 Name）或文件名兜底。 */
  title?: string;
}

interface MiniPlayerProps {
  track: TrackInfo | null;
  position: number;
  /** 已解码总时长（秒）。流式解码中持续增长，读完即为全长；0 表示未知。 */
  duration: number;
  playing: boolean;
  paused: boolean;
  objectCount: number;
  volume: number;
  onTogglePlay: () => void;
  onReplay: () => void;
  onVolume: (v: number) => void;
}

function formatTime(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${String(s).padStart(2, "0")}`;
}

export const MiniPlayer = memo(function MiniPlayer({
  track,
  position,
  duration,
  playing,
  paused,
  objectCount,
  volume,
  onTogglePlay,
  onReplay,
  onVolume,
}: MiniPlayerProps) {
  if (!track) return null;
  const progress = duration > 0 ? Math.min(1, position / duration) : 0;
  return (
    <div className={`miniplayer ${window.sdaDesktop?.rendererMode === "swiftshader" ? "software-renderer" : ""}`}>
      <div className="mp-glass">
        <div className="mp-bar">
          {/* 左：封面 + 曲名 */}
          <div className="mp-left">
            <div className={`mp-art ${track.coverUrl ? "has-cover" : ""} ${playing && !paused ? "playing" : ""}`}>
              {track.coverUrl ? <img src={track.coverUrl} alt="" /> : <><span /><span /><span /><span /></>}
            </div>
            <div className="mp-meta">
              <div className="mp-title">{track.title ?? track.codec}</div>
              <div className="mp-sub">
                {track.codec} · {(track.sampleRate / 1000).toFixed(1)} kHz · {track.rawBedLabels?.length ? `${track.rawBedLabels.length} 原始声道` : "等待首帧"} · {track.objectChannels ? `${track.objectChannels} 对象` : "无对象"} · {track.container}
              </div>
            </div>
          </div>

          {/* 中：传输控制 + 进度 */}
          <div className="mp-center">
            <div className="mp-transport">
              <button className="mp-btn" onClick={onReplay} title="从头重新播放">
                ⟲
              </button>
              <button
                className="mp-btn mp-play"
                onClick={onTogglePlay}
                title={playing && !paused ? "暂停" : paused ? "继续" : "播放"}
              >
                {playing && !paused ? "❚❚" : "▶"}
              </button>
            </div>
            <div className="mp-progress">
              <span className="mp-time">{formatTime(position)}</span>
              <div className="mp-track-line">
                <div className="mp-track-fill" style={{ transform: `scaleX(${progress})` }} />
                {duration <= 0 && <div className="mp-track-shimmer" />}
              </div>
              <span className="mp-time dim">{duration > 0 ? formatTime(duration) : "--:--"}</span>
            </div>
          </div>

          {/* 右：对象数 + 音量 */}
          <div className="mp-right">
            <span className="mp-objs">{objectCount} 对象</span>
            <div className="mp-vol" title="音量">
              <span className="mp-vol-icon">🔊</span>
              <input
                type="range"
                min={0}
                max={100}
                value={Math.round(volume * 100)}
                onChange={(e) => onVolume(Number(e.target.value) / 100)}
              />
            </div>
          </div>
        </div>
      </div>
    </div>
  );
});
