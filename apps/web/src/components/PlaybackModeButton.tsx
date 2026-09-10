import { ListOrdered, Repeat, Repeat1 } from "lucide-react";
import { followingPlaybackMode, PLAYBACK_MODE_LABELS, type PlaybackMode } from "../playbackOrder";

export default function PlaybackModeButton({mode, onChange, className = ""}: {
  mode: PlaybackMode; onChange: (mode: PlaybackMode) => void; className?: string;
}) {
  const Icon = mode === "repeat-one" ? Repeat1 : mode === "repeat-all" ? Repeat : ListOrdered;
  const next = followingPlaybackMode(mode);
  return <button type="button" className={`playback-mode-button ${className}`}
    aria-label={`播放模式：${PLAYBACK_MODE_LABELS[mode]}`} aria-pressed={mode !== "sequence"}
    title={`${PLAYBACK_MODE_LABELS[mode]} · 点击切换为${PLAYBACK_MODE_LABELS[next]}`}
    onClick={() => onChange(next)}>
    <Icon size={17} strokeWidth={1.8} aria-hidden="true" />
  </button>;
}
