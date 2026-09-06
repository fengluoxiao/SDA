import ku100 from "./ku100-comparison-levels.json";
import type {RoomSimulation} from "./vite-env";

export type ComparisonMode="raw"|"calibrated"|"room";
export type ReflectionStage="direct"|"early"|"full";

export function comparisonGain(layout:string,simulation:RoomSimulation,mode:ComparisonMode,stage:ReflectionStage):number {
  const levels=ku100.layouts[layout as keyof typeof ku100.layouts];
  if(!levels)throw new Error("当前布局缺少 KU100 参考电平");
  const room=simulation.comparison.energyDb;
  const energy=mode==="room"?room[stage==="full"?"room":stage]:levels[mode][stage];
  if(energy===undefined||!Number.isFinite(energy))throw new Error("档案缺少分段参考电平，请重新生成房间");
  const reference=Math.min(...Object.values(levels.raw),...Object.values(levels.calibrated),
    ...[room.room,room.direct,room.early].filter((v):v is number=>v!==undefined&&Number.isFinite(v)));
  return Math.max(-40,reference-energy);
}
