import { useEffect, useId, useRef, useState } from "react";

// Generate only when the panel changes size. Chromium refracts the live backdrop;
// no scene readback, per-frame JS, or work on the audio thread is needed.
export function GlassRefraction() {
  const ref = useRef<HTMLDivElement>(null);
  const id = "glass-" + useId().replace(/:/g, "");
  const [map, setMap] = useState<{ url: string; width: number; height: number }>();
  useEffect(() => {
    const element = ref.current;
    if (!element || window.sdaDesktop?.rendererMode === "swiftshader") return;
    let frame = 0;
    const update = () => {
      const { width, height } = element.getBoundingClientRect();
      if (width < 1 || height < 1) return;
      const ratio = Math.min(1, 1200 / width);
      const canvas = document.createElement("canvas");
      canvas.width = Math.ceil(width * ratio); canvas.height = Math.ceil(height * ratio);
      const context = canvas.getContext("2d");
      if (!context) return;
      const pixels = context.createImageData(canvas.width, canvas.height);
      const radius = Math.min(22, height / 2), bevel = Math.min(40, height * 0.48);
      for (let y = 0; y < canvas.height; y++) {
        for (let x = 0; x < canvas.width; x++) {
          const px = (x + 0.5) / ratio - width / 2, py = (y + 0.5) / ratio - height / 2;
          const qx = Math.abs(px) - (width / 2 - radius), qy = Math.abs(py) - (height / 2 - radius);
          const ox = Math.max(qx, 0), oy = Math.max(qy, 0), length = Math.hypot(ox, oy);
          const distance = length + Math.min(Math.max(qx, qy), 0) - radius;
          const depth = -distance;
          const t = Math.max(0, Math.min(1, depth / bevel));
          const bend = 16 * t * t * (1 - t) * (1 - t);
          const nx = length > 0 ? ox / length : qx > qy ? 1 : 0;
          const ny = length > 0 ? oy / length : qx > qy ? 0 : 1;
          const i = (y * canvas.width + x) * 4;
          pixels.data[i] = Math.round(127.5 - Math.sign(px) * nx * bend * 110);
          pixels.data[i + 1] = Math.round(127.5 - Math.sign(py) * ny * bend * 110);
          pixels.data[i + 2] = 128; pixels.data[i + 3] = 255;
        }
      }
      context.putImageData(pixels, 0, 0);
      setMap({ url: canvas.toDataURL(), width, height });
    };
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame); frame = requestAnimationFrame(update);
    });
    observer.observe(element);
    return () => { observer.disconnect(); cancelAnimationFrame(frame); };
  }, []);
  return <>
    <svg className="mp-filter-defs" aria-hidden="true" width="0" height="0">
      <defs>
        <filter id={id} x="0" y="0" width="100%" height="100%" colorInterpolationFilters="sRGB">
          {map && <feImage href={map.url} x="0" y="0" width={map.width} height={map.height} preserveAspectRatio="none" result="lens" />}
          <feDisplacementMap in="SourceGraphic" in2="lens" scale="18" xChannelSelector="R" yChannelSelector="G" />
        </filter>
      </defs>
    </svg>
    <div ref={ref} className="mp-refraction" aria-hidden="true" style={map ? { backdropFilter: `url("#${id}")`, WebkitBackdropFilter: `url("#${id}")` } : undefined} />
  </>;
}
