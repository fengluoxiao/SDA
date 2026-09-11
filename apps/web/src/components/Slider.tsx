import { forwardRef, type CSSProperties, type InputHTMLAttributes } from "react";

type SliderProps = Omit<InputHTMLAttributes<HTMLInputElement>, "type"> & {
  /** Optional neutral point for bipolar controls such as EQ. */
  origin?: number;
};

/** Native range interaction with a fill that also follows programmatic changes. */
export const Slider = forwardRef<HTMLInputElement, SliderProps>(function Slider(
  { min = 0, max = 100, value, defaultValue, origin, style, onInput, ...props }, ref,
) {
  const lower = Number(min), upper = Number(max);
  const percent = (raw: number) => upper > lower
    ? Math.max(0, Math.min(100, (raw - lower) / (upper - lower) * 100)) : 0;
  const position = percent(Number(value ?? defaultValue ?? (lower + upper) / 2));
  const neutral = percent(origin ?? lower);
  const fill = (position: number) => ({
    "--slider-start": `${Math.min(neutral, position)}%`,
    "--slider-end": `${Math.max(neutral, position)}%`,
    "--slider-origin": `${neutral}%`,
  } as CSSProperties);
  return <input {...props} ref={ref} type="range" min={min} max={max}
    value={value} defaultValue={defaultValue} data-origin={origin === undefined ? undefined : "true"}
    style={{ ...fill(position), ...style }}
    onInput={event => {
      if (value === undefined) {
        const next = fill(percent(event.currentTarget.valueAsNumber));
        Object.entries(next).forEach(([key, val]) => event.currentTarget.style.setProperty(key, String(val)));
      }
      onInput?.(event);
    }} />;
});
