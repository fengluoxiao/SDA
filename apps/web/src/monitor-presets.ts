import type { MonitorSettings } from "./vite-env";

export const MONITOR_PRESETS = [
  {
    id: "transparent",
    name: "透明监听 · 全频输出",
    source: "功能参考 D-MON：通道电平、延时、极性；非硬件仿真",
    url: "https://www.trinnov.com/en/products/d-mon/",
    bassEnabled: false,
  },
  {
    id: "bass-80",
    name: "低频管理 · 80 Hz",
    source: "Neumann KH 750 DSP：80 Hz / 24 dB/oct；采用 SDA LR4 分频",
    url: "https://www.neumann.com/en-en/products/monitors/kh-750-dsp",
    bassEnabled: true,
  },
] as const;

export function createMonitorPreset(id:string,current:MonitorSettings,names:readonly string[]):MonitorSettings {
  const preset=MONITOR_PRESETS.find(p=>p.id===id);
  if(!preset)throw new Error("未知监听预设");
  if(preset.bassEnabled&&!names.includes("LFE"))throw new Error("当前布局没有低音输出");
  return {
    ...current,
    dimDb:-20,
    bassEnabled:preset.bassEnabled,
    crossoverHz:80,
    bassDb:0,
    outputs:Object.fromEntries(names.map(name=>[name,{trimDb:0,delayMs:0,invert:false,muted:current.outputs[name]?.muted??false}])),
  };
}
