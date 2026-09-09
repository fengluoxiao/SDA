import type { MonitorSettings } from "./vite-env";

export const AHB2_SOURCE = "https://benchmarkmedia.com/products/benchmark-ahb2-power-amplifier";
// Stereo operation only. The 8-ohm, 100 W rating describes voltage at the load,
// not the amplifier supply rails. Output impedance is a 1 kHz approximation.
const loadOhms = 8;
const outputOhms = loadOhms / 254;
const loadPeakV = Math.sqrt(2 * 100 * loadOhms);
export const HARDWARE_PRESETS = [
  { id: "ahb2-high", name: "AHB2 · 高增益 · 2 Vrms", gainDb: 23, lineRms: 2 },
  { id: "ahb2-mid", name: "AHB2 · 中增益 · 4 Vrms", gainDb: 17, lineRms: 4 },
  { id: "ahb2-low", name: "AHB2 · 低增益 · 9.8 Vrms", gainDb: 9.2, lineRms: 9.8 },
] as const;

export function createHardwarePreset(id: string, current: MonitorSettings): MonitorSettings {
  const preset = HARDWARE_PRESETS.find(p => p.id === id);
  if (!preset) throw new Error("未知硬件配置");
  return { ...current, hardware: {
    enabled: current.hardware?.enabled ?? false,
    inputDb: 0,
    // AHB2 is an analogue amplifier, not a DAC. Preserve the separate DAC's
    // precision; the source voltage below is the selected input matching level.
    dacBits: current.hardware?.dacBits ?? 24,
    lineRms: preset.lineRms, gainDb: preset.gainDb,
    railV: loadPeakV * (loadOhms + outputOhms) / loadOhms,
    currentA: 29, loadOhms, outputOhms, bandwidthHz: 200000,
  } };
}

export function matchingHardwarePreset(hardware: NonNullable<MonitorSettings["hardware"]>): string {
  return HARDWARE_PRESETS.find(p => {
    const expected = createHardwarePreset(p.id, { hardware } as MonitorSettings).hardware!;
    return (Object.keys(expected) as (keyof typeof expected)[]).every(k =>
      k === "enabled" || k === "dacBits" || k === "inputDb" || Math.abs(Number(expected[k]) - Number(hardware[k])) < 1e-6);
  })?.id ?? "";
}
