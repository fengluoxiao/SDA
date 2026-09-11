const clone = value => JSON.parse(JSON.stringify(value));
const roomOnly = settings => { const {monitor, ...room} = settings; return room; };
const hardwareDefault = {enabled:false,inputDb:0,dacBits:24,lineRms:2,gainDb:26,railV:28,currentA:7,loadOhms:8,outputOhms:.05,bandwidthHz:60000};
function el(tag, text, cls) { const node=document.createElement(tag); if(text)node.textContent=text; if(cls)node.className=cls; return node; }
export function createTools(send,request) {
  const dialog=document.getElementById("sound-tools"), content=document.getElementById("tools-content"), note=document.getElementById("tools-note");
  let state=null, online=false, page="room", draft=null, expected="", dirty=false, pending=null, signature="";
  let playing=false, test=null;
  let pendingAction="";
  const config={length:6,width:5,height:3.2,earHeight:1.2,placement:.7,listeningDistance:1.2,material:"studio",order:10};
  const cancelGeneration=document.getElementById("tools-cancel-generation");
  cancelGeneration.onclick=()=>{send("roomCancel");report("正在取消房间生成…");};
  const report=text=>{note.textContent=text;};
  const mark=()=>{dirty=true;report("尚未应用");};
  function issue(action,value) {
    if(pending||!online)return;
    const id=send(action,value);
    if(!id){report("尚未连接主机");return;}
    pending=id;pendingAction=action;cancelGeneration.hidden=action!=="roomGenerate";report(action==="roomGenerate"?"主机正在生成房间…":"等待主机应用…");lock();
  }
  function lock() {
    for(const control of content.querySelectorAll("button,input"))control.disabled=!online||!!pending||!!state?.locked;
  }
  function button(text,fn,primary=false) { const b=el("button",text,primary?"primary":"");b.type="button";b.onclick=fn;return b; }
  function toggle(parent,obj,key,label) {
    const row=el("label",null,"tool-switch"),input=el("input");input.type="checkbox";input.checked=!!obj[key];input.setAttribute("role","switch");input.setAttribute("aria-label",label);
    input.onchange=()=>{obj[key]=input.checked;mark();};row.append(el("span",label),input);parent.append(row);
  }
  function number(parent,obj,key,label,min,max,step,unit="") {
    const row=el("label",null,"tool-number"),input=el("input");input.type="number";input.value=obj[key];input.min=min;input.max=max;input.step=["dacBits","order"].includes(key)?1:"any";input.setAttribute("aria-label",label);
    input.oninput=()=>{if(input.validity.valid&&Number.isFinite(input.valueAsNumber)){obj[key]=input.valueAsNumber;mark();}};
    row.append(el("span",label),input,el("small",unit));parent.append(row);
  }
  function choice(parent,label,items,value,onChange) {
    const wrap=el("div",null,"tool-choice"),title=el("span",label,"label"),trigger=button(items.find(x=>x.id===value)?.name||"请选择",()=>{list.hidden=!list.hidden;trigger.setAttribute("aria-expanded",String(!list.hidden));if(!list.hidden)list.querySelector("button")?.focus();});
    trigger.setAttribute("aria-label",label);trigger.setAttribute("aria-haspopup","listbox");trigger.setAttribute("aria-expanded","false");
    const list=el("div",null,"tool-options");list.role="listbox";list.hidden=true;
    for(const item of items){const option=button(item.name,()=>{trigger.textContent=item.name;list.hidden=true;trigger.setAttribute("aria-expanded","false");onChange(item.id);trigger.focus();});option.role="option";option.setAttribute("aria-selected",String(item.id===value));list.append(option);}
    list.onkeydown=e=>{const options=[...list.children],index=options.indexOf(document.activeElement);if(e.key==="Escape"){e.stopPropagation();list.hidden=true;trigger.focus();}if(["ArrowDown","ArrowUp","Home","End"].includes(e.key)){e.preventDefault();options[e.key==="Home"?0:e.key==="End"?options.length-1:(index+(e.key==="ArrowDown"?1:-1)+options.length)%options.length]?.focus();}};
    wrap.append(title,trigger,list);parent.append(wrap);
  }
  function section(title) {const part=el("fieldset",null,"tool-section");part.append(el("legend",title));content.append(part);return part;}
  function fresh() {
    if(!state)return;
    draft=clone(page==="room"?roomOnly(state.cinema.settings):page==="monitor"?state.cinema.settings.monitor:{head:state.head,dense:state.dense,calibrated:state.calibrated});
    expected=JSON.stringify(page==="room"?{profileId:state.cinema.profileId,settings:draft}:draft);
    dirty=false;render();
  }
  function render() {
    content.replaceChildren();if(!state){content.append(el("p","正在读取主机音频设置…"));return;}
    document.getElementById("tools-title").textContent={room:"房间",monitor:"监听",hrtf:"耳廓"}[page];
    const header=el("p",`${state.layout} · 修改在主机生效`,"muted");content.append(header);
    if(page==="room") {
      const part=section("房间仿真");
      toggle(part,draft,"enabled","启用房间");
      choice(part,"房间档案",state.rooms.filter(r=>r.layout===state.layout).map(r=>({id:r.id,name:(r.builtin?"内置 · ":"")+r.name})),state.cinema.profileId,id=>issue("roomApply",id));
      choice(part,"反射试听",[{id:"direct",name:"直达声"},{id:"early",name:"直达声与早期反射"},{id:"full",name:"完整房间"}],draft.reflectionMode||"full",id=>{draft.reflectionMode=id;mark();});
      for(const args of [["directDb","直达声",-24,6,.5,"dB"],["earlyDb","早期反射",-40,6,.5,"dB"],["lateDb","晚期混响",-40,6,.5,"dB"],["earlyMs","早期反射分界",10,100,1,"ms"]])number(part,draft,...args);
      const channels=section("音箱校准");
      for(const speaker of state.speakers){const details=el("details");details.append(el("summary",speaker.label));const o=draft.speakers[speaker.name]??={gainDb:0,delayMs:0,lowDb:0,highDb:0};number(details,o,"gainDb",`${speaker.label} 房间电平`,-24,6,.1,"dB");number(details,o,"delayMs",`${speaker.label} 房间延时`,0,20,.1,"ms");if(speaker.name!=="LFE"){number(details,o,"lowDb",`${speaker.label} 低频`,-6,6,.1,"dB");number(details,o,"highDb",`${speaker.label} 高频`,-6,6,.1,"dB");}channels.append(details);}
      if(state.generator?.available){
        const generation=section("创建房间档案"),details=el("details");details.append(el("summary","自定义尺寸与材料"));generation.append(details);
        for(const args of [["length","房间长度",3,10,.1,"m"],["width","房间宽度",3,8,.1,"m"],["height","房间高度",2.2,4,.1,"m"],["earHeight","耳朵高度",.8,1.6,.1,"m"],["placement","摆位比例",.5,1,.05,""],["listeningDistance","监听距离",.8,2.5,.1,"m"],["order","反射阶数",1,12,1,""]])number(details,config,...args);
        choice(details,"房间材料",[{id:"studio",name:"录音棚"},{id:"rockwool_50mm_80kgm3",name:"50 mm 岩棉"},{id:"plasterboard",name:"石膏板"},{id:"hard_surface",name:"硬质表面"}],config.material,id=>{config.material=id;mark();});
        details.append(button("在主机生成房间",()=>{if([...details.querySelectorAll("input")].every(input=>input.reportValidity()))issue("roomGenerate",{...config,layout:state.layout});}),el("p","完成后从房间档案中选择并应用。","muted"));
      }
    } else if(page==="monitor") {
      const part=section("监听电平");toggle(part,draft,"enabled","启用监听处理器");number(part,draft,"levelDb","监听衰减",-80,0,.5,"dB");toggle(part,draft,"dim","DIM");number(part,draft,"dimDb","DIM 衰减",-40,0,.5,"dB");toggle(part,draft,"muted","总静音");
      choice(part,"内置监听配置",[{id:"transparent",name:"透明监听 · 全频输出"},...(state.speakers.some(s=>s.name==="LFE")?[{id:"bass-80",name:"低频管理 · 80 Hz"}]:[])],"",id=>issue("monitorPreset",id));
      const outputs=section("输出通道");outputs.append(button("对齐当前房间",()=>issue("monitorAlign")));
      for(const speaker of state.speakers){const details=el("details");details.append(el("summary",speaker.label));const o=draft.outputs[speaker.name]??={trimDb:0,delayMs:0,invert:false,muted:false};number(details,o,"trimDb",`${speaker.label} 监听电平`,-24,6,.1,"dB");number(details,o,"delayMs",`${speaker.label} 监听延时`,0,20,.1,"ms");toggle(details,o,"invert",`${speaker.label} 反相`);toggle(details,o,"muted",`${speaker.label} 静音`);outputs.append(details);}
      const bass=section("低频管理");toggle(bass,draft,"bassEnabled","启用低频管理");number(bass,draft,"crossoverHz","LR4 分频点",40,160,1,"Hz");number(bass,draft,"bassDb","重定向低频电平",-24,6,.5,"dB");
      const hardware=section("硬件链路");draft.hardware={...hardwareDefault,...draft.hardware};toggle(hardware,draft.hardware,"enabled","启用硬件链路");
      choice(hardware,"功放参数配置",[{id:"ahb2-high",name:"AHB2 · 高增益 · 2 Vrms"},{id:"ahb2-mid",name:"AHB2 · 中增益 · 4 Vrms"},{id:"ahb2-low",name:"AHB2 · 低增益 · 9.8 Vrms"}],"",id=>issue("hardwarePreset",id));
      hardware.append(el("p","规格约束的电路近似，与主机使用同一处理器。","muted"));
      for(const args of [["inputDb","输入增益",-60,12,.5,"dB"],["dacBits","DAC 位深",8,24,1,"bit"],["lineRms","满幅线路输出",.1,12,.1,"Vrms"],["gainDb","功放增益",0,40,.1,"dB"],["railV","等效峰值电压上限",1,80,.01,"V"],["currentA","峰值电流上限",.01,30,.01,"A"],["loadOhms","负载阻抗",2,600,.1,"Ω"],["outputOhms","输出阻抗",0,20,.0001,"Ω"],["bandwidthHz","标称带宽近似",5000,250000,100,"Hz"]])number(hardware,draft.hardware,...args);
    } else {
      const part=section("播放使用的 HRTF");choice(part,"耳廓档案",state.heads,state.head,id=>{draft.head=id;mark();});part.append(el("p","个人档案与内置测量库均来自主机。切换后由主机重新渲染，手机接收最终双耳音频。","muted"));
      if(state.head.startsWith("personal-")) {
        const label=el("label",null,"tool-number"),name=el("input");name.type="text";name.maxLength=80;name.value=state.heads.find(h=>h.id===state.head)?.name||"个人档案";name.setAttribute("aria-label","个人档案名称");name.oninput=mark;label.append(el("span","当前档案名称"),name);part.append(label);
        part.append(button("保存名称",()=>issue("hrtfRename",{id:state.head,name:name.value})),button("另存副本",()=>issue("hrtfCopy",{id:state.head,name:`${name.value.slice(0,76)} 副本`})));
      }
      if(state.head==="ku100") {toggle(part,draft,"calibrated","KU100 数据校准");toggle(part,draft,"dense","高解析逐对象 HRTF");part.append(button("应用 KU100 设置",()=>issue("hrtfTune",{dense:!!draft.dense,calibrated:!!draft.calibrated})));}
      part.append(button("创建个人耳廓 · 感知测试",async()=>{
        if(playing){report("请先暂停歌曲，再开始感知测试");return;}
        try{const {openPhrtfTest}=await import("./phrtf-test.mjs");test?.close();test=openPhrtfTest(state,request);}catch(e){report(e.message);}
      }));
    }
    const actions=el("div",null,"tool-actions");actions.append(button("撤销更改",()=>{fresh();report("已读取主机当前配置");}),button("应用",()=>{
      if(![...content.querySelectorAll("input")].every(input=>input.reportValidity()))return;
      issue(page==="room"?"roomSettings":page==="monitor"?"monitorSettings":"hrtf",page==="hrtf"?draft.head:{settings:draft,expected});
    },true));content.append(actions);lock();
  }
  document.getElementById("tools-close").onclick=()=>dialog.close();
  dialog.addEventListener("click",e=>{if(e.target===dialog)dialog.close();});
  for(const b of document.querySelectorAll("[data-tool]"))b.onclick=()=>{document.getElementById("player-settings").open=false;if(page!==b.dataset.tool){page=b.dataset.tool;fresh();report("");}else if(!draft)fresh();if(!dialog.open)dialog.showModal();};
  return {
    update(next,connected,isPlaying=false) {
      playing=isPlaying;
      test?.update(next,connected,isPlaying);
      online=connected;for(const b of document.querySelectorAll("[data-tool]"))b.disabled=!connected||!next;
      if(!next){state=null;lock();return;}
      const changed=JSON.stringify(next)!==signature;signature=JSON.stringify(next);state=next;
      if(changed&&!dirty&&!pending)fresh();else lock();
      if(next.locked)report("主机正在切换音频或对照试听，请稍候");
      else if(pendingAction==="roomGenerate"&&next.generator?.running)report(`主机正在生成房间 · ${next.generator.current} / ${next.generator.total}`);
      else if(next.error)report(next.error);
    },
    acknowledged(id,error) {if(id!==pending)return;pending=null;const action=pendingAction;pendingAction="";cancelGeneration.hidden=true;if(!error){dirty=false;fresh();}report(error||(action==="roomGenerate"?"房间已生成，请从档案中选择应用":"主机已应用"));lock();},
    disconnected() {test?.close();test=null;cancelGeneration.hidden=true;pendingAction="";pending=null;online=false;lock();report("连接已断开，重新连接后可继续编辑");for(const b of document.querySelectorAll("[data-tool]"))b.disabled=true;},
  };
}
