import { Component, lazy, memo, Suspense, type ErrorInfo, type ReactNode } from "react";
import { sphericalToWebAudio, type VirtualSpeaker } from "@sda/renderer";
import type { VisualObject } from "@sda/player";

export type { VisualObject };
export type Theme = "dark" | "light";

const ObjectView3D = lazy(() =>
  import("./ObjectView3D").then(({ ObjectView }) => ({ default: ObjectView })),
);

function clampPercent(value: number): number {
  return Math.min(92, Math.max(8, 50 + value * 38));
}

class WebglErrorBoundary extends Component<{ children: ReactNode; mode: string }, { error: Error | null }> {
  state: { error: Error | null } = { error: null };

  static getDerivedStateFromError(error: Error): { error: Error } {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error("[SDA] WebGL 初始化失败", error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div className="flat-view webgl-error" role="alert">
          <strong>WebGL 初始化失败</strong>
          <span>{this.state.error.message || "three.js 无法创建渲染上下文"}</span>
          <small>当前模式：{this.props.mode}。可用 SDA_ELECTRON_RENDERER=2d 启动兼容视图。</small>
        </div>
      );
    }
    return this.props.children;
  }
}

/** CSS-only top-down view used by Electron's explicit 2D mode. */
function FlatObjectView({
  objects,
  layout,
  theme,
  mutedIds,
  soundingIds,
}: {
  objects: VisualObject[];
  layout: readonly VirtualSpeaker[];
  theme: Theme;
  mutedIds?: ReadonlySet<number>;
  soundingIds?: ReadonlySet<number>;
}) {
  const background = theme === "light" ? "#e9edf4" : "#0c101c";
  return (
    <div className="flat-view" style={{ background }} aria-label="二维空间对象视图">
      <div className="flat-room">
        <div className="flat-room-grid" />
        <div className="flat-front-wall">前方</div>
        <div className="flat-listener" title="听者">●</div>
        {layout.filter((s) => !s.isLfe).map((speaker) => {
          const [x, , z] = sphericalToWebAudio(speaker);
          return (
            <span
              className="flat-speaker"
              key={speaker.name}
              title={speaker.name}
              style={{ left: `${clampPercent(x)}%`, top: `${clampPercent(-z)}%` }}
            />
          );
        })}
        {objects.map((object) => {
          const muted = mutedIds?.has(object.id) ?? false;
          const sounding = !muted && (soundingIds?.has(object.id) ?? false);
          return (
            <span
              className={`flat-object${muted ? " muted" : ""}${sounding ? " sounding" : ""}`}
              key={object.id}
              title={`对象 #${object.id}`}
              style={{ left: `${clampPercent(object.pos[0])}%`, top: `${clampPercent(-object.pos[1])}%` }}
            >
              <b>#{object.id}</b>
              {object.pos[2] > 0.15 && <i>↑</i>}
            </span>
          );
        })}
      </div>
      <div className="flat-caption">
        <strong>二维兼容视图</strong>
        <span>{objects.length} 个对象 · Electron 已禁用 WebGL</span>
      </div>
    </div>
  );
}

export const ObjectView = memo(function ObjectView({
  objects,
  layout,
  theme = "dark",
  mutedIds,
  soundingIds,
}: {
  objects: VisualObject[];
  layout: readonly VirtualSpeaker[];
  theme?: Theme;
  mutedIds?: ReadonlySet<number>;
  soundingIds?: ReadonlySet<number>;
}) {
  const desktop = window.sdaDesktop;
  if (desktop && desktop.electron3D !== true) {
    return <FlatObjectView objects={objects} layout={layout} theme={theme} mutedIds={mutedIds} soundingIds={soundingIds} />;
  }

  const rendererMode = desktop?.rendererMode ?? "browser";
  return (
    <WebglErrorBoundary mode={rendererMode}>
      <Suspense fallback={<div className="flat-view" aria-label="正在加载三维视图">正在加载三维视图…</div>}>
        <ObjectView3D objects={objects} layout={layout} theme={theme} mutedIds={mutedIds} soundingIds={soundingIds} />
      </Suspense>
    </WebglErrorBoundary>
  );
}, (previous, next) => previous.objects === next.objects
  && previous.layout === next.layout
  && previous.theme === next.theme
  && previous.mutedIds === next.mutedIds
  && previous.soundingIds === next.soundingIds);
