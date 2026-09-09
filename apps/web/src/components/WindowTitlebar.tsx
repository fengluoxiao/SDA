import { useEffect, useState } from "react";
import { Minus, Square, Copy, X } from "lucide-react";

export function WindowTitlebar() {
  const [maximized, setMaximized] = useState(false);
  const desktop = window.sdaDesktop;
  useEffect(() => {
    let active = true;
    void desktop?.getWindowMaximized?.().then(value => { if (active) setMaximized(value); });
    const unsubscribe = desktop?.onWindowMaximized?.(setMaximized);
    return () => { active = false; unsubscribe?.(); };
  }, [desktop]);
  if (!desktop?.windowControl) return null;
  return <div className="window-titlebar">
    <span className="window-caption">SDA</span>
    <div className="window-actions">
      <button title="最小化" aria-label="最小化" onClick={() => void desktop.windowControl?.("minimize")}><Minus size={15} /></button>
      <button title={maximized ? "还原窗口" : "最大化"} aria-label={maximized ? "还原窗口" : "最大化"} onClick={() => void desktop.windowControl?.("maximize")}>
        {maximized ? <Copy size={13} /> : <Square size={13} />}
      </button>
      <button className="window-close" title="关闭窗口" aria-label="关闭窗口" onClick={() => void desktop.windowControl?.("close")}><X size={17} /></button>
    </div>
  </div>;
}
