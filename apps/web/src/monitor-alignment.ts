import type {CinemaRoomSummary,CinemaSettings,MonitorSettings} from "./vite-env";

export function alignmentContext(settings:CinemaSettings,profileId:string|null,layout:string):string {
  return JSON.stringify({profileId,layout,enabled:settings.enabled,speakers:Object.entries(settings.speakers).sort(([a],[b])=>a.localeCompare(b))});
}

export function alignMonitorToRoom(room:CinemaRoomSummary,cinema:CinemaSettings,current:MonitorSettings,names:readonly string[],layout:string):MonitorSettings {
  if(!room.builtin||!cinema.enabled||room.layout!==layout)throw new Error("请先应用与当前布局匹配的内置房间");
  const channels=names.filter(name=>name!=="LFE");
  if(!channels.length)throw new Error("当前布局没有可对齐通道");
  const rows=channels.map(name=>{
    const matches=room.rows.filter(row=>row.name===name);
    if(matches.length!==1)throw new Error("房间缺少通道响应："+name);
    const row=matches[0]!,cal=cinema.speakers[name];
    if(cal&&(cal.lowDb!==0||cal.highDb!==0))throw new Error("请先清除房间通道 EQ，再生成直达声对齐");
    const arrival=row.arrivalMs+(cal?.delayMs??0),energy=row.directEnergyDb+(cal?.gainDb??0);
    if(!Number.isFinite(arrival)||!Number.isFinite(energy))throw new Error("房间响应分析无效");
    return {name,arrival,energy};
  });
  const latest=Math.max(...rows.map(row=>row.arrival)),quietest=Math.min(...rows.map(row=>row.energy));
  const outputs={...current.outputs};
  for(const row of rows){
    const trimDb=quietest-row.energy,delayMs=latest-row.arrival;
    if(trimDb < -24-1e-6||delayMs>20+1e-6)throw new Error("所需补偿超出监听处理器范围，请调整房间参数");
    outputs[row.name]={trimDb:Math.max(-24,Math.round(trimDb*1000)/1000),delayMs:Math.min(20,Math.round(delayMs*1000)/1000),invert:false,muted:current.outputs[row.name]?.muted??false};
  }
  return {...current,outputs};
}
